//! Text matching and scope policy shared by every annotation reader.
use html_pipeline::engine::search_queries;
use book_model::{Annotation, RecurringScope};
use content_address::BookId;
use html_pipeline::engine::TextPatternMatch;
use html::search::TextPatternMatcher;
use html::text::TextSearchOptions;
use std::collections::HashMap;
use std::{cmp::Reverse, ops::Range};

/// Owned by the shared mark session. Page changes and note/formatting edits
/// reuse the compiled phrases; only phrase membership or text rebuilds it.
#[derive(Default)]
pub(crate) struct RecurringMatcher {
    inputs: Vec<(String, String)>,
    matcher: Option<TextPatternMatcher>,
}

impl RecurringMatcher {
    pub(crate) fn update<'a>(&mut self, annotations: impl Iterator<Item = &'a Annotation>) -> bool {
        let mut inputs = annotations.flat_map(|annotation| annotation.recurring_phrases().map(move |text| (annotation.id.as_str(), text))).collect::<Vec<_>>();
        inputs.sort_unstable();
        if inputs.iter().copied().eq(self.inputs.iter().map(|(id, text)| (id.as_str(), text.as_str()))) {
            return false;
        }
        self.inputs = inputs.into_iter().map(|(id, text)| (id.to_owned(), text.to_owned())).collect();
        self.matcher = if self.inputs.is_empty() {
            None
        } else {
            match TextPatternMatcher::new(self.inputs.iter().map(|(_, text)| text.split_whitespace().collect::<Vec<_>>().join(" ")), TextSearchOptions { match_case: false, whole_word: true, match_diacritics: true }) {
                Ok(matcher) => Some(matcher),
                Err(error) => {
                    log::error!("could not compile recurring annotation phrases: {error}");
                    None
                }
            }
        };
        true
    }

    pub(crate) fn patterns(&self) -> Option<&TextPatternMatcher> {
        self.matcher.as_ref()
    }

    pub(crate) fn annotation_id(&self, pattern: usize) -> Option<&str> {
        self.inputs.get(pattern).map(|(id, _)| id.as_str())
    }

    pub(crate) fn matches(&self, text: &str) -> Vec<TextPatternMatch> {
        self.matcher.as_ref().map_or_else(Vec::new, |matcher| {
            search_queries::text_pattern_matcher_find_matches(&matcher, text)
        })
    }

    pub(crate) fn matching_ids<'a>(&'a self, text: &str) -> impl Iterator<Item = &'a str> {
        self.matches(text).into_iter().map(|found| self.inputs[found.pattern].0.as_str())
    }

    pub(crate) fn matching_annotations<'a>(&'a self, text: &str, annotations: impl Iterator<Item = &'a Annotation>) -> impl Iterator<Item = (&'a Annotation, TextPatternMatch)> {
        let annotations = annotations.map(|annotation| (annotation.id.as_str(), annotation)).collect::<HashMap<_, _>>();
        self.matches(text).into_iter().filter_map(move |found| annotations.get(self.inputs[found.pattern].0.as_str()).map(|annotation| (*annotation, found)))
    }
}

/// Keep the longest overlapping phrase for each note, retaining other notes.
pub(crate) fn distinct_matches<'a, M>(matches: impl Iterator<Item = (&'a Annotation, M)>, range: impl Fn(&M) -> Range<usize>) -> Vec<(&'a Annotation, M)> {
    let mut matches = matches.collect::<Vec<_>>();
    matches.sort_by_key(|(_, found)| {
        let range = range(found);
        (range.start, Reverse(range.end))
    });
    let mut ends = HashMap::new();
    matches.retain(|(annotation, found)| {
        let range = range(found);
        if ends.get(annotation.id.as_str()).is_some_and(|end| *end > range.start) {
            return false;
        }
        ends.insert(annotation.id.as_str(), range.end);
        true
    });
    matches
}

/// Readers resolve their own chapter boundaries; scope decisions are shared.
pub(crate) fn applies_at(annotation: &Annotation, book: BookId, origin_chapter: Option<usize>, chapter: Option<usize>, same_document: bool) -> bool {
    match annotation.anchor.recurring_scope() {
        None => false,
        Some(RecurringScope::Library) => true,
        Some(RecurringScope::Book) => annotation.book_id == book,
        Some(RecurringScope::Chapter) => {
            annotation.book_id == book
                && match (origin_chapter, chapter) {
                    (Some(origin), Some(current)) => origin == current,
                    _ => same_document,
                }
        }
    }
}
