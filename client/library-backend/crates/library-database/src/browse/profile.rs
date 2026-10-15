//! Opt-in preparation timings, enabled through the normal logging filter.
use web_time::Instant;

pub(super) struct Stage {
    name: &'static str,
    start: Option<Instant>,
}

impl Stage {
    pub(super) fn new(name: &'static str) -> Self {
        Self { name, start: log::log_enabled!(target: "browse_profile", log::Level::Debug).then(Instant::now) }
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        if let Some(start) = self.start {
            log::debug!(target: "browse_profile", "{} {:.3} ms", self.name, start.elapsed().as_secs_f64() * 1000.0);
        }
    }
}
