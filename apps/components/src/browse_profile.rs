//! Browser profiling shares its timeline with GPUI page rendering.

pub struct Stage(gpui::profiler::Stage);

impl Stage {
    pub fn new(name: &'static str) -> Self {
        Self(gpui::profiler::Stage::new("browse_profile", name))
    }
    pub fn deferred(name: &'static str) -> Self {
        Self(gpui::profiler::Stage::deferred("browse_profile", name))
    }
    pub fn finish(self, outcome: &'static str) {
        self.0.finish(outcome);
    }
    pub fn cancel(self) {
        self.0.cancel();
    }
}
