//! Optional startup timings. Enable with `RUST_LOG=startup_profile=debug`.
//! Android also accepts the boolean launch extra `bokheim.startup_profile`.
use std::sync::{OnceLock, atomic::{AtomicBool, Ordering}};
use std::fmt::Display;
use web_time::Instant;

static ORIGIN: OnceLock<Instant> = OnceLock::new();
static ENABLED: AtomicBool = AtomicBool::new(false);

/// Records consecutive stages within a scope. Nested scopes overlap, so their
/// durations must not be added to their parent's duration.
pub struct StartupProfile {
    scope: &'static str,
    started: Option<Instant>,
    previous: Option<Instant>,
    library_id: Option<String>,
}

impl StartupProfile {
    /// Enables timings at info level for hosts without an environment-based
    /// logger, such as Android's opt-in launch-intent profiler.
    pub fn enable() {
        ENABLED.store(true, Ordering::Relaxed);
    }

    pub fn enabled() -> bool {
        ENABLED.load(Ordering::Relaxed) || log::log_enabled!(target: "startup_profile", log::Level::Debug)
    }

    pub fn new(scope: &'static str) -> Self {
        let started = Self::enabled().then(Instant::now);
        if let Some(started) = started {
            ORIGIN.get_or_init(|| started);
        }
        Self { scope, started, previous: started, library_id: None }
    }

    pub fn for_library(scope: &'static str, id: impl Display) -> Self {
        let mut profile = Self::new(scope);
        if profile.started.is_some() {
            profile.library_id = Some(id.to_string());
        }
        profile
    }

    pub fn mark(&mut self, stage: &'static str) {
        if let (Some(started), Some(previous)) = (self.started, self.previous) {
            let now = Instant::now();
            self.previous = Some(now);
            let level = if ENABLED.load(Ordering::Relaxed) { log::Level::Info } else { log::Level::Debug };
            log::log!(target: "startup_profile", level, "scope={} stage={} duration_ms={:.3} elapsed_ms={:.3} since_start_ms={:.3} library_id={}",
                self.scope, stage, (now - previous).as_secs_f64() * 1000.0,
                (now - started).as_secs_f64() * 1000.0,
                now.duration_since(*ORIGIN.get_or_init(|| started)).as_secs_f64() * 1000.0,
                self.library_id.as_deref().unwrap_or("-"));
        }
    }
}
