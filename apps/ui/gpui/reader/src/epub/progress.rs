//! How far through the book the reader is.
//!
//! The renderer re-reports progress on every relayout, usually with identical
//! numbers. Folding that here keeps repaints and database writes tied to real
//! movement rather than to layout churn.

#[derive(Debug, PartialEq)]
pub(crate) struct ProgressState {
    /// Fraction of the whole book read, in `0.0..=1.0`.
    pub(crate) fraction: f32,
    /// Fraction of the current section read, in `0.0..=1.0`.
    pub(crate) doc_fraction: f32,
    /// One-based position within the book's synthetic locations.
    pub(crate) location: u64,
    pub(crate) total_locations: u64,
    /// One-based index of the current document.
    pub(crate) section: usize,
    pub(crate) section_count: usize,
}

impl Default for ProgressState {
    fn default() -> Self {
        // A book always has at least one location and one section, and
        // positions are one-based, so zero is never a valid starting value.
        Self { fraction: 0.0, doc_fraction: 0.0, location: 1, total_locations: 1, section: 1, section_count: 1 }
    }
}

/// The progress numbers in the form the toolbar draws them.
///
/// A `Copy` summary rather than a borrow of [`ProgressState`] because the
/// toolbar model is compared wholesale to decide whether to repaint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ProgressSummary {
    /// Whole percent through the book, `0..=100`.
    pub(crate) percent: u8,
    pub(crate) location: u64,
    pub(crate) total_locations: u64,
    pub(crate) section: usize,
    pub(crate) section_count: usize,
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

    /// The detail behind it, which does not earn toolbar width.
    pub(crate) fn tooltip(self) -> String {
        format!("{}% · location {} of {} · section {} of {}", self.percent, self.location, self.total_locations, self.section, self.section_count)
    }
}

impl ProgressState {
    pub(crate) fn summary(&self) -> ProgressSummary {
        // Clamped rather than trusted: the renderer reports a fraction derived
        // from layout, and a rounding overshoot must not print "101%".
        let percent = (self.fraction.clamp(0.0, 1.0) * 100.0).round() as u8;
        ProgressSummary { percent, location: self.location, total_locations: self.total_locations, section: self.section, section_count: self.section_count }
    }

    /// Records a progress report, returning whether it actually moved.
    ///
    /// `doc` is the renderer's zero-based document index; sections are shown
    /// one-based.
    pub(crate) fn update(&mut self, fraction: f32, doc_fraction: f32, location: u64, total_locations: u64, doc: usize, doc_count: usize) -> bool {
        let next = Self { fraction, doc_fraction, location, total_locations, section: doc + 1, section_count: doc_count };
        if *self == next {
            return false;
        }
        *self = next;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_repeated_report_is_not_movement() {
        let mut progress = ProgressState::default();
        assert!(progress.update(0.25, 0.1, 40, 160, 2, 12), "the first report moves off the default");
        assert!(!progress.update(0.25, 0.1, 40, 160, 2, 12), "an identical report must not repaint or persist");
    }

    #[test]
    fn any_changed_component_counts_as_movement() {
        for (fraction, doc_fraction, location, total, doc, count) in
            [(0.26, 0.1, 40, 160, 2, 12), (0.25, 0.1, 41, 160, 2, 12), (0.25, 0.1, 40, 161, 2, 12), (0.25, 0.1, 40, 160, 3, 12), (0.25, 0.1, 40, 160, 2, 13), (0.25, 0.2, 40, 160, 2, 12)]
        {
            let mut progress = ProgressState::default();
            progress.update(0.25, 0.1, 40, 160, 2, 12);
            assert!(progress.update(fraction, doc_fraction, location, total, doc, count), "a change in any component must be reported");
        }
    }

    #[test]
    fn the_summary_rounds_to_whole_percent_and_cannot_exceed_one_hundred() {
        let mut progress = ProgressState::default();
        progress.update(0.4449, 0.1, 40, 160, 2, 12);
        assert_eq!(progress.summary().percent, 44);
        progress.update(1.004, 0.2, 160, 160, 11, 12);
        assert_eq!(progress.summary().percent, 100, "a rounding overshoot must not print 101%");
    }

    #[test]
    fn the_toolbar_label_is_just_the_percentage() {
        let mut progress = ProgressState::default();
        progress.update(0.42, 0.1, 40, 160, 2, 12);
        assert_eq!(progress.summary().label(), "42%");
        assert_eq!(progress.summary().tooltip(), "42% · location 40 of 160 · section 3 of 12");
    }

    #[test]
    fn sections_are_reported_one_based() {
        let mut progress = ProgressState::default();
        progress.update(0.0, 0.1, 1, 160, 0, 12);
        assert_eq!(progress.section, 1, "the renderer's document 0 is section 1 to the reader");
    }
}
