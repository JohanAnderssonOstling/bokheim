//! Ordered synchronous transactions and persistence. Ordinary independent I/O
//! uses the blocking pool; this boundary keeps admitted mutations in FIFO order.

use crate::executor::{BackendOutput, BackendSend};
#[cfg(target_arch = "wasm32")]
use crate::executor::BackendExecutor;

const MAX_QUEUED_COMMANDS: usize = 256;

#[cfg(target_arch = "wasm32")]
static PENDING: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(MAX_QUEUED_COMMANDS);

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::MAX_QUEUED_COMMANDS;
    use std::cell::Cell;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, PoisonError, mpsc};

    type Command = Box<dyn FnOnce() + Send>;
    static DISPATCHER: DispatcherSlot = DispatcherSlot(Mutex::new(None));
    thread_local! {
        static ON_WORKER: Cell<bool> = const { Cell::new(false) };
    }

    #[derive(Clone)]
    struct Dispatcher {
        commands: mpsc::Sender<Command>,
        queued: Arc<AtomicUsize>,
    }

    #[derive(Default)]
    struct DispatcherSlot(Mutex<Option<Dispatcher>>);

    impl DispatcherSlot {
        fn get_or_start(&self, start: impl FnOnce() -> Result<Dispatcher, String>) -> Result<Dispatcher, String> {
            let mut slot = self.0.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(dispatcher) = &*slot {
                return Ok(dispatcher.clone());
            }
            let dispatcher = start()?;
            *slot = Some(dispatcher.clone());
            Ok(dispatcher)
        }
    }

    impl Dispatcher {
        fn new() -> Result<Self, String> {
            let (commands, incoming) = mpsc::channel::<Command>();
            std::thread::Builder::new()
                .name("storage-transactions".into())
                .spawn(move || {
                    ON_WORKER.set(true);
                    while let Ok(command) = incoming.recv() {
                        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(command));
                    }
                })
                .map_err(|error| format!("cannot start storage worker: {error}"))?;
            Ok(Self { commands, queued: Arc::new(AtomicUsize::new(0)) })
        }

        fn enqueue(&self, operation: Command) -> Result<(), String> {
            self.queued.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |queued| (queued < MAX_QUEUED_COMMANDS).then_some(queued + 1))
                .map_err(|_| "storage transaction queue is full; retry the operation".to_owned())?;
            let queued = self.queued.clone();
            if self.commands.send(Box::new(move || {
                queued.fetch_sub(1, Ordering::Relaxed);
                operation();
            })).is_err() {
                self.queued.fetch_sub(1, Ordering::Relaxed);
                return Err("storage worker stopped".into());
            }
            Ok(())
        }

        fn submit<T: Send + 'static>(&self, operation: impl FnOnce() -> T + Send + 'static) -> Result<tokio::sync::oneshot::Receiver<T>, String> {
            let (sender, receiver) = tokio::sync::oneshot::channel();
            let runtime = tokio::runtime::Handle::try_current().ok();
            self.enqueue(Box::new(move || {
                if !sender.is_closed() {
                    let _entered = runtime.as_ref().map(|handle| handle.enter());
                    let _ = sender.send(crate::profiling::operation(operation));
                }
            }))?;
            Ok(receiver)
        }

        fn dispatch_cleanup(&self, operation: impl FnOnce() + Send + 'static) -> Result<(), String> {
            // Already-admitted persistence and disposal bypass request admission,
            // but share the same FIFO so shutdown barriers retain their meaning.
            self.commands.send(Box::new(operation)).map_err(|_| "storage worker stopped".to_owned())
        }
    }

    fn dispatcher() -> Result<Dispatcher, String> {
        DISPATCHER.get_or_start(Dispatcher::new)
    }

    pub(super) fn run_sync<T: Send + 'static>(operation: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
        if ON_WORKER.get() {
            return Ok(operation());
        }
        let (sender, receiver) = mpsc::sync_channel(1);
        let runtime = tokio::runtime::Handle::try_current().ok();
        dispatcher()?.enqueue(Box::new(move || {
            let _entered = runtime.as_ref().map(|handle| handle.enter());
            let _ = sender.send(crate::profiling::operation(operation));
        }))?;
        receiver.recv().map_err(|_| "storage transaction stopped before returning a result".to_owned())
    }

    pub(super) fn dispatch_cleanup(operation: impl FnOnce() + Send + 'static) -> Result<(), String> {
        dispatcher()?.dispatch_cleanup(operation)
    }

    pub(super) async fn run<T: Send + 'static>(operation: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
        if ON_WORKER.get() {
            return Ok(operation());
        }
        dispatcher()?.submit(operation)?.await.map_err(|_| "storage transaction stopped before returning a result".to_owned())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::time::Duration;

        #[test]
        fn failed_startup_can_retry_without_replacing_a_running_worker() {
            let slot = DispatcherSlot::default();
            assert!(slot.get_or_start(|| Err("temporary thread-creation failure".into())).is_err());
            let dispatcher = slot.get_or_start(Dispatcher::new).unwrap();
            let owner = dispatcher.submit(|| std::thread::current().id()).unwrap().blocking_recv().unwrap();
            let retained = slot.get_or_start(|| panic!("a running worker must be reused")).unwrap();
            assert_eq!(retained.submit(|| std::thread::current().id()).unwrap().blocking_recv().unwrap(), owner);
        }

        #[test]
        fn panicking_command_does_not_strand_following_work() {
            let dispatcher = Dispatcher::new().unwrap();
            let failed = dispatcher.submit(|| panic!("test command failure")).unwrap();
            assert!(failed.blocking_recv().is_err());
            assert_eq!(dispatcher.submit(|| 42).unwrap().blocking_recv().unwrap(), 42);
        }

        #[test]
        fn nested_synchronous_commands_use_the_same_worker() {
            let owner = run_sync(|| {
                let thread = std::thread::current().id();
                assert_eq!(run_sync(|| std::thread::current().id()).unwrap(), thread);
                thread
            })
            .unwrap();
            assert_ne!(owner, std::thread::current().id());
        }

        #[test]
        fn cleanup_is_admitted_when_the_request_queue_is_full() {
            let dispatcher = Dispatcher::new().unwrap();
            let (started, running) = mpsc::channel();
            let (release, resume) = mpsc::channel();
            let first = dispatcher
                .submit(move || {
                    started.send(()).unwrap();
                    resume.recv_timeout(Duration::from_secs(5)).unwrap();
                })
                .unwrap();
            running.recv_timeout(Duration::from_secs(5)).unwrap();
            let pending = (0..MAX_QUEUED_COMMANDS).map(|_| dispatcher.submit(|| ()).unwrap()).collect::<Vec<_>>();
            assert!(dispatcher.submit(|| ()).is_err());
            let (closed, observed) = mpsc::channel();
            dispatcher.dispatch_cleanup(move || closed.send(()).unwrap()).unwrap();
            release.send(()).unwrap();
            first.blocking_recv().unwrap();
            for result in pending {
                result.blocking_recv().unwrap();
            }
            observed.recv_timeout(Duration::from_secs(5)).unwrap();
        }
    }
}

