//! Short storage commands. A command cannot retain storage while awaiting: the
//! queue accepts synchronous closures and delivers owned results afterwards.

use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Priority {
    Interactive,
    Background,
}

const INTERACTIVE_BURST: usize = 8;
const MAX_QUEUED_COMMANDS: usize = 256;

struct Queue<T> {
    interactive: VecDeque<T>,
    background: VecDeque<T>,
    interactive_run: usize,
}

impl<T> Default for Queue<T> {
    fn default() -> Self {
        Self { interactive: VecDeque::new(), background: VecDeque::new(), interactive_run: 0 }
    }
}

impl<T> Queue<T> {
    fn push(&mut self, priority: Priority, command: T) -> Result<(), T> {
        if self.interactive.len() + self.background.len() >= MAX_QUEUED_COMMANDS {
            return Err(command);
        }
        match priority {
            Priority::Interactive => self.interactive.push_back(command),
            Priority::Background => self.background.push_back(command),
        }
        Ok(())
    }

    fn pop(&mut self) -> Option<T> {
        if !self.background.is_empty() && (self.interactive.is_empty() || self.interactive_run >= INTERACTIVE_BURST) {
            self.interactive_run = 0;
            return self.background.pop_front();
        }
        if let Some(command) = self.interactive.pop_front() {
            self.interactive_run += 1;
            return Some(command);
        }
        self.interactive_run = 0;
        None
    }
}

#[cfg(target_arch = "wasm32")]
mod browser {
    use super::*;
    use std::cell::{Cell, RefCell};

    thread_local! {
        static COMMANDS: RefCell<Queue<Box<dyn FnOnce()>>> = RefCell::new(Queue::default());
        static RUNNING: Cell<bool> = const { Cell::new(false) };
    }

