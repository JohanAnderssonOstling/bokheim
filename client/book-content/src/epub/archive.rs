//! Immutable ZIP directory metadata, independent async sources, and owned cursors.

use std::collections::HashMap;
use std::io;
use std::sync::Arc;

use async_zip::base::read1::{
    ZipOptions,
    seek::{ZipArchiveInner, ZipArchiveReader},
};
use futures_lite::io::AsyncReadExt;

use super::{MAX_ARCHIVE_ENTRIES, MAX_BINARY_RESOURCE_BYTES, MAX_PACKAGE_DOCUMENT_BYTES};

use crate::source::{ReaderFactory, Source};

pub(crate) struct ArchiveMetadata {
    inner: Arc<ZipArchiveInner>,
    entries: HashMap<String, usize>,
    names: Vec<String>,
}

impl ArchiveMetadata {
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
    pub(crate) async fn open(source: Source) -> io::Result<Self> {
        Self::open_with_limit(source, MAX_BINARY_RESOURCE_BYTES.max(MAX_PACKAGE_DOCUMENT_BYTES)).await
    }
    pub(crate) async fn open_with_limit(mut source: Source, limit: u64) -> io::Result<Self> {
        let options = ZipOptions {
            max_cd_num_files: MAX_ARCHIVE_ENTRIES as u64,
            max_cd_num_files_load: MAX_ARCHIVE_ENTRIES as u64,
            max_uncompressed_size_per_file: limit,
            max_compressed_size_per_file: limit,
            ..ZipOptions::untrusted()
        };
        let reader = ZipArchiveReader::open_with_options(&mut source.reader, options).await.map_err(io::Error::other)?;
        let metadata = Arc::new(ArchiveMetadata::new(reader.inner().clone())?);
        Ok(Self { metadata, source })
    }
    pub(crate) async fn read(&mut self, uri: &str, limit: u64) -> io::Result<Vec<u8>> {
        if self.metadata.size(uri)? > limit {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "EPUB resource exceeds the size limit"));
        }
        self.read_prefix(uri, limit.saturating_add(1)).await.and_then(|bytes| if bytes.len() as u64 > limit { Err(io::Error::new(io::ErrorKind::InvalidData, "EPUB resource exceeds the size limit")) } else { Ok(bytes) })
    }
    pub(crate) async fn read_prefix(&mut self, uri: &str, length: u64) -> io::Result<Vec<u8>> {
        let mut reader = ZipArchiveReader::new_with_inner(&mut self.source.reader, self.metadata.inner.clone());
        let mut entry = reader.file(self.metadata.index(uri)?).await.map_err(io::Error::other)?.take(length);
        let mut bytes = Vec::with_capacity(length.min(256 * 1024) as usize);
        entry.read_to_end(&mut bytes).await?;
        Ok(bytes)
    }
    pub(crate) async fn into_archive(self) -> io::Result<Archive> {
        Ok(Archive { metadata: self.metadata, factory: self.source.into_factory().await? })
    }
}
