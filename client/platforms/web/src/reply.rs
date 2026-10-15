//! Callback channels carry replies; their receivers belong to request owners.
use async_channel::{Receiver, RecvError, Sender};

/// Close after the first reply, including when it has already been received.
pub fn send_once<T>(reply: &Sender<T>, value: T) {
    let _ = reply.try_send(value);
    reply.close();
}

/// Retains the reply receiver and cleanup resource even before the request's
/// first poll. Only an unfinished reply invokes cancellation on Drop.
pub struct OwnedReply<T, F: FnOnce()> {
    response: Receiver<T>,
    cancel: Option<F>,
}
impl<T, F: FnOnce()> OwnedReply<T, F> {
    pub fn new(response: Receiver<T>, cancel: F) -> Self {
        Self { response, cancel: Some(cancel) }
    }
    pub async fn receive(&self) -> Result<T, RecvError> {
        self.response.recv().await
    }
}
impl<T, F: FnOnce()> Drop for OwnedReply<T, F> {
    fn drop(&mut self) {
        if !self.response.is_closed() {
            if let Some(cancel) = self.cancel.take() {
                cancel();
            }
        }
    }
}

/// Stale replies cannot settle a newer request. Closing the channel stops an
/// active cycle even when an earlier success is still buffered.
pub async fn matching<T>(replies: &Receiver<(u32, T)>, id: u32) -> Option<T> {
    while !replies.is_closed() {
        let (reply_id, value) = replies.recv().await.ok()?;
        if replies.is_closed() {
            return None;
        }
        if reply_id == id {
            return Some(value);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_channel::{bounded, unbounded};
    use futures_util::future::pending;
    use std::cell::Cell;

    #[test]
    fn first_reply_wins_even_after_it_has_been_received() {
        let (reply, response) = bounded(1);
        let malformed = reply.clone();
        send_once(&reply, Ok::<_, &str>(10));
        assert_eq!(response.try_recv().unwrap(), Ok(10));
        send_once(&malformed, Err("late error"));
        assert!(response.try_recv().is_err());
        assert!(response.is_closed());
    }

    #[tokio::test]
    async fn cancellation_closes_an_active_operation_without_a_payload() {
        let (cancel, cancelled) = bounded::<()>(1);
        let mut wait = Box::pin(cancelled.recv());
        assert!(futures_util::poll!(&mut wait).is_pending());
        cancel.close();
        assert!(wait.await.is_err());
        cancel.close();
    }

    #[tokio::test]
    async fn matching_ignores_old_ids_and_stopping_wins_over_buffered_success() {
        let (reply, responses) = unbounded();
        reply.try_send((1, false)).unwrap();
        reply.try_send((2, true)).unwrap();
        assert_eq!(matching(&responses, 2).await, Some(true));
        reply.try_send((3, true)).unwrap();
        reply.close();
        assert_eq!(matching(&responses, 3).await, None);
    }

    #[tokio::test]
    async fn closing_replies_wakes_an_active_cycle() {
        let (reply, responses) = unbounded::<(u32, bool)>();
        let mut wait = Box::pin(matching(&responses, 1));
        assert!(futures_util::poll!(&mut wait).is_pending());
        reply.close();
        assert_eq!(wait.await, None);
    }

    #[tokio::test]
    async fn dropping_a_request_before_polling_or_while_waiting_cancels_it() {
        let cancelled = Cell::new(false);
        let (_reply, response) = bounded::<()>(1);
        let owner = OwnedReply::new(response, || cancelled.set(true));
        let request = async move {
            let _owner = owner;
            pending::<()>().await
        };
        drop(request);
        assert!(cancelled.get());

        cancelled.set(false);
        let (_reply, response) = bounded::<()>(1);
        let owner = OwnedReply::new(response, || cancelled.set(true));
        let mut request = Box::pin(async move { owner.receive().await });
        assert!(futures_util::poll!(&mut request).is_pending());
        drop(request);
        assert!(cancelled.get());
    }

    #[test]
    fn a_buffered_reply_prevents_cancellation_when_the_request_is_dropped() {
        let cancelled = Cell::new(false);
        let (reply, response) = bounded(1);
        let owner = OwnedReply::new(response, || cancelled.set(true));
        send_once(&reply, ());
        drop(owner);
        assert!(!cancelled.get());
    }
}
