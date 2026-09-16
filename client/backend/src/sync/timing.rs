//! Clock-independent background scheduling policy shared by native and WASM.
#[cfg_attr(all(target_arch = "wasm32", feature = "web-runtime-tests"), wasm_bindgen::prelude::wasm_bindgen)]
pub struct BackgroundTiming {
    retry: f64,
    debounce: f64,
    next_remote: f64,
    not_before: f64,
    wake_at: f64,
    refresh_version: u64,
    active: Option<(bool, u64)>,
}

#[cfg_attr(all(target_arch = "wasm32", feature = "web-runtime-tests"), wasm_bindgen::prelude::wasm_bindgen)]
impl BackgroundTiming {
    #[cfg_attr(all(target_arch = "wasm32", feature = "web-runtime-tests"), wasm_bindgen::prelude::wasm_bindgen(constructor))]
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
        let first_during_pass = self.active == Some((true, self.refresh_version));
        self.refresh_version += 1;
        self.next_remote = if first_during_pass { now } else { self.next_remote.min(now) };
        self.not_before = now;
    }

    pub fn deadline(&self) -> f64 {
        if self.active.is_some() { f64::INFINITY } else { self.next_remote.min(self.wake_at).max(self.not_before) }
    }

    /// Returns the remote-refresh flag when a cycle is due. Early timers do nothing.
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
        let new_refresh = version != self.refresh_version;
        self.not_before = if ok || new_refresh { now } else { now + self.retry };
        if remote && !new_refresh {
            self.next_remote = if ok { f64::INFINITY } else { now + self.retry };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn timing() -> BackgroundTiming {
        BackgroundTiming::new(0.0, 30.0, 5.0)
    }

    #[test]
    fn login_and_server_notifications_start_without_debounce() {
        let mut t = timing();
        t.wake(10.0);
        t.refresh(11.0);
        assert_eq!(t.start(11.0), Some(true));
        t.refresh(12.0);
        assert_eq!(t.start(12.0), None, "notifications do not overlap an active exchange");
        t.finish(13.0, true);
        assert_eq!(t.start(13.0), Some(true), "the queued change runs as soon as the exchange finishes");
    }

    #[test]
    fn continuous_writes_have_a_fixed_deadline_and_one_followup() {
        let mut t = timing();
        t.wake(0.0);
        t.wake(3.0);
        assert_eq!(t.start(4.9), None);
        assert_eq!(t.start(5.0), Some(false));
        t.wake(5.0);
        assert_eq!(t.start(100.0), None);
        t.finish(100.0, true);
        assert_eq!(t.start(100.0), Some(false));
    }

    #[test]
    fn failure_backs_off_despite_writes() {
        let mut t = timing();
        t.refresh(0.0);
        assert_eq!(t.start(5.0), Some(true));
        t.finish(5.0, false);
        for now in 6..35 {
            t.wake(now as f64);
            assert_eq!(t.start(now as f64), None);
        }
        assert_eq!(t.start(35.0), Some(true));
    }

    #[test]
    fn refresh_during_a_pass_survives_success_or_failure() {
        for ok in [true, false] {
            let mut t = timing();
            t.refresh(0.0);
            t.start(5.0);
            t.refresh(6.0);
            t.refresh(7.0);
            t.finish(7.0, ok);
            assert_eq!(t.start(7.0), Some(true));
        }
    }

    #[test]
    fn idle_and_successful_work_have_no_timer() {
        let mut t = timing();
        assert_eq!(t.deadline(), f64::INFINITY);
        assert_eq!(t.start(30_000.0), None);
        t.wake(30_000.0);
        assert_eq!(t.start(30_005.0), Some(false));
        t.finish(30_006.0, true);
        assert_eq!(t.deadline(), f64::INFINITY);
        t.refresh(30_006.25);
        assert_eq!(t.start(30_006.25), Some(true));
        t.finish(30_012.0, true);
        assert_eq!(t.deadline(), f64::INFINITY);
    }

    #[test]
    fn deferred_work_wakes_once_at_its_deadline() {
        let mut t = timing();
        t.wake_after(0.0, 86400.0);
        t.wake_after(1.0, 86400.0);
        assert_eq!(t.start(86399.0), None);
        assert_eq!(t.start(86400.0), Some(false));
        t.finish(86401.0, true);
        assert_eq!(t.deadline(), f64::INFINITY);
    }

    #[test]
    fn failed_local_work_retries_without_another_wakeup() {
        let mut t = timing();
        t.wake(0.0);
        assert_eq!(t.start(5.0), Some(false));
        t.finish(6.0, false);
        assert_eq!(t.start(35.0), None);
        assert_eq!(t.start(36.0), Some(false));
        t.finish(37.0, true);
        assert_eq!(t.deadline(), f64::INFINITY);
    }
}
