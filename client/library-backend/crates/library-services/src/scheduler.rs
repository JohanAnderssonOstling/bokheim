//! Coalesced wake scheduling for one library's durable worker.

use client_platform_runtime::executor::timeout;
use std::{future::Future, mem::take, time::Duration};
use tokio::sync::watch;
use web_time::Instant;

/// Pending wake state. Repeated requests retain the earliest deadline.
#[derive(Clone, Copy, Default)]
pub struct Schedule {
    pub wake_requested_at: Option<Instant>,
    pub deadline: Option<Instant>,
    pub refresh_remote: bool,
}

#[derive(Clone)]
pub struct SyncScheduler(watch::Sender<Schedule>);

impl Default for SyncScheduler {
    fn default() -> Self {
        Self(watch::channel(Schedule::default()).0)
    }
}

impl SyncScheduler {
    pub fn subscribe(&self) -> watch::Receiver<Schedule> {
        self.0.subscribe()
    }
    pub fn request_remote_refresh(&self) {
        self.0.send_modify(|schedule| schedule.refresh_remote = true);
    }
    pub fn wake(&self) {
        let now = Instant::now();
        self.0.send_modify(|schedule| schedule.wake_requested_at = Some(schedule.wake_requested_at.map_or(now, |at| at.min(now))));
    }
    pub fn wake_after(&self, delay: Duration) {
        let deadline = Instant::now() + delay;
        self.0.send_modify(|schedule| schedule.deadline = Some(schedule.deadline.map_or(deadline, |at| at.min(deadline))));
    }
    fn take_pending(&self) -> Schedule {
        let mut pending = Schedule::default();
        self.0.send_if_modified(|schedule| {
            if schedule.wake_requested_at.is_none() && schedule.deadline.is_none() && !schedule.refresh_remote {
                return false;
            }
            pending = take(schedule);
            true
        });
        pending
    }
}

/// The cycle callback receives whether a remote refresh is due.
/// Dropping this future stops the worker and its current cycle.
pub async fn run_timed<C, F>(cycle: C, scheduler: &SyncScheduler, retry: Duration, debounce: Duration)
where
    C: FnMut(bool) -> F,
    F: Future<Output = Result<(), ()>>,
{
    run_timed_with_backoff(cycle, scheduler, retry, retry, debounce).await;
}

/// Battery-sensitive hosts gradually reduce repeated failed work. An explicit
/// remote refresh bypasses the delay; success restores the initial retry rate.
pub async fn run_timed_with_backoff<C, F>(mut cycle: C, scheduler: &SyncScheduler, retry: Duration, retry_max: Duration, debounce: Duration)
where
    C: FnMut(bool) -> F,
    F: Future<Output = Result<(), ()>>,
{
    let started = Instant::now();
    let now = || started.elapsed().as_secs_f64() * 1000.0;
    let mut changes = scheduler.subscribe();
    let mut timing = BackgroundTiming::with_backoff(now(), retry.as_secs_f64() * 1000.0, retry_max.as_secs_f64() * 1000.0, debounce.as_secs_f64() * 1000.0);
    loop {
        changes.borrow_and_update();
        apply(&mut timing, started, scheduler.take_pending());
        let Some(remote) = timing.start(now()) else {
            let deadline = timing.deadline();
            if deadline.is_finite() {
                let _ = timeout(Duration::from_secs_f64(((deadline - now()).max(0.0)) / 1000.0), changes.changed()).await;
            } else {
                let _ = changes.changed().await;
            }
            continue;
        };
        let running = cycle(remote);
        tokio::pin!(running);
        loop {
            tokio::select! {
                biased;
                result = &mut running => { timing.finish(now(), result.is_ok()); break; }
                _ = changes.changed() => apply(&mut timing, started, scheduler.take_pending()),
            }
        }
    }
}

fn apply(timing: &mut BackgroundTiming, started: Instant, pending: Schedule) {
    let elapsed = |at: Instant| at.saturating_duration_since(started).as_secs_f64() * 1000.0;
    if let Some(at) = pending.wake_requested_at {
        timing.wake(elapsed(at));
    }
    if let Some(at) = pending.deadline {
        timing.wake_after(elapsed(at), 0.0);
    }
    if pending.refresh_remote {
        timing.refresh(elapsed(Instant::now()));
    }
}

