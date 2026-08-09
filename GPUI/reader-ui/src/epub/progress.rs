//! How far through the book the reader is.
//!
//! The renderer re-reports progress on every relayout, usually with identical
//! numbers. Folding that here keeps repaints and database writes tied to real
//! movement rather than to layout churn.

#[derive(Debug, PartialEq)]
pub(crate) struct ProgressState {
    /// Fraction of the whole book read, in `0.0..=1.0`.
    pub(crate) fraction: f32,
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
        Self { fraction: 0.0, location: 1, total_locations: 1, section: 1, section_count: 1 }
    }
}

impl ProgressState {
    /// Records a progress report, returning whether it actually moved.
    ///
    /// `doc` is the renderer's zero-based document index; sections are shown
    /// one-based.
    pub(crate) fn update(&mut self, fraction: f32, location: u64, total_locations: u64, doc: usize, doc_count: usize) -> bool {
        let next = Self { fraction, location, total_locations, section: doc + 1, section_count: doc_count };
        if *self == next {
            return false;
        }
        *self = next;
        true
    }

    pub(crate) fn at_start(&self) -> bool {
        self.location <= 1
    }

    pub(crate) fn at_end(&self) -> bool {
        self.location >= self.total_locations
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_repeated_report_is_not_movement() {
        let mut progress = ProgressState::default();
        assert!(progress.update(0.25, 40, 160, 2, 12), "the first report moves off the default");
        assert!(!progress.update(0.25, 40, 160, 2, 12), "an identical report must not repaint or persist");
    }

    #[test]
    fn any_changed_component_counts_as_movement() {
        for (fraction, location, total, doc, count) in [(0.26, 40, 160, 2, 12), (0.25, 41, 160, 2, 12), (0.25, 40, 161, 2, 12), (0.25, 40, 160, 3, 12), (0.25, 40, 160, 2, 13)] {
            let mut progress = ProgressState::default();
            progress.update(0.25, 40, 160, 2, 12);
            assert!(progress.update(fraction, location, total, doc, count), "a change in any component must be reported");
        }
    }

    #[test]
    fn sections_are_reported_one_based() {
        let mut progress = ProgressState::default();
        progress.update(0.0, 1, 160, 0, 12);
        assert_eq!(progress.section, 1, "the renderer's document 0 is section 1 to the reader");
    }

    #[test]
    fn the_boundaries_gate_page_turns() {
        let mut progress = ProgressState::default();
        assert!(progress.at_start(), "a freshly opened book is at the start");

        progress.update(0.0, 1, 160, 0, 12);
        assert!(progress.at_start());
        assert!(!progress.at_end());

        progress.update(1.0, 160, 160, 11, 12);
        assert!(!progress.at_start());
        assert!(progress.at_end());
    }

    #[test]
    fn a_single_location_book_is_at_both_boundaries() {
        let progress = ProgressState::default();
        assert!(progress.at_start());
        assert!(progress.at_end(), "a one-location book must not offer a next page");
    }
}
