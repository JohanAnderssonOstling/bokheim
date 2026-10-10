//! One async worker owns audiobook fetches and retry state. Decoder reads use
//! the shared range reader; control requests wake the worker through a channel.

use super::{native::blocking_reader, RemoteFileError, REQUEST_TIMEOUT};
use crate::range_reader::{validate_prefix, RangeFetcher, RangeReader};
use crate::{BoxedBookReader, FetchFuture};
use futures_channel::oneshot;
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use std::time::Duration;
use tokio::runtime::Handle;
use tokio::sync::{mpsc, watch};
use tokio::time::{sleep_until, Instant};

#[derive(Clone, Copy, Default)]
struct ControlRequest {
    generation: u64,
    cancelled: bool,
    paused: bool,
}

/// Commands for the byte worker and an observation of its terminal failure.
/// Only the decoder acknowledges a seek; its generation is separate from the
/// requested generation so an in-flight read cannot consume obsolete bytes.
#[derive(Clone)]
pub struct AudioStreamControl {
    requests: watch::Sender<ControlRequest>,
    reader_generation: Arc<AtomicU64>,
    failure: watch::Receiver<Option<String>>,
}

/// A control with no byte worker attached: requests are still recorded, and
/// no failure is ever reported.
impl Default for AudioStreamControl {
    fn default() -> Self {
        Self { requests: watch::channel(ControlRequest::default()).0, reader_generation: Arc::new(AtomicU64::new(0)), failure: watch::channel(None).1 }
    }
}

impl AudioStreamControl {
    pub fn request_seek(&self, generation: u64) {
        self.requests.send_modify(|request| request.generation = request.generation.max(generation));
    }

    pub fn begin_seek(&self, generation: u64) -> bool {
        if check_request(*self.requests.borrow(), generation).is_err() {
            return false;
        }
        self.reader_generation.store(generation, Ordering::Release);
        check_request(*self.requests.borrow(), generation).is_ok()
    }

    pub fn is_interrupted(&self) -> bool {
        self.requests.borrow().generation != self.reader_generation.load(Ordering::Acquire)
    }
    pub fn cancel(&self) {
        self.requests.send_modify(|request| request.cancelled = true);
    }
    pub fn set_paused(&self, paused: bool) {
        self.requests.send_modify(|request| request.paused = paused);
    }
    pub fn is_cancelled(&self) -> bool {
        self.requests.borrow().cancelled
    }
    pub fn is_paused(&self) -> bool {
        self.requests.borrow().paused
    }
    pub fn failure(&self) -> Option<String> {
        self.failure.borrow().clone()
    }

    fn check(&self, generation: u64) -> io::Result<()> {
        check_request(*self.requests.borrow(), generation)
    }
}