pub struct BackgroundTiming {
    retry: f64,
    retry_max: f64,
    next_retry: f64,
    debounce: f64,
    next_remote: f64,
    not_before: f64,
    wake_at: f64,
    refresh_version: u64,
    active: Option<(bool, u64)>,
}
impl BackgroundTiming {
    pub fn new(now: f64, retry: f64, debounce: f64) -> Self {
        Self::with_backoff(now, retry, retry, debounce)
    }
    fn with_backoff(now: f64, retry: f64, retry_max: f64, debounce: f64) -> Self {
        Self { retry, retry_max: retry_max.max(retry), next_retry: retry, debounce, next_remote: f64::INFINITY, not_before: now, wake_at: f64::INFINITY, refresh_version: 0, active: None }
    }
    pub fn wake(&mut self, now: f64) {
        self.wake_at = self.wake_at.min(now + self.debounce);
    }
    pub fn wake_after(&mut self, now: f64, delay: f64) {
        self.wake_at = self.wake_at.min(now + delay);
    }
    pub fn refresh(&mut self, now: f64) {
        self.next_retry = self.retry;
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
        let changed = version != self.refresh_version;
        let delay = self.next_retry;
        self.next_retry = if ok || changed { self.retry } else { (delay * 2.0).min(self.retry_max) };
        if !ok {
            self.wake_at = self.wake_at.min(now + delay);
        }
        self.not_before = if ok || changed { now } else { now + delay };
        if remote && !changed {
            self.next_remote = if ok { f64::INFINITY } else { now + delay };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_work_backs_off_to_the_limit_and_success_resets_it() {
        let mut timing = BackgroundTiming::with_backoff(0.0, 30.0, 900.0, 0.0);
        timing.refresh(0.0);
        let mut now = 0.0;
        for delay in [30.0, 60.0, 120.0, 240.0, 480.0, 900.0, 900.0] {
            assert_eq!(timing.start(now), Some(true));
            timing.finish(now, false);
            assert_eq!(timing.deadline(), now + delay);
            assert_eq!(timing.start(now + delay - 0.5), None);
            now += delay;
        }
        assert_eq!(timing.start(now), Some(true));
        timing.finish(now, true);
        assert!(timing.deadline().is_infinite());
        timing.wake(now);
        assert_eq!(timing.start(now), Some(false));
        timing.finish(now, false);
        assert_eq!(timing.deadline(), now + 30.0);
    }

    #[test]
    fn explicit_remote_refresh_bypasses_backoff_even_during_a_failed_pass() {
        let mut timing = BackgroundTiming::with_backoff(0.0, 30.0, 900.0, 0.0);
        timing.refresh(0.0);
        assert_eq!(timing.start(0.0), Some(true));
        timing.finish(0.0, false);
        assert_eq!(timing.start(30.0), Some(true));
        timing.finish(30.0, false);
        assert_eq!(timing.deadline(), 90.0);
        timing.refresh(35.0);
        assert_eq!(timing.start(35.0), Some(true));
        timing.refresh(36.0);
        timing.finish(37.0, false);
        assert_eq!(timing.start(37.0), Some(true));
        timing.finish(37.0, false);
        assert_eq!(timing.deadline(), 67.0);
    }

    #[tokio::test]
    async fn wakes_and_refreshes_coalesce_without_reentering_a_running_cycle() {
        let scheduler = SyncScheduler::default();
        let (completed, results) = async_channel::unbounded();
        let (release, released) = async_channel::bounded(1);
        scheduler.wake();
        let running = run_timed(
            async |remote| {
                completed.send(remote).await.unwrap();
                released.recv().await.unwrap();
                Ok(())
            },
            &scheduler,
            Duration::from_secs(30),
            Duration::ZERO,
        );
        let exercise = async {
            assert!(!results.recv().await.unwrap());
            scheduler.wake();
            scheduler.wake();
            scheduler.wake_after(Duration::ZERO);
            scheduler.request_remote_refresh();
            release.send(()).await.unwrap();
            assert!(results.recv().await.unwrap());
            scheduler.wake();
            release.send(()).await.unwrap();
            assert!(!results.recv().await.unwrap());
        };
        timeout(Duration::from_secs(5), async {
            tokio::select! {
                _ = running => panic!("worker ended before cancellation"),
                () = exercise => {},
            }
        })
        .await
        .expect("scheduler lost a wake or failed to stop");
        assert!(results.try_recv().is_err(), "coalesced wakes must not start extra cycles");
    }

    #[tokio::test]
    async fn cycles_lock_owned_state_across_awaits_and_wakes_do_not_reenter_it() {
        let scheduler = SyncScheduler::default();
        let (completed, results) = async_channel::unbounded();
        let passes = tokio::sync::Mutex::new(0);
        scheduler.wake();
        let running = run_timed(
            |_| async {
                let mut count = passes.lock().await;
                *count += 1;
                tokio::task::yield_now().await;
                completed.send(*count).await.unwrap();
                Ok(())
            },
            &scheduler,
            Duration::from_secs(30),
            Duration::ZERO,
        );
        let exercise = async {
            assert_eq!(results.recv().await.unwrap(), 1);
            scheduler.wake();
            assert_eq!(results.recv().await.unwrap(), 2);
        };
        tokio::select! {
            _ = running => panic!("worker ended before cancellation"),
            () = exercise => {},
        }
        assert_eq!(*passes.lock().await, 2);
    }
}
