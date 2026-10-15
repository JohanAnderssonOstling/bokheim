//! Wake scheduling for one library's durable workers.

use std::time::Duration;
use sync_common::ContentHash;

pub enum SyncRequest {
    Wake(Vec<ContentHash>),
    WakeAfter(Duration),
    RefreshRemote,
    Stop,
}

/// What one scheduled cycle should do: whether server state needs a refresh,
/// plus content hashes woken with payload since the previous cycle. Payload
/// hashes are latency hints only; the database remains the work queue, so a
/// targeted pass never replaces the bounded drain.
pub struct CycleInput {
    pub remote: bool,
    pub hashes: Vec<ContentHash>,
}

#[derive(Clone)]
pub struct SyncScheduler(async_channel::Sender<SyncRequest>);

impl SyncScheduler {
    pub fn new() -> (Self, async_channel::Receiver<SyncRequest>) {
        let (sender, receiver) = async_channel::unbounded();
        (Self(sender), receiver)
    }
    pub fn request_remote_refresh(&self) {
        let _ = self.0.try_send(SyncRequest::RefreshRemote);
    }
    pub fn wake(&self) {
        let _ = self.0.try_send(SyncRequest::Wake(Vec::new()));
    }
    pub fn wake_after(&self, delay: Duration) {
        let _ = self.0.try_send(SyncRequest::WakeAfter(delay));
    }
    pub fn wake_hashes(&self, hashes: Vec<ContentHash>) {
        let _ = self.0.try_send(SyncRequest::Wake(hashes));
    }
    pub fn stop(&self) {
        let _ = self.0.try_send(SyncRequest::Stop);
    }
}

pub async fn run_timed<C>(mut cycle: C, requests: async_channel::Receiver<SyncRequest>, retry: Duration, debounce: Duration)
where
    C: AsyncFnMut(CycleInput) -> Result<(), ()>,
{
    let started = web_time::Instant::now();
    let now = || started.elapsed().as_secs_f64() * 1000.0;
    let mut timing = BackgroundTiming::new(now(), retry.as_secs_f64() * 1000.0, debounce.as_secs_f64() * 1000.0);
    let mut pending: Vec<ContentHash> = Vec::new();
    loop {
        while let Ok(request) = requests.try_recv() {
            if matches!(request, SyncRequest::Stop) {
                return;
            }
            apply(&mut timing, now(), request, &mut pending);
        }
        let Some(remote) = timing.start(now()) else {
            let deadline = timing.deadline();
            let request = if deadline.is_finite() { crate::executor::timeout(Duration::from_secs_f64(((deadline - now()).max(0.0)) / 1000.0), requests.recv()).await } else { Ok(requests.recv().await) };
            match request {
                Ok(Ok(SyncRequest::Stop)) | Ok(Err(_)) => return,
                Ok(Ok(request)) => apply(&mut timing, now(), request, &mut pending),
                Err(_) => {}
            }
            continue;
        };
        let running = cycle(CycleInput { remote, hashes: std::mem::take(&mut pending) });
        tokio::pin!(running);
        loop {
            tokio::select! {
                biased;
                result = &mut running => { timing.finish(now(), result.is_ok()); break; }
                request = requests.recv() => match request { Ok(SyncRequest::Stop) | Err(_) => return, Ok(request) => apply(&mut timing, now(), request, &mut pending) },
            }
        }
    }
}

fn apply(timing: &mut BackgroundTiming, now: f64, request: SyncRequest, pending: &mut Vec<ContentHash>) {
    match request {
        SyncRequest::Wake(hashes) => {
            timing.wake(now);
            for hash in hashes {
                if !pending.contains(&hash) {
                    pending.push(hash);
                }
            }
        }
        SyncRequest::WakeAfter(delay) => timing.wake_after(now, delay.as_secs_f64() * 1000.0),
        SyncRequest::RefreshRemote => timing.refresh(now),
        SyncRequest::Stop => {}
    }
}

pub struct BackgroundTiming {
    retry: f64,
    debounce: f64,
    next_remote: f64,
    not_before: f64,
    wake_at: f64,
    refresh_version: u64,
    active: Option<(bool, u64)>,
}
impl BackgroundTiming {
    pub fn new(now: f64, retry: f64, debounce: f64) -> Self {
        Self { retry, debounce, next_remote: f64::INFINITY, not_before: now, wake_at: f64::INFINITY, refresh_version: 0, active: None }
    }
    pub fn wake(&mut self, now: f64) {
        self.wake_at = self.wake_at.min(now + self.debounce);
    }
    pub fn wake_after(&mut self, now: f64, delay: f64) {
        self.wake_at = self.wake_at.min(now + delay);
    }
    pub fn refresh(&mut self, now: f64) {
        let first = self.active == Some((true, self.refresh_version));
        self.refresh_version += 1;
        self.next_remote = if first { now } else { self.next_remote.min(now) };
        self.not_before = now;
    }
    pub fn deadline(&self) -> f64 {
        if self.active.is_some() { f64::INFINITY } else { self.next_remote.min(self.wake_at).max(self.not_before) }
    }
    pub fn start(&mut self, now: f64) -> Option<bool> {
        if now < self.deadline() {
            return None;
        }
        let remote = now >= self.next_remote;
        self.active = Some((remote, self.refresh_version));
        self.wake_at = f64::INFINITY;
        Some(remote)
    }
    pub fn finish(&mut self, now: f64, ok: bool) {
        let Some((remote, version)) = self.active.take() else {
            return;
        };
        if !ok {
            self.wake_at = self.wake_at.min(now + self.retry);
        }
        let changed = version != self.refresh_version;
        self.not_before = if ok || changed { now } else { now + self.retry };
        if remote && !changed {
            self.next_remote = if ok { f64::INFINITY } else { now + self.retry };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_buffered_stop_prevents_starting_another_cycle() {
        let (scheduler, requests) = SyncScheduler::new();
        scheduler.wake();
        scheduler.stop();
        drop(scheduler);
        run_timed(async |_| panic!("a stopped worker must not start another cycle"), requests, Duration::from_secs(30), Duration::ZERO).await;
    }

    #[tokio::test]
    async fn cycles_borrow_owned_state_across_awaits_and_wakes_do_not_reenter_it() {
        let (scheduler, requests) = SyncScheduler::new();
        let (completed, results) = async_channel::unbounded();
        let mut passes = 0;
        scheduler.wake();
        let running = run_timed(
            async |_| {
                passes += 1;
                let count = &mut passes;
                tokio::task::yield_now().await;
                completed.send(*count).await.unwrap();
                Ok(())
            },
            requests,
            Duration::from_secs(30),
            Duration::ZERO,
        );
        let exercise = async {
            assert_eq!(results.recv().await.unwrap(), 1);
            scheduler.wake();
            assert_eq!(results.recv().await.unwrap(), 2);
            scheduler.stop();
        };
        tokio::join!(running, exercise);
        assert_eq!(passes, 2);
    }
}