fn check_request(request: ControlRequest, generation: u64) -> io::Result<()> {
    // read_exact automatically retries Interrupted, preventing decoder commands
    // from being processed. WouldBlock returns control to the decoder instead.
    if request.cancelled {
        return Err(io::Error::new(io::ErrorKind::ConnectionAborted, "audiobook stream cancelled"));
    }
    if request.generation != generation {
        return Err(io::Error::new(io::ErrorKind::WouldBlock, "audiobook stream seek requested"));
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct RetryPolicy {
    attempts: usize,
    initial_delay: Duration,
    max_delay: Duration,
}
impl RetryPolicy {
    fn delay(self, retry: usize) -> Duration {
        self.initial_delay.saturating_mul(1u32.checked_shl(retry.min(31) as u32).unwrap_or(u32::MAX)).min(self.max_delay)
    }
}
const RETRY: RetryPolicy = RetryPolicy { attempts: 8, initial_delay: Duration::from_millis(500), max_delay: Duration::from_secs(8) };

struct Request {
    generation: u64,
    offset: u64,
    length: usize,
    reply: oneshot::Sender<io::Result<Vec<u8>>>,
}

/// Gate retry attempts while paused, and interrupt immediately on seek/stop.
async fn wait_retry(control: &mut watch::Receiver<ControlRequest>, generation: u64, delay: Duration) -> io::Result<()> {
    let deadline = Instant::now() + delay;
    loop {
        let request = *control.borrow_and_update();
        check_request(request, generation)?;
        if request.paused {
            control.changed().await.map_err(|_| io::Error::other("audiobook control closed"))?;
        } else if Instant::now() >= deadline {
            return Ok(());
        } else {
            tokio::select! {
                _ = sleep_until(deadline) => {},
                changed = control.changed() => changed.map_err(|_| io::Error::other("audiobook control closed"))?,
            }
        }
    }
}

async fn fetch_with_retry<F>(fetch: &mut F, request: &Request, control: &mut watch::Receiver<ControlRequest>) -> io::Result<Vec<u8>>
where
    F: FnMut(u64, usize) -> FetchFuture<'static, Result<Vec<u8>, RemoteFileError>>,
{
    for attempt in 0..RETRY.attempts {
        if attempt > 0 {
            wait_retry(control, request.generation, RETRY.delay(attempt - 1)).await?;
        }
        check_request(*control.borrow_and_update(), request.generation)?;
        let response = tokio::time::timeout(REQUEST_TIMEOUT, fetch(request.offset, request.length));
        tokio::pin!(response);
        let response = loop {
            tokio::select! {
                result = &mut response => break result.unwrap_or_else(|_| Err(RemoteFileError::Retryable("remote file read timed out".into()))),
                changed = control.changed() => {
                    changed.map_err(|_| io::Error::other("audiobook control closed"))?;
                    check_request(*control.borrow_and_update(), request.generation)?;
                }
            }
        };
        check_request(*control.borrow_and_update(), request.generation)?;
        let response = response.and_then(|bytes| {
            if bytes.len() == request.length {
                Ok(bytes)
            } else if bytes.len() < request.length {
                Err(RemoteFileError::Retryable("incomplete remote file range".into()))
            } else {
                Err(RemoteFileError::Failed("oversized remote file range".into()))
            }
        });
        match response {
            Ok(bytes) => return Ok(bytes),
            Err(RemoteFileError::Retryable(_)) if attempt + 1 < RETRY.attempts => {}
            Err(error) => return Err(io::Error::other(error)),
        }
    }
    unreachable!("retry policy always allows an initial attempt")
}

/// Construct on the application’s runtime, which owns the fetch worker.
/// Decoder reads run on a blocking thread while that runtime remains alive.
pub fn native_audio_reader<F>(length: u64, prefix: Vec<u8>, mut fetch: F) -> io::Result<(BoxedBookReader, AudioStreamControl)>
where
    F: FnMut(u64, usize) -> FetchFuture<'static, Result<Vec<u8>, RemoteFileError>> + Send + 'static,
{
    validate_prefix(length, &prefix)?;
    let handle = Handle::try_current().map_err(io::Error::other)?;
    let (requests, mut control_updates) = watch::channel(ControlRequest::default());
    let (failure_sender, failure) = watch::channel(None);
    let control = AudioStreamControl { requests, reader_generation: Arc::new(AtomicU64::new(0)), failure };
    let (sender, mut receiver) = mpsc::unbounded_channel::<Request>();
    handle.spawn(async move {
        while let Some(request) = receiver.recv().await {
            if request.reply.is_canceled() {
                continue;
            }
            let result = fetch_with_retry(&mut fetch, &request, &mut control_updates).await;
            if let Err(error) = &result {
                if !matches!(error.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::ConnectionAborted) {
                    failure_sender.send_replace(Some(error.to_string()));
                }
            }
            let _ = request.reply.send(result);
        }
    });
    let reader_control = control.clone();
    let fetch: RangeFetcher = Box::new(move |offset, length| {
        let generation = reader_control.reader_generation.load(Ordering::Acquire);
        let check = reader_control.check(generation);
        let (reply, response) = oneshot::channel();
        let sent = if check.is_ok() {
            sender.send(Request { generation, offset, length, reply })
        } else {
            drop(reply);
            Ok(())
        };
        let control = reader_control.clone();
        Box::pin(async move {
            check?;
            sent.map_err(|_| io::Error::other("audiobook range worker stopped"))?;
            let result = response.await.map_err(|_| io::Error::other("audiobook range worker stopped"))?;
            control.check(generation)?;
            result
        })
    });
    let reader = blocking_reader(RangeReader::new(length, prefix, fetch), handle);
    Ok((Box::new(AudioRangeReader { reader, control: control.clone() }), control))
}

struct AudioRangeReader {
    reader: BoxedBookReader,
    control: AudioStreamControl,
}
impl Read for AudioRangeReader {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        self.control.check(self.control.reader_generation.load(Ordering::Acquire))?;
        if let Some(error) = self.control.failure() {
            return Err(io::Error::other(error));
        }
        self.reader.read(output)
    }
}
impl Seek for AudioRangeReader {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.reader.seek(position)
    }
}
impl Drop for AudioRangeReader {
    fn drop(&mut self) {
        self.control.cancel();
    }
}
