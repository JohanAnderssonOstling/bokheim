//! Complete prepared text, independent of its original container and transport.

use super::{ReadingBook, ResourceSource};
use crate::{BookTocEntry, PreparedResources};
use futures_util::future::BoxFuture;
use std::{
    io::{self, BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write},
    sync::Arc,
};

const MAGIC: &[u8; 8] = b"BKHTXT\x02\0";
const HEADER_BYTES: usize = MAGIC.len() + blake3::OUT_LEN;
const BUFFER_BYTES: usize = 64 * 1024;

pub type ResourceFetch = Arc<dyn Fn(String) -> BoxFuture<'static, io::Result<Vec<u8>>> + Send + Sync>;

pub(super) struct CachedResources {
    pub(super) cfi_bases: Option<Vec<Option<String>>>,
    pub(super) languages: Vec<String>,
    pub(super) fetch: ResourceFetch,
}

type CachedText = (PreparedResources, Vec<u64>, Vec<BookTocEntry>, Option<Vec<Option<String>>>, Vec<String>);

struct HashingWriter<W> {
    inner: W,
    hasher: blake3::Hasher,
}

impl<W: Write> Write for HashingWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let count = self.inner.write(bytes)?;
        self.hasher.update(&bytes[..count]);
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl ReadingBook {
    /// Stream complete prepared text into a private, seekable staging file.
    /// Returns false without writing when preparation skipped a resource.
    /// Publication and durability remain the caller's responsibility.
    pub fn write_cache<W: Write + Seek + ?Sized>(&self, writer: &mut W) -> io::Result<bool> {
        if !self.cache_complete {
            return Ok(false);
        }
        let data = (&self.resources, &self.document_weights, &self.toc, self.publication_cfi_bases(), self.languages());
        let start = writer.stream_position()?;
        writer.write_all(&[0; HEADER_BYTES])?;
        // Buffer before hashing so small serializer writes become bounded
        // chunks for both the incremental hash and the underlying file.
        let mut payload = BufWriter::with_capacity(BUFFER_BYTES, HashingWriter { inner: &mut *writer, hasher: blake3::Hasher::new() });
        bincode::serde::encode_into_std_write(data, &mut payload, bincode::config::standard()).map_err(|error| match error {
            bincode::error::EncodeError::Io { inner, .. } => inner,
            error => io::Error::other(error),
        })?;
        payload.flush()?;
        let HashingWriter { inner: writer, hasher } = payload.into_inner().map_err(|error| error.into_error())?;
        let end = writer.stream_position()?;
        writer.seek(SeekFrom::Start(start))?;
        writer.write_all(MAGIC)?;
        writer.write_all(hasher.finalize().as_bytes())?;
        writer.seek(SeekFrom::Start(end))?;
        writer.flush()?;
        Ok(true)
    }

    /// Restore text from an immutable cache file without opening the original
    /// container. Verify the checksum before decoding, reusing one bounded buffer
    /// for both passes. The fetcher is only called for absent resources like images.
    pub fn read_cache<R: Read + Seek + ?Sized>(reader: &mut R, fetch: ResourceFetch) -> io::Result<Self> {
        let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid prepared text cache");
        let mut payload = BufReader::with_capacity(BUFFER_BYTES, reader);
        let mut header = [0; HEADER_BYTES];
        payload.read_exact(&mut header)?;
        if !header.starts_with(MAGIC) {
            return Err(invalid());
        }
        let start = payload.stream_position()?;
        let mut hasher = blake3::Hasher::new();
        loop {
            let bytes = payload.fill_buf()?;
            if bytes.is_empty() {
                break;
            }
            hasher.update(bytes);
            let count = bytes.len();
            payload.consume(count);
        }
        if hasher.finalize().as_bytes() != &header[MAGIC.len()..] {
            return Err(invalid());
        }
        // Atomic cache replacement keeps this open file's bytes stable across
        // verification and decoding, including pinned browser generations.
        payload.seek(SeekFrom::Start(start))?;
        let (resources, document_weights, toc, cfi_bases, languages): CachedText = bincode::serde::decode_from_std_read(&mut payload, bincode::config::standard()).map_err(io::Error::other)?;
        let documents = resources.documents();
        if !payload.fill_buf()?.is_empty() || documents.is_empty() || documents.len() != document_weights.len() || cfi_bases.as_ref().is_some_and(|bases| bases.len() != documents.len()) || documents.iter().any(|uri| !resources.exists(uri))
        {
            return Err(invalid());
        }
        Ok(Self { resources, document_weights, toc, source: ResourceSource::Cached(Arc::new(CachedResources { cfi_bases, languages, fetch })), cache_complete: true })
    }
}
