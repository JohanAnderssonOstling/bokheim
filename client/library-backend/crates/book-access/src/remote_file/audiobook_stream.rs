//! Opt-in recovery for native audiobook range reads. Document readers keep the
//! ordinary single-attempt adapter. A retry never advances the file position.
use super::*;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

#[derive(Default)]
struct ControlState {
    requested_generation: u64,
    reader_generation: u64,
    cancelled: bool,
    paused: bool,
    pending: bool,
    failure: Option<String>,
}

#[derive(Clone, Default)]
pub struct AudioStreamControl(Arc<(Mutex<ControlState>, Condvar)>);

impl AudioStreamControl {
    /// Invalidate the current range without closing this source or changing its
    /// pinned revision. Only the decoder may acknowledge the requested seek.
    pub fn request_seek(&self, generation: u64) {
        let mut state = self.0 .0.lock().unwrap();
        state.requested_generation = state.requested_generation.max(generation);
        drop(state);
        self.0 .1.notify_all();
    }

    pub fn begin_seek(&self, generation: u64) -> bool {
        let mut state = self.0 .0.lock().unwrap();
        if state.cancelled || generation != state.requested_generation {
            return false;
        }
        state.reader_generation = generation;
        state.pending = false;
        true
    }

    pub fn is_interrupted(&self) -> bool {
        let state = self.0 .0.lock().unwrap();
        state.requested_generation != state.reader_generation
    }

    fn check_generation(&self, generation: u64) -> io::Result<()> {
        Self::check_state(&self.0 .0.lock().unwrap(), generation)
    }

    fn check_state(state: &ControlState, generation: u64) -> io::Result<()> {
        // Do not use Interrupted: Read::read_exact retries it internally and
        // would never give the decoder a chance to process Stop or Seek.
        if state.cancelled {
            return Err(io::Error::new(io::ErrorKind::ConnectionAborted, "audiobook stream cancelled"));
        }
        if generation != state.requested_generation {
            return Err(io::Error::new(io::ErrorKind::WouldBlock, "audiobook stream seek requested"));
        }
        Ok(())
    }

    pub fn cancel(&self) {
        self.0 .0.lock().unwrap().cancelled = true;
        self.0 .1.notify_all();
    }

    pub fn set_paused(&self, paused: bool) {
        self.0 .0.lock().unwrap().paused = paused;
        self.0 .1.notify_all();
    }

    pub fn is_cancelled(&self) -> bool {
        self.0 .0.lock().unwrap().cancelled
    }
    pub fn is_paused(&self) -> bool {
        self.0 .0.lock().unwrap().paused
    }
    pub fn is_pending(&self) -> bool {
        self.0 .0.lock().unwrap().pending
    }
    pub fn failure(&self) -> Option<String> {
        self.0 .0.lock().unwrap().failure.clone()
    }

    fn complete(&self, failure: Option<String>) {
        let mut state = self.0 .0.lock().unwrap();
        state.pending = false;
        state.failure = failure;
    }

