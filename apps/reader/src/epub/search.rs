//! Find-in-book state and the request number that keeps an obsolete whole-book
//! scan from overwriting a newer query.
//!
//! Every keystroke supersedes the scan in flight. The worker checks the shared
//! request number to stop early, and the UI checks it again before displaying
//! the completed result set.

use ui_text::text;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use gpui::{ListAlignment, ListState, px};
use skrift_column_paginator::{BookSearchMatch, BookSearchTarget, SearchOptions, SearchScope};

use crate::epub::SearchState;
use crate::epub::toc::TocState;

const MIN_SEARCH_CHARACTERS: usize = 3;

#[derive(PartialEq, Eq)]
pub(super) enum SearchRow {
    Heading(usize),
    Match(usize),
}

impl Default for SearchState {
    fn default() -> Self {
        // Keep Kobo search bounded to the loaded EPUB document instead of
        // repeatedly scanning the publication on its single CPU core.
        let scope = if cfg!(feature = "kobo") { SearchScope::CurrentDocument } else { SearchScope::WholeBook };
        Self {
            query: None,
            pending: false,
            cancellation: Arc::new(AtomicU64::new(0)),
            options: SearchOptions { match_case: false, whole_word: false, match_diacritics: false, scope },
            match_position: None,
            results: Default::default(),
            rows: Vec::new(),
            contents: Default::default(),
            list: ListState::new(0, ListAlignment::Top, px(100.0)),
        }
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
        self.contents.set_query(&query);
        *self.query.as_mut()? = query;
        self.pending = true;
        Some(self.begin_attempt())
    }

    /// Claims the pending query, clearing renderer results for input shorter
    /// than three non-whitespace characters, including when Enter is pressed.
    ///
    /// Returns `None` when the renderer is already showing this query, which
    /// is how a second Enter press falls through to "go to next match".
    pub(in crate::epub) fn take_pending(&mut self) -> Option<(String, SearchOptions)> {
        let query = self.query.as_ref()?;
        if !self.pending {
            return None;
        }
        self.pending = false;
        let query = if query.chars().filter(|character| !character.is_whitespace()).take(MIN_SEARCH_CHARACTERS).count() >= MIN_SEARCH_CHARACTERS { query.clone() } else { String::new() };
        Some((query, self.options))
    }

    pub(in crate::epub) fn clear_results(&mut self) {
        self.results.clear();
        self.rows.clear();
        self.contents = Default::default();
        self.list.reset(0);
    }

    /// Group once when results or TOC anchors change, rather than parsing every
    /// match's CFI and rebuilding every heading on each sidebar repaint.
    pub(in crate::epub) fn rebuild_rows(&mut self, toc: &TocState) {
        self.contents.tree = toc.annotation_tree(self.results.iter().map(|result| match &result.target {
            BookSearchTarget::SourceCfi(cfi) => toc.ordinal_for_cfi(cfi),
            BookSearchTarget::LocalOccurrence(_) => toc.ordinal_for_document(result.doc),
        }));
        // Publications without an outline still expose matches by document.
        if self.contents.tree.branches.is_empty() {
            self.contents.tree.other.clear();
            self.contents.tree.branches = search_groups(toc, self.results.iter()).into_iter().map(|(_, document, indices)| {
                crate::shell::annotation_tree::AnnotationBranch { id: document, title: format!(text!(reader::section), document + 1), annotations: indices, children: Vec::new() }
            }).collect();
        }
        self.refresh_rows();
    }

    pub(in crate::epub) fn refresh_rows(&mut self) {
        let rows = self.contents.rows().into_iter().map(|(branch, index)| if branch { SearchRow::Heading(index) } else { SearchRow::Match(index) }).collect();
        crate::shell::sidebar_list::replace_rows(&self.list, &mut self.rows, rows);
    }

    /// Clears the query when the renderer explicitly deactivates search.
    /// Hiding the sidebar does not deactivate search.
    pub(in crate::epub) fn clear(&mut self) {
        self.begin_attempt();
        self.query = None;
        self.pending = false;
        self.match_position = None;
        self.clear_results();
    }
}

/// Preserve reading order even when some spine documents lack TOC entries.
pub(in crate::epub) fn search_groups<'a>(
    toc: &TocState,
    results: impl IntoIterator<Item = &'a BookSearchMatch>,
) -> Vec<(Option<usize>, usize, Vec<usize>)> {
    let chapters = toc.chapter_ordinals();
    let mut groups: Vec<(Option<usize>, usize, Vec<usize>)> = Vec::new();
    let mut group_indices = HashMap::new();
    for (index, result) in results.into_iter().enumerate() {
        let ordinal = match &result.target {
            BookSearchTarget::SourceCfi(cfi) => toc.ordinal_for_cfi(cfi),
            BookSearchTarget::LocalOccurrence(_) => toc.ordinal_for_document(result.doc),
        };
        let chapter = ordinal.and_then(|ordinal| chapters.iter().rev().find(|&&chapter| chapter <= ordinal).copied());
        let key = (chapter, if chapter.is_some() { 0 } else { result.doc });
        let group = *group_indices.entry(key).or_insert_with(|| {
            groups.push((chapter, result.doc, Vec::new()));
            groups.len() - 1
        });
        groups[group].2.push(index);
    }
    groups
}
