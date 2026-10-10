//! Native parser bridge borrowing the application’s I/O runtime.

use super::{RemoteFileError, REQUEST_TIMEOUT};
use crate::range_reader::{validate_prefix, RangeFetcher, RangeReader};
use crate::{BoxedBookReader, FetchFuture};
use std::{
    io::{self, Read, Seek, SeekFrom},
    time::Duration,
};
use sync_wrapper::SyncWrapper;
use tokio::runtime::Handle;
use tokio_util::{
    compat::{Compat, FuturesAsyncReadCompatExt},
    io::SyncIoBridge,
};

type AsyncFetch = Box<dyn FnMut(u64, usize) -> FetchFuture<'static, Result<Vec<u8>, RemoteFileError>> + Send>;

struct NativeReader {
    // The synchronous parser has exclusive access when reading or seeking.
    // SyncWrapper satisfies BoxedBookReader's Sync bound without a lock.
    bridge: SyncWrapper<SyncIoBridge<Compat<RangeReader>>>,
}

impl Read for NativeReader {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        self.bridge.get_mut().read(output)
    }
}

impl Seek for NativeReader {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        self.bridge.get_mut().seek(from)
    }
}

/// Construct on the application's runtime; perform synchronous reads on a
/// parser thread. The application must keep its runtime alive while reading.
pub fn native_reader<F>(length: u64, prefix: Vec<u8>, fetch: F) -> io::Result<BoxedBookReader>
where
    F: FnMut(u64, usize) -> FetchFuture<'static, Result<Vec<u8>, RemoteFileError>> + Send + 'static,
{
    open(length, prefix, Box::new(fetch), REQUEST_TIMEOUT)
}

fn open(length: u64, prefix: Vec<u8>, mut fetch: AsyncFetch, timeout: Duration) -> io::Result<BoxedBookReader> {
    validate_prefix(length, &prefix)?;
    let handle = Handle::try_current().map_err(io::Error::other)?;
    let fetch: RangeFetcher = Box::new(move |offset, length| {
        let response = fetch(offset, length);
        Box::pin(async move { tokio::time::timeout(timeout, response).await.map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "remote file read timed out"))?.map_err(io::Error::other) })
    });
    let reader = RangeReader::new(length, prefix, fetch);
    Ok(blocking_reader(reader, handle))
}

pub(super) fn blocking_reader(reader: RangeReader, handle: Handle) -> BoxedBookReader {
    let bridge = SyncIoBridge::new_with_handle(reader.compat(), handle);
    Box::new(NativeReader { bridge: SyncWrapper::new(bridge) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::range_reader::BLOCK_BYTES;
    use std::sync::{Arc, Mutex};

    #[test]
    fn seeks_reuse_cached_blocks_and_read_ahead_grows() {
        let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
        let _context = runtime.enter();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = requests.clone();
        let length = 8 * BLOCK_BYTES as u64 + 3;
        let mut reader = native_reader(length, vec![0; BLOCK_BYTES], move |offset, length| {
            recorded.lock().unwrap().push((offset, length));
            Box::pin(async move {
                tokio::time::sleep(Duration::from_millis(1)).await;
                Ok((offset..offset + length as u64).map(|n| (n / BLOCK_BYTES as u64) as u8).collect())
            })
        })
        .unwrap();
        let mut bytes = vec![0; 4 * BLOCK_BYTES];
        reader.read_exact(&mut bytes).unwrap();
        assert_eq!(*requests.lock().unwrap(), vec![(BLOCK_BYTES as u64, BLOCK_BYTES), (2 * BLOCK_BYTES as u64, 2 * BLOCK_BYTES)]);
        assert!(bytes.chunks(BLOCK_BYTES).enumerate().all(|(i, block)| block.iter().all(|&b| b == i as u8)));
        reader.seek(SeekFrom::Start(BLOCK_BYTES as u64 + 7)).unwrap();
        reader.read_exact(&mut bytes[..3]).unwrap();
        assert_eq!(&bytes[..3], &[1; 3]);
        assert_eq!(requests.lock().unwrap().len(), 2);
        reader.seek(SeekFrom::End(-3)).unwrap();
        reader.read_exact(&mut bytes[..3]).unwrap();
        assert_eq!(&bytes[..3], &[8; 3]);
        assert_eq!(reader.read(&mut bytes[..1]).unwrap(), 0);
        assert!(reader.seek(SeekFrom::Start(0)).is_ok());
        assert_eq!(reader.seek(SeekFrom::Current(-1)).unwrap_err().kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn incomplete_response_does_not_advance_position_or_poison_cache() {
        let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
        let _context = runtime.enter();
        let mut calls = 0;
        let mut reader = native_reader(2 * BLOCK_BYTES as u64, vec![0; BLOCK_BYTES], move |_, length| {
            calls += 1;
            let count = if calls == 1 { length - 1 } else { length };
            Box::pin(async move { Ok(vec![9; count]) })
        })
        .unwrap();
        reader.seek(SeekFrom::Start(BLOCK_BYTES as u64)).unwrap();
        assert_eq!(reader.read(&mut [0]).unwrap_err().kind(), io::ErrorKind::UnexpectedEof);
        assert_eq!(reader.stream_position().unwrap(), BLOCK_BYTES as u64);
        let mut bytes = [0];
        reader.read_exact(&mut bytes).unwrap();
        assert_eq!(bytes, [9]);
    }

    #[test]
    fn stalled_fetch_times_out_and_can_be_retried() {
        let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
        let _context = runtime.enter();
        let mut calls = 0;
        let mut reader = open(
            2 * BLOCK_BYTES as u64,
            vec![0; BLOCK_BYTES],
            Box::new(move |_, length| {
                calls += 1;
                let stall = calls == 1;
                Box::pin(async move {
                    if stall {
                        std::future::pending::<()>().await;
                    }
                    Ok(vec![5; length])
                })
            }),
            Duration::from_millis(10),
        )
        .unwrap();
        reader.seek(SeekFrom::Start(BLOCK_BYTES as u64)).unwrap();
        assert_eq!(reader.read(&mut [0]).unwrap_err().kind(), io::ErrorKind::TimedOut);
        let mut bytes = [0];
        reader.read_exact(&mut bytes).unwrap();
        assert_eq!(bytes, [5]);
    }

    #[tokio::test]
    async fn construction_and_drop_are_safe_inside_async_context() {
        let reader = native_reader(BLOCK_BYTES as u64, vec![0; BLOCK_BYTES], |_, _| Box::pin(async { panic!("prefix should be cached") })).unwrap();
        drop(reader);
    }
}
