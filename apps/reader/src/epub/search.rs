//! Find-in-book state and the request number that keeps an obsolete whole-book
//! scan from overwriting a newer query.
//!
//! Every keystroke supersedes the scan in flight. The worker checks the shared
//! request number to stop early, and the UI checks it again before displaying
//! the completed result set.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use html_view_core::{SearchOptions, SearchScope};

use crate::epub::SearchState;

impl Default for SearchState {
    fn default() -> Self {
        Self { query: None, pending: false, cancellation: Arc::new(AtomicU64::new(0)), options: SearchOptions { match_case: false, whole_word: false, match_diacritics: false, scope: SearchScope::WholeBook }, match_position: None }
    }
}

impl SearchState {
    /// The request currently allowed to produce results.
    pub(in crate::epub) fn current_request(&self) -> u64 {
        self.cancellation.load(Ordering::Acquire)
    }

    pub(in crate::epub) fn is_current(&self, request: u64) -> bool {
        self.current_request() == request
    }

    /// Whether completed results still belong to the active query.
    pub(in crate::epub) fn still_wants(&self, request: u64, query: &str) -> bool {
        self.is_current(request) && self.query.as_deref() == Some(query)
    }

    /// Starts a new attempt, superseding anything in flight.
    fn begin_attempt(&self) -> u64 {
        self.cancellation.fetch_add(1, Ordering::AcqRel).wrapping_add(1)
    }

    /// Records newly typed text and returns its request number.
    pub(in crate::epub) fn set_query(&mut self, query: String) -> Option<u64> {
        *self.query.as_mut()? = query;
        self.pending = true;
        Some(self.begin_attempt())
    }

    /// Claims the pending query.
    ///
    /// Returns `None` when the renderer is already showing this query, which
    /// is how a second Enter press falls through to "go to next match".
    pub(in crate::epub) fn take_pending(&mut self) -> Option<(String, SearchOptions)> {
        let query = self.query.as_ref()?;
        if !self.pending {
            return None;
        }
        self.pending = false;
        Some((query.clone(), self.options))
    }

    /// Clears the search and cancels anything in flight.
    pub(in crate::epub) fn clear(&mut self) {
        self.begin_attempt();
        self.query = None;
        self.pending = false;
        self.match_position = None;
    }
}
