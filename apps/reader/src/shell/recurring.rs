//! Text matching and scope policy shared by every annotation reader.
use book_model::{Annotation, RecurringScope};
use content_address::BookId;
use html::text::{TextPatternMatch, TextPatternMatcher, TextSearchOptions};
use std::collections::HashMap;

/// Owned by the shared mark session. Page changes and note/formatting edits
/// reuse the compiled phrases; only phrase membership or text rebuilds it.
#[derive(Default)]
pub(crate) struct RecurringMatcher {
    inputs: Vec<(String, String)>,
    matcher: Option<TextPatternMatcher>,
}

impl RecurringMatcher {
    pub(crate) fn update<'a>(&mut self, annotations: impl Iterator<Item = &'a Annotation>) -> bool {
        let mut inputs = annotations.map(|annotation| (annotation.id.as_str(), annotation.exact_text.as_str())).collect::<Vec<_>>();
        inputs.sort_unstable_by_key(|(id, _)| *id);
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

    pub(crate) fn matches(&self, text: &str) -> Vec<TextPatternMatch> {
        self.matcher.as_ref().map_or_else(Vec::new, |matcher| matcher.find_matches(text))
    }

    pub(crate) fn matching_ids<'a>(&'a self, text: &str) -> impl Iterator<Item = &'a str> {
        self.matches(text).into_iter().map(|found| self.inputs[found.pattern].0.as_str())
    }

    pub(crate) fn matching_annotations<'a>(&'a self, text: &str, annotations: impl Iterator<Item = &'a Annotation>) -> impl Iterator<Item = (&'a Annotation, TextPatternMatch)> {
        let annotations = annotations.map(|annotation| (annotation.id.as_str(), annotation)).collect::<HashMap<_, _>>();
        self.matches(text).into_iter().filter_map(move |found| annotations.get(self.inputs[found.pattern].0.as_str()).map(|annotation| (*annotation, found)))
    }
}

/// Readers resolve their own chapter boundaries; scope decisions are shared.
pub(crate) fn applies_at(annotation: &Annotation, book: BookId, origin_chapter: Option<usize>, chapter: Option<usize>, same_document: bool) -> bool {
    match annotation.anchor.recurring_scope() {
        None => false,
        Some(RecurringScope::Library) => true,
        Some(RecurringScope::Book) => annotation.content_hash == book,
        Some(RecurringScope::Chapter) => {
            annotation.content_hash == book
                && match (origin_chapter, chapter) {
                    (Some(origin), Some(current)) => origin == current,
                    _ => same_document,
                }
        }
    }
}