/// Synchronous calls already on the owner execute directly, avoiding a nested
/// request that would wait on itself. Browser callers must be on their OPFS owner.
pub fn run_sync<T: BackendOutput>(operation: impl FnOnce() -> T + BackendSend + 'static) -> Result<T, String> {
    #[cfg(target_arch = "wasm32")]
    {
        Ok(operation())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        native::run_sync(operation)
    }
}

pub fn dispatch_cleanup(operation: impl FnOnce() + BackendSend + Send + 'static) -> Result<(), String> {
    #[cfg(target_arch = "wasm32")]
    {
        crate::executor::storage_cleanup(operation);
        Ok(())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        native::dispatch_cleanup(operation)
    }
}

pub async fn run<T: BackendOutput + Send>(operation: impl FnOnce() -> T + BackendSend + Send + 'static) -> Result<T, String> {
    #[cfg(target_arch = "wasm32")]
    {
        let permit = PENDING.try_acquire().map_err(|_| "storage transaction queue is full; retry the operation".to_owned())?;
        let (reply, result) = tokio::sync::oneshot::channel();
        BackendExecutor::default().dispatch_storage(move || {
            drop(permit);
            if !reply.is_closed() {
                let _ = reply.send(operation());
            }
        }).map_err(|error| error.to_string())?;
        result.await.map_err(|_| "storage transaction stopped before returning a result".to_owned())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        native::run(operation).await
    }
}

/// Persistence shares transaction ordering and remains admissible during shutdown.
#[cfg(not(target_arch = "wasm32"))]
pub fn dispatch_persistence(operation: impl FnOnce() + Send + 'static) -> Result<(), String> {
    native::dispatch_cleanup(operation)
}
