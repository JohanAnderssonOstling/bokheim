//! Opt-in timings for the running import, without retaining source bodies.
use serde_json::json;
use std::time::Instant;

pub struct Span {
    stage: &'static str,
    label: String,
    started: Instant,
    enabled: bool,
}

pub fn measure(stage: &'static str, label: impl Into<String>) -> Span {
    Span { stage, label: label.into(), started: Instant::now(), enabled: std::env::var("BOKHEIM_CATALOG_TIMINGS").as_deref() == Ok("1") }
}

impl Drop for Span {
    fn drop(&mut self) {
        if self.enabled {
            eprintln!("catalog-timing {}", json!({"stage":self.stage,"label":self.label,"ms":self.started.elapsed().as_secs_f64() * 1000.0}));
        }
    }
}
