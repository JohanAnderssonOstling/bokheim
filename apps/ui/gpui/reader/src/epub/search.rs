//! Find-in-book state: the query, its options, and the bookkeeping that keeps
//! a slow whole-book scan from overwriting a newer one.
//!
//! Searching a whole EPUB runs off-thread and streams matches back in batches.
//! Every keystroke supersedes the scan in flight, so each attempt carries a
//! generation number: the worker checks it to stop early, and late results are
//! discarded when they no longer match. That bookkeeping is pure data, kept
//! here and tested directly.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use gpui::Task;
use html_view_core::{SearchOptions, SearchScope};

/// Which text a search treats as a match.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SearchFilters {
    pub(crate) match_case: bool,
    pub(crate) whole_word: bool,
    pub(crate) match_diacritics: bool,
    /// Whether to scan the whole publication or only the open document.
    pub(crate) whole_book: bool,
}

impl SearchFilters {
    fn renderer_options(self) -> SearchOptions {
        SearchOptions { match_case: self.match_case, whole_word: self.whole_word, match_diacritics: self.match_diacritics, scope: if self.whole_book { SearchScope::WholeBook } else { SearchScope::CurrentDocument } }
    }
}

/// The text input backing the search box lives on the view, not here — this
/// struct is the bookkeeping, and stays free of GPUI entities so it can be
/// exercised without an app.
pub(crate) struct SearchState {
    /// What the user has typed, which may not have been applied yet.
    query: String,
    /// The query the renderer is currently showing results for.
    applied_query: String,
    /// Incremented by every change that invalidates work in flight.
    generation: u64,
    /// The newest generation, readable from the search worker so it can stop
    /// as soon as its own attempt is superseded.
    cancellation: Arc<AtomicU64>,
    /// Debounce timer for the next apply.
    pub(crate) debounce: Option<Task<()>>,
    /// The streaming whole-book scan, dropped when a newer one starts.
    pub(crate) index_task: Option<Task<()>>,
    pub(crate) filters: SearchFilters,
    /// Position within the matches, as `(current, total)`.
    pub(crate) match_position: Option<(usize, usize)>,
    pub(crate) active: bool,
}

impl Default for SearchState {
    fn default() -> Self {
        Self {
            query: String::new(),
            applied_query: String::new(),
            generation: 0,
            cancellation: Arc::new(AtomicU64::new(0)),
            debounce: None,
            index_task: None,
            filters: SearchFilters { whole_book: true, ..SearchFilters::default() },
            match_position: None,
            active: false,
        }
    }
}

impl SearchState {
    #[cfg(test)]
    fn query(&self) -> &str {
        &self.query
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    /// A handle the search worker polls to learn that its attempt is stale.
    pub(crate) fn cancellation(&self) -> Arc<AtomicU64> {
        self.cancellation.clone()
    }

    /// Whether `generation` is still the attempt in progress.
    pub(crate) fn is_current(&self, generation: u64) -> bool {
        self.generation == generation
    }

    /// Whether results already on screen belong to `query` at `generation`.
    pub(crate) fn still_wants(&self, generation: u64, query: &str) -> bool {
        self.is_current(generation) && self.query == query
    }

    /// Starts a new attempt, superseding anything in flight.
    fn begin_attempt(&mut self) -> u64 {
        self.generation = self.generation.wrapping_add(1);
        self.cancellation.store(self.generation, Ordering::Release);
        self.generation
    }

    /// Records newly typed text and returns the generation to apply it under.
    pub(crate) fn set_query(&mut self, query: String) -> u64 {
        self.query = query;
        self.begin_attempt()
    }

    /// Changes the filters and forces the current query to be searched again,
    /// since different filters yield different matches for the same text.
    pub(crate) fn update_filters(&mut self, update: impl FnOnce(&mut SearchFilters)) {
        update(&mut self.filters);
        self.begin_attempt();
        self.applied_query.clear();
    }

    /// Claims the pending query if it differs from what is already applied.
    ///
    /// Returns `None` when the renderer is already showing this query, which
    /// is how a second Enter press falls through to "go to next match".
    pub(crate) fn take_pending(&mut self) -> Option<(String, SearchOptions)> {
        if self.applied_query == self.query {
            return None;
        }
        self.applied_query.clone_from(&self.query);
        Some((self.query.clone(), self.filters.renderer_options()))
    }

    /// Clears the search and cancels anything in flight.
    pub(crate) fn clear(&mut self) {
        self.begin_attempt();
        self.query.clear();
        self.applied_query.clear();
        self.match_position = None;
        self.active = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> SearchState {
        SearchState::default()
    }

    #[test]
    fn a_newer_query_supersedes_the_one_in_flight() {
        let mut search = state();
        let first = search.set_query("plough".to_owned());
        let second = search.set_query("ploughman".to_owned());

        assert_ne!(first, second);
        assert!(!search.is_current(first), "the earlier attempt must be recognized as stale");
        assert!(search.is_current(second));
        assert_eq!(search.cancellation().load(Ordering::Acquire), second, "the worker must see the newest generation");
    }

    #[test]
    fn applying_the_same_query_twice_yields_nothing_the_second_time() {
        let mut search = state();
        search.set_query("plough".to_owned());

        assert!(search.take_pending().is_some());
        assert!(search.take_pending().is_none(), "an unchanged query must fall through to match navigation");
    }

    #[test]
    fn changing_a_filter_forces_the_same_query_to_be_searched_again() {
        let mut search = state();
        search.set_query("plough".to_owned());
        search.take_pending().expect("first apply");

        search.update_filters(|filters| filters.match_case = true);

        let (query, options) = search.take_pending().expect("a filter change must re-apply the query");
        assert_eq!(query, "plough");
        assert!(options.match_case);
    }

    #[test]
    fn scope_selects_the_matching_renderer_option() {
        let whole_book = SearchFilters { whole_book: true, ..SearchFilters::default() };
        assert_eq!(whole_book.renderer_options().scope, SearchScope::WholeBook);

        let section = SearchFilters { whole_book: false, ..SearchFilters::default() };
        assert_eq!(section.renderer_options().scope, SearchScope::CurrentDocument);
    }

    #[test]
    fn clearing_cancels_the_scan_and_drops_results() {
        let mut search = state();
        search.set_query("plough".to_owned());
        search.active = true;
        search.match_position = Some((2, 9));
        let before = search.generation();

        search.clear();

        assert!(search.query().is_empty());
        assert!(!search.active);
        assert_eq!(search.match_position, None);
        assert!(!search.is_current(before), "an in-flight scan must not survive a clear");
    }

    #[test]
    fn results_belong_only_to_the_query_that_asked_for_them() {
        let mut search = state();
        let stale = search.set_query("plough".to_owned());
        search.set_query("ploughman".to_owned());

        assert!(!search.still_wants(stale, "plough"), "results for a superseded query must be discarded");
        assert!(search.still_wants(search.generation(), "ploughman"));
    }
}
