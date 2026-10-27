//! Async batch dispatch owned by the platform's existing I/O executor.

use std::{io, sync::Arc};

use futures_channel::oneshot;
use futures_util::future::BoxFuture;

use crate::{async_source::send_response, FetchFuture, RemoteFileError};

pub(crate) type BatchFetch = Arc<dyn Fn(Vec<(u64, usize)>) -> BoxFuture<'static, io::Result<Vec<u8>>> + Send + Sync>;

#[cfg(feature = "pdf")]
pub(crate) async fn cancellable(fetch: &BatchFetch, ranges: Vec<(u64, usize)>, cancellation: &pdf_view_common::PdfPrefetchCancellation) -> io::Result<Vec<u8>> {
    use futures_util::future::{select, Either};

    if cancellation.is_cancelled() {
        return Err(io::Error::new(io::ErrorKind::Interrupted, "PDF prefetch canceled"));
    }
    let response = fetch(ranges);
    match select(Box::pin(cancellation.cancelled()), response).await {
        Either::Left(_) => Err(io::Error::new(io::ErrorKind::Interrupted, "PDF prefetch canceled")),
        Either::Right((result, _)) => result,
    }
    // Dropping the response closes its oneshot receiver. send_response then
    // drops the HTTP future, including its response body or browser reply port.
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn native<F>(fetch: F) -> io::Result<BatchFetch>
where
    F: Fn(Vec<(u64, usize)>) -> FetchFuture<'static, Result<Vec<u8>, RemoteFileError>> + Send + Sync + 'static,
{
    let handle = tokio::runtime::Handle::try_current().map_err(io::Error::other)?;
    let fetch = Arc::new(fetch);
    Ok(Arc::new(move |ranges| {
        let fetch = fetch.clone();
        let (reply, response) = oneshot::channel();
        handle.spawn(async move { send_response(fetch(ranges), reply).await });
        Box::pin(async move { response.await.map_err(|_| io::Error::other("book range batch canceled"))? })
    }))
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn browser<F>(mut fetch: F) -> BatchFetch
where
    F: FnMut(Vec<(u64, usize)>) -> FetchFuture<'static, Result<Vec<u8>, String>> + 'static,
{
    struct Request {
        ranges: Vec<(u64, usize)>,
        reply: oneshot::Sender<io::Result<Vec<u8>>>,
    }
    let (sender, receiver) = async_channel::unbounded::<Request>();
    wasm_bindgen_futures::spawn_local(async move {
        while let Ok(request) = receiver.recv().await {
            if !request.reply.is_canceled() {
                let response = fetch(request.ranges);
                wasm_bindgen_futures::spawn_local(send_response(response, request.reply));
            }
        }
    });
    Arc::new(move |ranges| {
        let (reply, response) = oneshot::channel();
        let sent = sender.try_send(Request { ranges, reply });
        Box::pin(async move {
            sent.map_err(|_| io::Error::other("book batch service stopped"))?;
            response.await.map_err(|_| io::Error::other("book range batch canceled"))?
        })
    })
}
