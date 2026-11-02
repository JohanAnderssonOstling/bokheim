//! Bounded cached PDF demand reads with adaptive sequential read-ahead.
use crate::BoxedBookReader;
use crate::range_cache::{BLOCK_BYTES, MAX_BUNDLE_BYTES, RangeCache, seek_position};
use crate::range_reader::validate_prefix;
use std::{
    io::{self, Read, Seek, SeekFrom},
    sync::{Arc, Mutex},
};
const BLOCK: u64 = BLOCK_BYTES as u64;
const CAPACITY: usize = 512; // 32 MiB of raw PDF bytes.
pub use crate::BundleFetch as Fetch;
struct State {
    cache: RangeCache,
    fetch: Fetch,
}
struct Source {
    length: u64,
    state: Mutex<State>,
}
struct Reader {
    source: Arc<Source>,
    position: u64,
    next_fetch_offset: Option<u64>,
    fetch_bytes: usize,
}

pub fn open(length: u64, prefix: Vec<u8>, fetch: Fetch) -> io::Result<BoxedBookReader> {
    validate_prefix(length, &prefix)?;
    let mut cache = RangeCache::new(CAPACITY);
    cache.insert(0, prefix);
    let source = Arc::new(Source { length, state: Mutex::new(State { cache, fetch }) });
    Ok(Box::new(Reader { source, position: 0, next_fetch_offset: None, fetch_bytes: BLOCK_BYTES }))
}
impl Read for Reader {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if bytes.is_empty() || self.position >= self.source.length {
            return Ok(0);
        }
        let mut state = self.source.state.lock().map_err(|_| io::Error::other("PDF byte cache poisoned"))?;
        if !state.cache.contains(self.position) {
            let offset = self.position / BLOCK * BLOCK;
            let needed = bytes.len().saturating_add((self.position - offset) as usize).min(MAX_BUNDLE_BYTES);
            let adaptive = if self.next_fetch_offset == Some(offset) { (self.fetch_bytes * 2).min(MAX_BUNDLE_BYTES) } else { BLOCK_BYTES };
            let blocks = adaptive.max(needed).div_ceil(BLOCK_BYTES);
            let mut length = (self.source.length - offset).min((blocks * BLOCK_BYTES) as u64) as usize;
            // A cached interval ends speculative read-ahead. Reuse those
            // blocks instead of downloading them again in the larger request.
            for ahead in 1..blocks {
                if state.cache.contains(offset + (ahead * BLOCK_BYTES) as u64) {
                    length = length.min(ahead * BLOCK_BYTES);
                    break;
                }
            }
            let data = state.fetch.fetch(vec![(offset, length)])?;
            state.cache.insert_range(offset, length, data)?;
            self.next_fetch_offset = Some(offset + length as u64);
            self.fetch_bytes = length;
        }
        let count = state.cache.read(self.position, bytes);
        self.position += count as u64;
        Ok(count)
    }
}
impl Seek for Reader {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let position = seek_position(self.position, self.source.length, from)?;
        if position != self.position {
            self.next_fetch_offset = None;
            self.fetch_bytes = BLOCK_BYTES;
        }
        self.position = position;
        Ok(position)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sequential_demand_reads_grow_to_four_mib() {
        let length = 16 * 1024 * 1024 + 13;
        {
            let calls = Arc::new(Mutex::new(Vec::new()));
            let recorded = calls.clone();
            let fetch: Fetch = Box::new(move |ranges: Vec<(u64, usize)>| {
                recorded.lock().unwrap().push(ranges.clone());
                Ok(ranges.into_iter().flat_map(|(offset, count)| (offset..offset + count as u64).map(|position| (position % 251) as u8)).collect())
            });
            let prefix = (0..BLOCK_BYTES).map(|position| (position % 251) as u8).collect();
            let mut reader = open(length, prefix, fetch).unwrap();
            reader.seek(SeekFrom::Start(BLOCK)).unwrap();
            let mut position = BLOCK;
            let mut buffer = vec![0; BLOCK_BYTES];
            while position < length {
                let count = (length - position).min(BLOCK) as usize;
                reader.read_exact(&mut buffer[..count]).unwrap();
                assert!(buffer[..count].iter().enumerate().all(|(index, byte)| *byte == ((position + index as u64) % 251) as u8));
                position += count as u64;
                assert_eq!(reader.stream_position().unwrap(), position, "no-op seeks must preserve sequential read-ahead");
            }
            assert_eq!(reader.read(&mut buffer).unwrap(), 0);
            let lengths = calls.lock().unwrap().iter().flatten().map(|(_, count)| *count).collect::<Vec<_>>();
            assert_eq!(lengths, vec![65536, 131072, 262144, 524288, 1048576, 2097152, 4194304, 4194304, 4194304, 13]);
            reader.seek(SeekFrom::Start(BLOCK)).unwrap();
            reader.read_exact(&mut buffer).unwrap();
            assert_eq!(calls.lock().unwrap().len(), 10, "rereading cached bytes must not fetch again");
        }
    }

    #[test]
    fn failed_expanded_demand_read_preserves_position_cache_and_retry_size() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let recorded = calls.clone();
        let mut reader = open(
            8 * BLOCK,
            vec![0; BLOCK_BYTES],
            Box::new(move |ranges: Vec<(u64, usize)>| {
                let mut calls = recorded.lock().unwrap();
                calls.push(ranges.clone());
                let count = ranges[0].1;
                if calls.len() == 2 { Ok(vec![9; count - 1]) } else { Ok(vec![7; count]) }
            }),
        )
        .unwrap();
        reader.seek(SeekFrom::Start(BLOCK)).unwrap();
        reader.read_exact(&mut vec![0; BLOCK_BYTES]).unwrap();
        assert_eq!(reader.read(&mut [0]).unwrap_err().kind(), io::ErrorKind::UnexpectedEof);
        assert_eq!(reader.stream_position().unwrap(), 2 * BLOCK);
        let mut byte = [0];
        reader.read_exact(&mut byte).unwrap();
        assert_eq!(byte, [7], "an incomplete response must not poison the cache");
        assert_eq!(*calls.lock().unwrap(), vec![vec![(BLOCK, BLOCK_BYTES)], vec![(2 * BLOCK, 2 * BLOCK_BYTES)], vec![(2 * BLOCK, 2 * BLOCK_BYTES)]]);
    }
}
