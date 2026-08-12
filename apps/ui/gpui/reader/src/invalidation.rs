use gpui::Context;

/// A view that repaints independently, and the unit the debug counters group
/// by.
///
/// One variant per entity: two names for the same entity would split its
/// counts across two rows and hide how often it actually repaints.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Component {
    /// The EPUB window: document, sidebars and overlays.
    ReaderShell,
    /// The EPUB toolbar, which repaints without the document.
    ReaderToolbar,
    /// The PDF window.
    PdfShell,
    /// The CBZ window.
    CbzShell,
}

impl Component {
    pub fn name(self) -> &'static str {
        match self {
            Self::ReaderShell => "reader_shell",
            Self::ReaderToolbar => "reader_toolbar",
            Self::PdfShell => "pdf_shell",
            Self::CbzShell => "cbz_shell",
        }
    }
}

#[cfg(debug_assertions)]
use std::collections::HashMap;
#[cfg(debug_assertions)]
use std::sync::{Mutex, OnceLock};

#[cfg(debug_assertions)]
#[derive(Clone, Debug, Default)]
pub struct ReaderPerformanceSnapshot {
    pub notifications: Vec<((Component, &'static str), u64)>,
    pub renders: Vec<(Component, u64)>,
    pub drawn_frames: u64,
    pub coalesced_invalidations: u64,
    pub maximum_invalidations_per_frame: u64,
    pub total_draw_micros: u64,
    pub maximum_draw_micros: u64,
}

#[cfg(debug_assertions)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReaderRepaintScenario {
    PageTurn,
    BoundaryNavigation,
}

#[cfg(debug_assertions)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReaderRepaintBudgetEvaluation {
    pub scenario: ReaderRepaintScenario,
    pub passed: bool,
    pub frame_count: u64,
    pub frame_budget: u64,
    pub maximum_invalidations_per_frame: u64,
    pub invalidation_budget: u64,
}

#[cfg(debug_assertions)]
impl ReaderPerformanceSnapshot {
    pub fn evaluate_reader_repaint_budget(&self, scenario: ReaderRepaintScenario) -> ReaderRepaintBudgetEvaluation {
        let (frame_budget, invalidation_budget) = match scenario {
            ReaderRepaintScenario::PageTurn => (1, 4),
            ReaderRepaintScenario::BoundaryNavigation => (0, 0),
        };
        ReaderRepaintBudgetEvaluation {
            scenario,
            passed: self.drawn_frames <= frame_budget && self.maximum_invalidations_per_frame <= invalidation_budget,
            frame_count: self.drawn_frames,
            frame_budget,
            maximum_invalidations_per_frame: self.maximum_invalidations_per_frame,
            invalidation_budget,
        }
    }
}

/// Request a reader repaint and retain a debug-only component/reason count.
pub(crate) fn notify<T: 'static>(cx: &mut Context<T>, component: Component, reason: &'static str) {
    #[cfg(debug_assertions)]
    {
        let counters = NOTIFICATION_COUNTS.get_or_init(|| Mutex::new(HashMap::new()));
        let mut counters = counters.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        *counters.entry((component, reason)).or_default() += 1;
    }
    cx.notify();
}

#[cfg(debug_assertions)]
static NOTIFICATION_COUNTS: OnceLock<Mutex<HashMap<(Component, &'static str), u64>>> = OnceLock::new();

#[cfg(debug_assertions)]
static RENDER_COUNTS: OnceLock<Mutex<HashMap<Component, u64>>> = OnceLock::new();

#[cfg(debug_assertions)]
static FRAME_COLLECTOR: OnceLock<Mutex<gpui::profiler::FrameTimingCollector>> = OnceLock::new();

#[cfg(debug_assertions)]
pub(crate) fn initialize_performance_measurements() {
    gpui::profiler::set_frame_trace_enabled(true);
    FRAME_COLLECTOR.get_or_init(|| Mutex::new(gpui::profiler::FrameTimingCollector::new()));
}

#[cfg(debug_assertions)]
pub(crate) fn record_render(component: Component) {
    let counters = RENDER_COUNTS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut counters = counters.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    *counters.entry(component).or_default() += 1;
}

#[cfg(not(debug_assertions))]
pub(crate) fn record_render(_component: Component) {}

#[cfg(debug_assertions)]
pub(crate) fn snapshot() -> Vec<((Component, &'static str), u64)> {
    let mut snapshot: Vec<((Component, &'static str), u64)> = NOTIFICATION_COUNTS.get().and_then(|counts| counts.lock().ok().map(|counts| counts.iter().map(|(key, count)| (*key, *count)).collect())).unwrap_or_default();
    snapshot.sort_unstable_by_key(|(key, _)| *key);
    snapshot
}

#[cfg(debug_assertions)]
pub(crate) fn performance_snapshot() -> ReaderPerformanceSnapshot {
    initialize_performance_measurements();
    let notifications = snapshot();
    let mut renders: Vec<(Component, u64)> = RENDER_COUNTS.get().and_then(|counts| counts.lock().ok().map(|counts| counts.iter().map(|(component, count)| (*component, *count)).collect())).unwrap_or_default();
    renders.sort_unstable_by_key(|(component, _)| *component);
    let frames = FRAME_COLLECTOR.get().and_then(|collector| collector.lock().ok().map(|mut collector| collector.collect_unseen())).unwrap_or_default();
    let mut result = ReaderPerformanceSnapshot { notifications, renders, drawn_frames: frames.len() as u64, ..Default::default() };
    for frame in frames {
        let draw_micros = frame.draw_duration().as_micros().min(u128::from(u64::MAX)) as u64;
        result.coalesced_invalidations = result.coalesced_invalidations.saturating_add(frame.invalidations);
        result.maximum_invalidations_per_frame = result.maximum_invalidations_per_frame.max(frame.invalidations);
        result.total_draw_micros = result.total_draw_micros.saturating_add(draw_micros);
        result.maximum_draw_micros = result.maximum_draw_micros.max(draw_micros);
    }
    result
}

#[cfg(debug_assertions)]
pub(crate) fn reset_performance_measurements() {
    initialize_performance_measurements();
    if let Some(counts) = NOTIFICATION_COUNTS.get() {
        counts.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clear();
    }
    if let Some(counts) = RENDER_COUNTS.get() {
        counts.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clear();
    }
    if let Some(collector) = FRAME_COLLECTOR.get() {
        *collector.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = gpui::profiler::FrameTimingCollector::new();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_turn_budget_rejects_an_extra_frame() {
        let within = ReaderPerformanceSnapshot { drawn_frames: 1, maximum_invalidations_per_frame: 4, ..Default::default() };
        assert!(within.evaluate_reader_repaint_budget(ReaderRepaintScenario::PageTurn).passed);

        let extra_frame = ReaderPerformanceSnapshot { drawn_frames: 2, maximum_invalidations_per_frame: 1, ..Default::default() };
        assert!(!extra_frame.evaluate_reader_repaint_budget(ReaderRepaintScenario::PageTurn).passed);
    }

    #[test]
    fn boundary_navigation_requires_no_frame() {
        let within = ReaderPerformanceSnapshot::default();
        assert!(within.evaluate_reader_repaint_budget(ReaderRepaintScenario::BoundaryNavigation).passed);

        let repainted = ReaderPerformanceSnapshot { drawn_frames: 1, maximum_invalidations_per_frame: 1, ..Default::default() };
        assert!(!repainted.evaluate_reader_repaint_budget(ReaderRepaintScenario::BoundaryNavigation).passed);
    }
}
