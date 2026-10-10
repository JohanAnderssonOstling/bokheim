//! How far through the book the reader is.
//!
//! The renderer re-reports progress on every relayout, usually with identical
//! numbers. Folding that here keeps repaints and database writes tied to real
//! movement rather than to layout churn.

use crate::epub::{ProgressState, ProgressSummary};

impl Default for ProgressState {
    fn default() -> Self {
        // A book always has at least one location and one section, and
        // positions are one-based, so zero is never a valid starting value.
        Self { fraction: 0.0, doc_fraction: 0.0, location: 1, total_locations: 1, section: 1, section_count: 1 }
    }
}

impl Default for ProgressSummary {
    fn default() -> Self {
        ProgressState::default().summary()
    }
}

impl ProgressSummary {
    /// What the toolbar shows: the one number worth a permanent place.
    pub(crate) fn label(self) -> String {
        format!("{}%", self.percent)
    }

    /// Progress is deliberately presented only as a percentage. ZIP byte
    /// weights are useful for relative progress, not as synthetic page or
    /// location numbers.
    pub(crate) fn tooltip(self) -> String {
        format!("{}% through book", self.percent)
    }
}

impl ProgressState {
    pub(in crate::epub) fn summary(&self) -> ProgressSummary {
        // Clamped rather than trusted: the renderer reports a fraction derived
        // from layout, and a rounding overshoot must not print "101%".
        let percent = (self.fraction.clamp(0.0, 1.0) * 100.0).round() as u8;
        ProgressSummary { percent, location: self.location, total_locations: self.total_locations, section: self.section, section_count: self.section_count }
    }

    /// Records a progress report, returning whether it actually moved.
    ///
    /// `doc` is the renderer's zero-based document index; sections are shown
    /// one-based.
    pub(in crate::epub) fn update(&mut self, fraction: f32, doc_fraction: f32, location: u64, total_locations: u64, doc: usize, doc_count: usize) -> bool {
        let next = Self { fraction, doc_fraction, location, total_locations, section: doc + 1, section_count: doc_count };
        if *self == next {
            return false;
        }
        *self = next;
        true
    }
}
