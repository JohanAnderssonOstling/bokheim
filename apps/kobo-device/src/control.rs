//! Async device controls with hardware work serialized independently of application jobs.
use crate::{battery, brightness, natural_light};
use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Mutex, OnceLock, PoisonError, mpsc};
use tokio::sync::oneshot;

type Job = Box<dyn FnOnce() + Send>;

struct Worker {
    name: &'static str,
    sender: Mutex<Option<mpsc::Sender<Job>>>,
}

impl Worker {
    const fn new(name: &'static str) -> Self {
        Self { name, sender: Mutex::new(None) }
    }

    async fn run<T: Send + 'static>(&self, operation: impl FnOnce() -> io::Result<T> + Send + 'static) -> io::Result<T> {
        let receiver = {
            let mut sender = self.sender.lock().unwrap_or_else(PoisonError::into_inner);
            if sender.is_none() {
                let (jobs, pending) = mpsc::channel::<Job>();
                std::thread::Builder::new().name(self.name.into()).spawn(move || {
                    while let Ok(job) = pending.recv() {
                        let _ = catch_unwind(AssertUnwindSafe(job));
                    }
                })?;
                *sender = Some(jobs);
            }
            let (reply, receiver) = oneshot::channel();
            sender
                .as_ref()
                .unwrap()
                .send(Box::new(move || {
                    if !reply.is_closed() {
                        let _ = reply.send(operation());
                    }
                }))
                .map_err(|_| io::Error::other("device worker stopped"))?;
            receiver
        };
        receiver.await.map_err(|_| io::Error::other("device operation stopped before replying"))?
    }
}

fn controls() -> &'static Worker {
    static WORKER: OnceLock<Worker> = OnceLock::new();
    WORKER.get_or_init(|| Worker::new("kobo-controls"))
}

pub async fn brightness() -> io::Result<u8> {
    controls().run(brightness::read).await
}

pub async fn set_brightness(percent: u8) -> io::Result<u8> {
    controls().run(move || brightness::set(percent)).await
}

pub async fn natural_light() -> io::Result<Option<u8>> {
    controls().run(natural_light::read).await
}

pub async fn set_natural_light(percent: u8) -> io::Result<u8> {
    controls().run(move || natural_light::set(percent)).await
}

pub async fn battery() -> io::Result<battery::BatteryStatus> {
    controls().run(battery::read).await
}

#[cfg(target_os = "linux")]
pub async fn bluetooth(request: crate::bluetooth::Request) -> io::Result<crate::bluetooth::Status> {
    static WORKER: Worker = Worker::new("kobo-bluetooth-controls");
    WORKER.run(move || crate::bluetooth::request(request)).await
}