    pub(super) async fn run<T: 'static>(priority: Priority, operation: impl FnOnce() -> T + 'static) -> Result<T, String> {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let command: Box<dyn FnOnce()> = Box::new(move || {
            // A cancelled caller must not start a queued write. Once a short
            // command starts, its transaction completes atomically.
            if !sender.is_closed() {
                let result = operation();
                let _ = sender.send(result);
            }
        });
        COMMANDS.with(|commands| commands.borrow_mut().push(priority, command)).map_err(|_| "storage command queue is full; retry the operation".to_owned())?;
        if !RUNNING.with(|running| running.replace(true)) {
            wasm_bindgen_futures::spawn_local(async {
                loop {
                    // A macrotask boundary lets new interactive messages arrive;
                    // chaining ready Rust futures alone would starve that loop.
                    crate::executor::sleep(std::time::Duration::ZERO).await;
                    let command = COMMANDS.with(|commands| commands.borrow_mut().pop());
                    match command {
                        Some(command) => command(),
                        None => {
                            RUNNING.with(|running| running.set(false));
                            break;
                        }
                    }
                }
            });
        }
        receiver.await.map_err(|_| "storage command stopped before returning a result".to_owned())
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::*;
    use std::cell::Cell;
    use std::sync::{mpsc, Arc, Mutex, OnceLock};

    type Command = Box<dyn FnOnce() + Send>;
    static DISPATCHER: OnceLock<Result<Dispatcher, String>> = OnceLock::new();
    thread_local! {
        static ON_WORKER: Cell<bool> = const { Cell::new(false) };
    }

    struct Dispatcher {
        commands: Arc<Mutex<Queue<Command>>>,
        wake: mpsc::SyncSender<()>,
    }

    impl Dispatcher {
        fn new() -> Result<Self, String> {
            let commands = Arc::new(Mutex::new(Queue::<Command>::default()));
            let pending = commands.clone();
            // Coalesce wakeups; command memory is bounded by Queue itself.
            let (wake, ready) = mpsc::sync_channel(1);
            std::thread::Builder::new()
                .name("storage-commands".into())
                .spawn(move || {
                    ON_WORKER.set(true);
                    while ready.recv().is_ok() {
                        loop {
                            let command = pending.lock().unwrap().pop();
                            let Some(command) = command else { break };
                            // A failed command drops its reply, but cannot kill the
                            // dispatcher and strand every subsequent operation.
                            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(command));
                        }
                    }
                })
                .map_err(|error| format!("cannot start storage worker: {error}"))?;
            Ok(Self { commands, wake })
        }

        fn submit<T: Send + 'static>(&self, priority: Priority, operation: impl FnOnce() -> T + Send + 'static) -> Result<tokio::sync::oneshot::Receiver<T>, String> {
            let (sender, receiver) = tokio::sync::oneshot::channel();
            // Preserve the submitting runtime context previously supplied by
            // spawn_blocking, including storage write observers.
            let runtime = tokio::runtime::Handle::try_current().ok();
            let command: Command = Box::new(move || {
                if !sender.is_closed() {
                    let _entered = runtime.as_ref().map(|handle| handle.enter());
                    let result = operation();
                    let _ = sender.send(result);
                }
            });
            self.enqueue(priority, command)?;
            Ok(receiver)
        }

        fn enqueue(&self, priority: Priority, command: Command) -> Result<(), String> {
            self.commands.lock().unwrap().push(priority, command).map_err(|_| "storage command queue is full; retry the operation".to_owned())?;
            self.wake()
        }

        fn wake(&self) -> Result<(), String> {
            match self.wake.try_send(()) {
                Ok(()) | Err(mpsc::TrySendError::Full(())) => Ok(()),
                Err(mpsc::TrySendError::Disconnected(())) => Err("storage worker stopped".into()),
            }
        }

        fn dispatch_cleanup(&self, operation: impl FnOnce() + Send + 'static) -> Result<(), String> {
            // Closing already-admitted resources must still succeed when request
            // admission is full. Cleanup does not allocate new storage resources.
            self.commands.lock().unwrap().background.push_back(Box::new(operation));
            self.wake()
        }
    }

    fn dispatcher() -> Result<&'static Dispatcher, String> {
        DISPATCHER.get_or_init(Dispatcher::new).as_ref().map_err(Clone::clone)
    }

    pub(super) fn run_sync<T: Send + 'static>(priority: Priority, operation: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
        if ON_WORKER.get() {
            return Ok(operation());
        }
        let (sender, receiver) = mpsc::sync_channel(1);
        let runtime = tokio::runtime::Handle::try_current().ok();
        dispatcher()?.enqueue(
            priority,
            Box::new(move || {
                let _entered = runtime.as_ref().map(|handle| handle.enter());
                let _ = sender.send(operation());
            }),
        )?;
        receiver.recv().map_err(|_| "storage command stopped before returning a result".to_owned())
    }

    pub(super) fn dispatch_cleanup(operation: impl FnOnce() + Send + 'static) -> Result<(), String> {
        dispatcher()?.dispatch_cleanup(operation)
    }

    pub(super) async fn run<T: Send + 'static>(priority: Priority, operation: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
        dispatcher()?.submit(priority, operation)?.await.map_err(|_| "storage command stopped before returning a result".to_owned())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::time::Duration;

        #[test]
        fn native_worker_prioritizes_interactive_and_skips_cancelled_writes() {
            let dispatcher = Dispatcher::new().unwrap();
            let (started, running) = mpsc::channel();
            let (release, resume) = mpsc::channel();
            let first = dispatcher
                .submit(Priority::Background, move || {
                    started.send(()).unwrap();
                    resume.recv_timeout(Duration::from_secs(5)).unwrap();
                })
                .unwrap();
            running.recv_timeout(Duration::from_secs(5)).unwrap();
            let (observed, order) = mpsc::channel();
            let cancelled = dispatcher
                .submit(Priority::Background, {
                    let observed = observed.clone();
                    move || observed.send("cancelled").unwrap()
                })
                .unwrap();
            drop(cancelled);
            let background = dispatcher
                .submit(Priority::Background, {
                    let observed = observed.clone();
                    move || observed.send("background").unwrap()
                })
                .unwrap();
            let interactive = dispatcher.submit(Priority::Interactive, move || observed.send("interactive").unwrap()).unwrap();
            release.send(()).unwrap();
            first.blocking_recv().unwrap();
            interactive.blocking_recv().unwrap();
            background.blocking_recv().unwrap();
            assert_eq!(order.recv_timeout(Duration::from_secs(5)).unwrap(), "interactive");
            assert_eq!(order.recv_timeout(Duration::from_secs(5)).unwrap(), "background");
            assert!(order.try_recv().is_err());
        }

        #[test]
        fn panicking_command_does_not_strand_following_work() {
            let dispatcher = Dispatcher::new().unwrap();
            let failed = dispatcher.submit(Priority::Background, || panic!("test command failure")).unwrap();
            assert!(failed.blocking_recv().is_err());
            assert_eq!(dispatcher.submit(Priority::Interactive, || 42).unwrap().blocking_recv().unwrap(), 42);
        }

        #[test]
        fn nested_synchronous_commands_use_the_same_worker() {
            let owner = run_sync(Priority::Interactive, || {
                let thread = std::thread::current().id();
                assert_eq!(run_sync(Priority::Interactive, || std::thread::current().id()).unwrap(), thread);
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
                .submit(Priority::Background, move || {
                    started.send(()).unwrap();
                    resume.recv_timeout(Duration::from_secs(5)).unwrap();
                })
                .unwrap();
            running.recv_timeout(Duration::from_secs(5)).unwrap();
            let pending = (0..MAX_QUEUED_COMMANDS).map(|_| dispatcher.submit(Priority::Background, || ()).unwrap()).collect::<Vec<_>>();
            assert!(dispatcher.submit(Priority::Interactive, || ()).is_err());
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

/// Synchronous storage boundaries use the same owner as async commands. Nested
/// operations on that owner execute directly rather than waiting on themselves.
/// The browser already confines synchronous calls to its storage event loop.
pub fn run_sync<T: crate::executor::BackendOutput>(priority: Priority, operation: impl FnOnce() -> T + crate::executor::BackendSend + 'static) -> Result<T, String> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = priority;
        Ok(operation())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        native::run_sync(priority, operation)
    }
}

/// Queues owner-side cleanup without waiting for it or cancelling it when the
/// caller drops its handle. Cleanup must not retain a database across an await.
pub fn dispatch_cleanup(operation: impl FnOnce() + crate::executor::BackendSend + 'static) -> Result<(), String> {
    #[cfg(target_arch = "wasm32")]
    {
        wasm_bindgen_futures::spawn_local(async move { operation() });
        Ok(())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        native::dispatch_cleanup(operation)
    }
}

pub async fn run<T: crate::executor::BackendOutput>(priority: Priority, operation: impl FnOnce() -> T + crate::executor::BackendSend + 'static) -> Result<T, String> {
    #[cfg(target_arch = "wasm32")]
    {
        browser::run(priority, operation).await
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        native::run(priority, operation).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readers_overtake_background_but_cannot_starve_it() {
        let mut queue = Queue::default();
        queue.push(Priority::Background, 100).unwrap();
        for id in 0..20 {
            queue.push(Priority::Interactive, id).unwrap();
        }
        for id in 0..INTERACTIVE_BURST {
            assert_eq!(queue.pop(), Some(id));
        }
        assert_eq!(queue.pop(), Some(100));
        for id in INTERACTIVE_BURST..20 {
            assert_eq!(queue.pop(), Some(id));
        }
        assert_eq!(queue.pop(), None);
    }
}
