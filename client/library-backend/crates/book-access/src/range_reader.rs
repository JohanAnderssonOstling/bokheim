//! Position, cache, and read-ahead policy for local or remote book bytes.

pub(crate) use crate::range_cache::BLOCK_BYTES;
use crate::range_cache::{seek_position, RangeCache};
use futures_util::future::BoxFuture;
use futures_util::io::{AsyncRead, AsyncSeek};
use std::io::{self, SeekFrom};
use std::pin::Pin;
use std::task::{Context, Poll};

const CACHE_BLOCKS: usize = 64;
pub(crate) const MAX_RANGE_BYTES: usize = 4 * BLOCK_BYTES;

struct RangeState {
    length: u64,
    position: u64,
    cache: RangeCache,
    next_fetch_offset: Option<u64>,
    batch_blocks: usize,
}

pub(crate) fn validate_prefix(length: u64, bytes: &[u8]) -> io::Result<()> {
    if bytes.len() != length.min(BLOCK_BYTES as u64) as usize || bytes.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid book prefix"));
    }
    Ok(())
}

impl RangeState {
    fn new(length: u64) -> Self {
        Self { length, position: 0, cache: RangeCache::new(CACHE_BLOCKS), next_fetch_offset: None, batch_blocks: 1 }
    }

    fn missing_range(&self) -> Option<(u64, usize)> {
        if self.position >= self.length || self.cache.contains(self.position) {
            return None;
        }
        let offset = self.position / BLOCK_BYTES as u64 * BLOCK_BYTES as u64;
        let mut blocks = if self.next_fetch_offset == Some(offset) { (self.batch_blocks * 2).min(MAX_RANGE_BYTES / BLOCK_BYTES) } else { 1 };
        for ahead in 1..blocks {
            let distance = (ahead * BLOCK_BYTES) as u64;
            if distance >= self.length - offset {
                break;
            }
            if self.cache.contains(offset + distance) {
                blocks = ahead;
                break;
            }
        }
        Some((offset, (self.length - offset).min((blocks * BLOCK_BYTES) as u64) as usize))
    }

    fn remember(&mut self, offset: u64, length: usize, bytes: Vec<u8>) -> io::Result<()> {
        self.cache.insert_range(offset, length, bytes)?;
        self.next_fetch_offset = Some(offset + length as u64);
        self.batch_blocks = length.div_ceil(BLOCK_BYTES);
        Ok(())
    }

    fn read_cached(&mut self, output: &mut [u8]) -> usize {
        let count = self.cache.read(self.position, output);
        self.position += count as u64;
        count
    }

    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let position = seek_position(self.position, self.length, from)?;
        if position != self.position {
            self.next_fetch_offset = None;
            self.batch_blocks = 1;
        }
        self.position = position;
        Ok(position)
    }
}

struct PendingRange {
    offset: u64,
    length: usize,
    response: BoxFuture<'static, io::Result<Vec<u8>>>,
}

pub(crate) type RangeFetcher = Box<dyn FnMut(u64, usize) -> BoxFuture<'static, io::Result<Vec<u8>>> + Send>;

pub(crate) struct RangeReader {
    state: RangeState,
    fetch: RangeFetcher,
    pending: Option<PendingRange>,
}

impl RangeReader {
    /// Prefix validation belongs to the source constructor. Each cursor owns
    /// its fetch callback, cache, and pending response.
    pub(crate) fn new(length: u64, prefix: Vec<u8>, fetch: RangeFetcher) -> Self {
        let mut state = RangeState::new(length);
        if !prefix.is_empty() {
            state.cache.insert(0, prefix);
        }
        Self { state, fetch, pending: None }
    }
}

impl AsyncRead for RangeReader {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, output: &mut [u8]) -> Poll<io::Result<usize>> {
        let reader = self.get_mut();
        if output.is_empty() || reader.state.position >= reader.state.length {
            return Poll::Ready(Ok(0));
        }
        if reader.pending.is_none() {
            if let Some((offset, length)) = reader.state.missing_range() {
                reader.pending = Some(PendingRange { offset, length, response: (reader.fetch)(offset, length) });
            }
        }
        if let Some(pending) = &mut reader.pending {
            let bytes = match pending.response.as_mut().poll(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(result) => result,
            };
            let pending = reader.pending.take().unwrap();
            reader.state.remember(pending.offset, pending.length, bytes?)?;
        }
        Poll::Ready(Ok(reader.state.read_cached(output)))
    }
}

impl AsyncSeek for RangeReader {
    fn poll_seek(self: Pin<&mut Self>, _: &mut Context<'_>, from: SeekFrom) -> Poll<io::Result<u64>> {
        let reader = self.get_mut();
        reader.state.seek(from)?;
        reader.pending = None;
        Poll::Ready(Ok(reader.state.position))
    }
}
