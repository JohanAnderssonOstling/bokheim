//! A seekable file bridge from blocking parser workers to an existing async
//! event loop. Only Rust-owned bytes and atomics cross threads; JS handles and
//! authentication remain on the event loop that created the file.
use super::*;
use std::sync::Arc;
use client_platform_web::signal::Signal;

struct Request {
    ranges: Vec<(u64, usize)>,
    reply: crossbeam_channel::Sender<Result<Vec<u8>, String>>,
    ready: Arc<Signal>,
}
struct FileRequests {
    sender: Option<crossbeam_channel::Sender<Request>>,
    ready: Arc<Signal>,
}
impl Drop for FileRequests {
    fn drop(&mut self) {
        self.sender.take();
        self.ready.notify();
    }
}

pub fn browser_reader<F>(length: u64, initial_bytes: Option<Vec<u8>>, mut fetch: F) -> Result<crate::BoxedBookReader, String>
where
    F: FnMut(u64, usize) -> crate::FetchFuture<'static, Result<Vec<u8>, String>> + 'static,
{
    if let Some(bytes) = &initial_bytes {
        validate_prefix(length, bytes)?;
    }
    let mut fetch = browser_bundle_fetch(move |ranges| {
        let (offset, length) = ranges[0];
        fetch(offset, length)
    })?;
    Ok(Box::new(RangeReader::new(length, Box::new(move |offset, length| fetch(vec![(offset, length)]))).with_prefix(initial_bytes.unwrap_or_default())))
}

pub fn browser_bundle_fetch<F>(mut fetch: F) -> Result<crate::BundleFetch, String>
where
    F: FnMut(Vec<(u64, usize)>) -> crate::FetchFuture<'static, Result<Vec<u8>, String>> + 'static,
{
    Signal::check()?;
    let (sender, receiver) = crossbeam_channel::bounded::<Request>(1);
    let ready = Arc::new(Signal::default());
    let requests = FileRequests { sender: Some(sender), ready: ready.clone() };
    // This task belongs to the existing event loop, before any parser begins
    // waiting. No nested worker creation or cross-thread JS future wakeups.
    wasm_bindgen_futures::spawn_local(async move {
        loop {
            let observed = ready.observed();
            match receiver.try_recv() {
                Ok(request) => {
                    let result = crate::time::timeout(REQUEST_TIMEOUT, fetch(request.ranges)).await.map_err(|_| "remote file read timed out".to_owned()).and_then(|result| result);
                    let _ = request.reply.try_send(result);
                    request.ready.notify();
                }
                Err(crossbeam_channel::TryRecvError::Disconnected) => break,
                Err(crossbeam_channel::TryRecvError::Empty) => ready.wait_async(observed, std::time::Duration::from_secs(35)).await,
            }
        }
    });
    Ok(Box::new(move |ranges| {
        let ready = Arc::new(Signal::default());
        let (reply, response) = crossbeam_channel::bounded(1);
        requests.sender.as_ref().unwrap().try_send(Request { ranges, reply, ready: ready.clone() }).map_err(io::Error::other)?;
        requests.ready.notify();
        loop {
            let observed = ready.observed();
            match response.try_recv() {
                Ok(result) => return result.map_err(io::Error::other),
                Err(crossbeam_channel::TryRecvError::Disconnected) => return Err(io::Error::other("remote file service stopped")),
                Err(crossbeam_channel::TryRecvError::Empty) => {}
            }
            let status = ready.wait(observed, std::time::Duration::from_secs(35)).map_err(io::Error::other)?;
            if status == "timed-out" {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "remote file read timed out"));
            }
        }
    }))
}
