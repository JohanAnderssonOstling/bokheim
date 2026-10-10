//! Independent async cursors over one immutable book revision.

use std::io;
use std::sync::Arc;

use futures_channel::oneshot;
use futures_util::future::{select, BoxFuture, Either};
use futures_util::io::{AsyncBufRead, AsyncSeek, BufReader};

use crate::range_reader::{validate_prefix, RangeReader};
use crate::RemoteFileError;

pub trait AsyncBookReader: AsyncBufRead + AsyncSeek + Unpin + Send {}
impl<T: AsyncBufRead + AsyncSeek + Unpin + Send> AsyncBookReader for T {}
pub type BoxedAsyncBookReader = Box<dyn AsyncBookReader>;
type RangeFetch = Arc<dyn Fn(u64, usize) -> BoxFuture<'static, io::Result<Vec<u8>>> + Send + Sync>;

/// A cheap, cloneable source. Each reader has its own position, read-ahead, and
/// bounded cache. Fetches must refer to the same revision for the source lifetime.
#[derive(Clone)]
pub struct AsyncBookSource {
    length: u64,
    prefix: Arc<Vec<u8>>,
    fetch: RangeFetch,
}

impl AsyncBookSource {
    pub fn from_ranges<F>(length: u64, prefix: Vec<u8>, fetch: F) -> io::Result<Self>
    where
        F: Fn(u64, usize) -> BoxFuture<'static, io::Result<Vec<u8>>> + Send + Sync + 'static,
    {
        if !prefix.is_empty() {
            validate_prefix(length, &prefix)?;
        }
        Ok(Self { length, prefix: Arc::new(prefix), fetch: Arc::new(fetch) })
    }

    pub fn length(&self) -> u64 {
        self.length
    }

    pub fn reader(&self) -> BoxedAsyncBookReader {
        let fetch = self.fetch.clone();
        Box::new(BufReader::new(RangeReader::new(self.length, self.prefix.as_ref().clone(), Box::new(move |offset, length| fetch(offset, length)))))
    }
}

pub(crate) async fn send_response<E>(response: crate::FetchFuture<'static, Result<Vec<u8>, E>>, mut reply: oneshot::Sender<io::Result<Vec<u8>>>)
where
    E: Into<RemoteFileError> + 'static,
{
    if reply.is_canceled() {
        return;
    }
    let response = Box::pin(crate::time::timeout(crate::remote_file::REQUEST_TIMEOUT, response));
    let result = match select(response, reply.cancellation()).await {
        Either::Left((result, _)) => result.map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "remote file read timed out")).and_then(|result| result.map_err(|error| io::Error::other(error.into()))),
        Either::Right(_) => return,
    };
    let _ = reply.send(result);
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::*;
    use crate::FetchFuture;
    use std::fs::File;
    #[cfg(unix)]
    use std::os::unix::fs::FileExt;
    #[cfg(windows)]
    use std::os::windows::fs::FileExt;
    use tokio::runtime::Handle;

    impl AsyncBookSource {
        /// Reuse the caller's I/O runtime. Polling the resulting reader does not
        /// require a Tokio context and never creates a runtime of its own.
        pub fn remote<F>(length: u64, prefix: Vec<u8>, fetch: F) -> io::Result<Self>
        where
            F: Fn(u64, usize) -> FetchFuture<'static, Result<Vec<u8>, RemoteFileError>> + Send + Sync + 'static,
        {
            let handle = Handle::try_current().map_err(io::Error::other)?;
            let fetch = Arc::new(fetch);
            Self::from_ranges(length, prefix, move |offset, length| {
                let fetch = fetch.clone();
                let (reply, response) = oneshot::channel();
                handle.spawn(async move {
                    send_response(fetch(offset, length), reply).await;
                });
                Box::pin(async move { response.await.map_err(|_| io::Error::other("book range request canceled"))? })
            })
        }

        /// Positional reads on a pinned file handle, using the existing blocking
        /// pool. Independent ZIP cursors never share a mutable file position.
        pub fn from_file(file: File) -> io::Result<Self> {
            let length = file.metadata()?.len();
            let file = Arc::new(file);
            let handle = Handle::try_current().map_err(io::Error::other)?;
            Self::from_ranges(length, Vec::new(), move |offset, length| {
                let file = file.clone();
                let task = handle.spawn_blocking(move || {
                    let mut bytes = vec![0; length];
                    let mut read = 0;
                    while read < length {
                        #[cfg(unix)]
                        let result = file.read_at(&mut bytes[read..], offset + read as u64);
                        #[cfg(windows)]
                        let result = file.seek_read(&mut bytes[read..], offset + read as u64);
                        match result {
                            Ok(0) => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "book file was truncated")),
                            Ok(count) => read += count,
                            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                            Err(error) => return Err(error),
                        }
                    }
                    Ok(bytes)
                });
                Box::pin(async move { task.await.map_err(io::Error::other)? })
            })
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod browser {
    use super::*;
    use crate::FetchFuture;

    struct Request {
        offset: u64,
        length: usize,
        reply: oneshot::Sender<io::Result<Vec<u8>>>,
    }

    impl AsyncBookSource {
        /// Keep JS futures on the owning event loop. Readers on background
        /// workers await channel replies; they never block or poll a JS future.
        pub fn browser<F>(length: u64, prefix: Vec<u8>, mut fetch: F) -> io::Result<Self>
        where
            F: FnMut(u64, usize) -> FetchFuture<'static, Result<Vec<u8>, String>> + 'static,
        {
            let (sender, receiver) = async_channel::unbounded::<Request>();
            let source = Self::from_ranges(length, prefix, move |offset, length| {
                let (reply, response) = oneshot::channel();
                let result = sender.try_send(Request { offset, length, reply });
                Box::pin(async move {
                    result.map_err(|_| io::Error::other("book source service stopped"))?;
                    response.await.map_err(|_| io::Error::other("book range request canceled"))?
                })
            })?;
            wasm_bindgen_futures::spawn_local(async move {
                while let Ok(request) = receiver.recv().await {
                    if request.reply.is_canceled() {
                        continue;
                    }
                    let response = fetch(request.offset, request.length);
                    wasm_bindgen_futures::spawn_local(send_response(response, request.reply));
                }
            });
            Ok(source)
        }
    }
}