    fn io_failure(&self, message: &str) -> io::Error {
        self.complete(Some(message.to_owned()));
        io::Error::other(message.to_owned())
    }
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

pub fn native_audio_reader<F>(length: u64, prefix: Vec<u8>, fetch: F) -> Result<(crate::BoxedBookReader, AudioStreamControl), String>
where
    F: FnMut(u64, usize) -> crate::FetchFuture<'static, Result<Vec<u8>, RemoteFileError>> + Send + 'static,
{
    {
        let mut fetch: _ = fetch;
        let policy: RetryPolicy = RETRY;

        validate_prefix(length, &prefix)?;
        let control = AudioStreamControl::default();
        let worker_control = control.clone();
        type Request = (u64, u64, usize, std::sync::mpsc::SyncSender<Result<Vec<u8>, RemoteFileError>>);
        let (send, receive) = std::sync::mpsc::sync_channel::<Request>(1);
        let worker = std::thread::Builder::new()
            .name("audiobook-ranges".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build();
                while let Ok((generation, offset, length, result)) = receive.recv() {
                    let response = match &runtime {
                        Ok(runtime) => runtime.block_on(async {
                            let request = tokio::time::timeout(REQUEST_TIMEOUT, fetch(offset, length));
                            tokio::pin!(request);
                            loop {
                                if worker_control.check_generation(generation).is_err() {
                                    break Err(RemoteFileError::Failed("audiobook stream interrupted".into()));
                                }
                                tokio::select! {
                                    result = &mut request => break result.unwrap_or_else(|_| Err(RemoteFileError::Retryable("remote file read timed out".into()))),
                                    _ = tokio::time::sleep(Duration::from_millis(20)) => {},
                                }
                            }
                        }),
                        Err(error) => Err(RemoteFileError::Failed(error.to_string())),
                    };
                    let _ = result.send(response);
                }
            })
            .map_err(|error| error.to_string())?;
        let reader_control = control.clone();
        let reader = RangeReader::new(
            length,
            Box::new(move |offset, length| {
                let generation = {
                    let mut state = reader_control.0 .0.lock().unwrap();
                    AudioStreamControl::check_state(&state, state.reader_generation)?;
                    state.pending = true;
                    state.reader_generation
                };
                for attempt in 0..policy.attempts {
                    // Pause only gates retries: a normal read can fill the prebuffer
                    // while paused, but a disconnected source must not retry forever.
                    if attempt > 0 {
                        {
                            let delay: Duration = policy.delay(attempt - 1);
                            'reader: {
                                let deadline = Instant::now() + delay;
                                let mut state = reader_control.0 .0.lock().unwrap();
                                loop {
                                    AudioStreamControl::check_state(&state, generation)?;
                                    if state.paused {
                                        state = reader_control.0 .1.wait(state).unwrap();
                                        continue;
                                    }
                                    let remaining = deadline.saturating_duration_since(Instant::now());
                                    if remaining.is_zero() {
                                        break 'reader ();
                                    }
                                    state = reader_control.0 .1.wait_timeout(state, remaining).unwrap().0;
                                }
                            }
                        };
                    }
                    reader_control.check_generation(generation)?;
                    let (reply, result) = std::sync::mpsc::sync_channel(1);
                    send.send((generation, offset, length, reply)).map_err(|_| reader_control.io_failure("audiobook range worker stopped"))?;
                    let response = result.recv().map_err(|_| reader_control.io_failure("audiobook range worker stopped"))?;
                    let response = response.and_then(|bytes| {
                        if bytes.len() == length {
                            Ok(bytes)
                        } else if bytes.len() < length {
                            Err(RemoteFileError::Retryable("incomplete remote file range".into()))
                        } else {
                            Err(RemoteFileError::Failed("oversized remote file range".into()))
                        }
                    });
                    reader_control.check_generation(generation)?;
                    match response {
                        Ok(bytes) => {
                            reader_control.complete(None);
                            return Ok(bytes);
                        }
                        Err(RemoteFileError::Retryable(_)) if attempt + 1 < policy.attempts => {}
                        Err(error) => {
                            let message = error.to_string();
                            reader_control.complete(Some(message.clone()));
                            return Err(io::Error::other(message));
                        }
                    }
                }
                unreachable!("retry policy always allows an initial attempt")
            }),
        )
        .with_prefix(prefix);
        Ok((Box::new(AudioRangeReader { reader: Some(reader), control: control.clone(), worker: Some(worker) }), control))
    }
}

struct AudioRangeReader {
    reader: Option<RangeReader>,
    control: AudioStreamControl,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Read for AudioRangeReader {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        {
            let state = self.control.0 .0.lock().unwrap();
            AudioStreamControl::check_state(&state, state.reader_generation)?;
        }
        if let Some(error) = self.control.failure() {
            return Err(io::Error::other(error));
        }
        self.reader.as_mut().unwrap().read(output)
    }
}
impl Seek for AudioRangeReader {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.reader.as_mut().unwrap().seek(position)
    }
}
impl Drop for AudioRangeReader {
    fn drop(&mut self) {
        self.control.cancel();
        self.reader.take(); // Close the request sender before joining its worker.
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    const FAST: RetryPolicy = RetryPolicy { attempts: 3, initial_delay: Duration::ZERO, max_delay: Duration::ZERO };

    #[test]
    fn retry_backoff_is_capped() {
        assert_eq!(RETRY.delay(0), Duration::from_millis(500));
        assert_eq!(RETRY.delay(1), Duration::from_secs(1));
        assert_eq!(RETRY.delay(20), Duration::from_secs(8));
    }
}
