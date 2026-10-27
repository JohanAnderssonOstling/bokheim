//! Immutable ZIP directory metadata, independent async sources, and owned cursors.

use std::collections::HashMap;
use std::io;
use std::sync::Arc;
use std::time::Instant;

use async_zip::base::read1::{
    ZipOptions,
    seek::{ZipArchiveInner, ZipArchiveReader},
};
use futures_lite::io::AsyncReadExt;

use super::{MAX_ARCHIVE_ENTRIES, MAX_BINARY_RESOURCE_BYTES, MAX_PACKAGE_DOCUMENT_BYTES};

use crate::profile::{PreparationTiming, ResourceReadTiming, measure};
use crate::source::{ReaderFactory, Source};

pub(crate) struct ArchiveMetadata {
    inner: Arc<ZipArchiveInner>,
    entries: HashMap<String, usize>,
    names: Vec<String>,
}

impl ArchiveMetadata {
    pub(crate) fn record_ranges(&self, names: &[String]) -> io::Result<Vec<(u64, usize)>> {
        let directory = self.inner.ceocdr().cd_offset().map_err(io::Error::other)?;
        let mut offsets = self.inner.cdrs().iter().map(|entry| entry.lfh_offset().map_err(io::Error::other)).collect::<io::Result<Vec<_>>>()?;
        offsets.push(directory);
        offsets.sort_unstable();
        offsets.dedup();
        let mut ranges = Vec::new();
        for name in names {
            let start = self.inner.cdrs()[self.index(name)?].lfh_offset().map_err(io::Error::other)?;
            let end = offsets.get(offsets.partition_point(|&offset| offset <= start)).copied().filter(|&end| end <= directory).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid ZIP entry span"))?;
            let count = usize::try_from(end - start).map_err(io::Error::other)?;
            ranges.push((start, count));
        }
        ranges.sort_unstable();
        ranges.dedup();
        let mut merged: Vec<(u64, usize)> = Vec::new();
        for (start, count) in ranges {
            if let Some((offset, length)) = merged.last_mut() {
                if *offset + *length as u64 == start {
                    *length = length.checked_add(count).ok_or_else(|| io::Error::other("ZIP entry span overflow"))?;
                    continue;
                }
            }
            merged.push((start, count));
        }
        Ok(merged)
    }
    fn new(inner: Arc<ZipArchiveInner>) -> io::Result<Self> {
        let mut entries = HashMap::new();
        let mut names = Vec::new();
        for (index, entry) in inner.cdrs().iter().enumerate() {
            let name = String::from_utf8(entry.insecure_file_name.as_bytes().to_vec()).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            if !entries.contains_key(&name) {
                entries.insert(name.clone(), index);
                names.push(name);
            }
        }
        Ok(Self { inner, entries, names })
    }
    fn index(&self, uri: &str) -> io::Result<usize> {
        self.entries.get(uri).copied().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("EPUB entry not found: {uri}")))
    }
    pub(crate) fn contains(&self, uri: &str) -> bool {
        self.entries.contains_key(uri)
    }
    pub(crate) fn names(&self) -> &[String] {
        &self.names
    }
    pub(crate) fn size(&self, uri: &str) -> io::Result<u64> {
        self.inner.cdrs()[self.index(uri)?].uncompressed_size().map_err(io::Error::other)
    }
}

pub(crate) struct Archive {
    metadata: Arc<ArchiveMetadata>,
    factory: ReaderFactory,
}
impl Archive {
    pub(crate) fn reader(&self) -> ArchiveReader {
        ArchiveReader { metadata: self.metadata.clone(), source: Source::reopenable(self.factory.clone()) }
    }
    pub(crate) async fn read(&self, uri: &str, limit: u64) -> io::Result<Vec<u8>> {
        self.reader().read(uri, limit).await
    }
}

/// A single cursor owned by a sequential inspection or preparation session.
/// The source keeps its buffering and position across ZIP entry decoders.
pub(crate) struct ArchiveReader {
    pub(crate) metadata: Arc<ArchiveMetadata>,
    source: Source,
}
impl ArchiveReader {
    pub(super) fn with_metadata(metadata: Arc<ArchiveMetadata>, source: Source) -> Self {
        Self { metadata, source }
    }
    pub(crate) async fn open(source: Source) -> io::Result<Self> {
        Self::open_with_limit(source, MAX_BINARY_RESOURCE_BYTES.max(MAX_PACKAGE_DOCUMENT_BYTES)).await
    }
    pub(crate) async fn open_with_limit(mut source: Source, limit: u64) -> io::Result<Self> {
        let options = ZipOptions { max_cd_num_files: MAX_ARCHIVE_ENTRIES as u64, max_cd_num_files_load: MAX_ARCHIVE_ENTRIES as u64, max_uncompressed_size_per_file: limit, max_compressed_size_per_file: limit, ..ZipOptions::untrusted() };
        let reader = ZipArchiveReader::open_with_options(&mut source.reader, options).await.map_err(io::Error::other)?;
        let metadata = Arc::new(ArchiveMetadata::new(reader.inner().clone())?);
        Ok(Self { metadata, source })
    }
    pub(crate) async fn read(&mut self, uri: &str, limit: u64) -> io::Result<Vec<u8>> {
        self.read_measured::<false>(uri, limit).await.map(|(bytes, _)| bytes)
    }
    pub(crate) async fn read_measured<const PROFILE: bool>(&mut self, uri: &str, limit: u64) -> io::Result<(Vec<u8>, ResourceReadTiming)> {
        if self.metadata.size(uri)? > limit {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "EPUB resource exceeds the size limit"));
        }
        let (bytes, timing) = self.read_prefix_measured::<PROFILE>(uri, limit.saturating_add(1)).await?;
        if bytes.len() as u64 > limit {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "EPUB resource exceeds the size limit"));
        }
        Ok((bytes, timing))
    }
    pub(crate) async fn read_prefix(&mut self, uri: &str, length: u64) -> io::Result<Vec<u8>> {
        self.read_prefix_measured::<false>(uri, length).await.map(|(bytes, _)| bytes)
    }
    async fn read_prefix_measured<const PROFILE: bool>(&mut self, uri: &str, length: u64) -> io::Result<(Vec<u8>, ResourceReadTiming)> {
        let mut reader = ZipArchiveReader::new_with_inner(&mut self.source.reader, self.metadata.inner.clone());
        let index = self.metadata.index(uri)?;
        let (entry, open) = measure::<PROFILE, _>(reader.file(index)).await;
        let mut entry = entry.map_err(io::Error::other)?.take(length);
        let started = PROFILE.then(Instant::now);
        let mut bytes = Vec::with_capacity(length.min(256 * 1024) as usize);
        let allocate = started
            .map(|started| {
                let elapsed = started.elapsed();
                PreparationTiming { elapsed, active: elapsed, polls: 1 }
            })
            .unwrap_or_default();
        let (result, decode) = measure::<PROFILE, _>(entry.read_to_end(&mut bytes)).await;
        result?;
        let timing = if PROFILE {
            ResourceReadTiming { open, allocate, decode, compressed_bytes: self.metadata.inner.cdrs()[index].compressed_size().map_err(io::Error::other)?, output_bytes: bytes.len(), output_capacity: bytes.capacity() }
        } else {
            ResourceReadTiming::default()
        };
        Ok((bytes, timing))
    }
    pub(crate) async fn into_archive(self) -> io::Result<Archive> {
        Ok(Archive { metadata: self.metadata, factory: self.source.into_factory().await? })
    }
}
