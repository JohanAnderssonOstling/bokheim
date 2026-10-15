//! Unicode normalization required by locale and measurement recognition.
use icu_casemap::{CaseMapper, CaseMapperBorrowed};
use unicode_normalization::UnicodeNormalization;
#[cfg(feature = "terminology")]
use std::ops::Range;

thread_local! {
    pub(crate) static CASE_MAPPER: CaseMapperBorrowed<'static> = CaseMapper::new();
}

/// Build a stable user-facing search key.
///
/// Compatibility composition makes canonically equivalent and compatibility
/// spellings searchable together, while retaining language-significant
/// diacritics such as Swedish å, ä, and ö.
pub fn canonical_search_text(text: &str) -> String {
    let compatible = text.nfkc().collect::<String>();
    let folded = unicode_case_fold(&compatible);
    collapse_whitespace(folded.nfkc(), folded.len())
}

pub fn unicode_case_fold(text: &str) -> String { CASE_MAPPER.with(|case_mapper| case_mapper.fold_string(text).into_owned()) }

fn collapse_whitespace(characters: impl Iterator<Item = char>, capacity: usize) -> String {
    characters.fold((String::with_capacity(capacity), false), |(mut normalized, pending_space), character| {
        if character.is_whitespace() {
            let has_content = !normalized.is_empty();
            return (normalized, has_content);
        }
        if pending_space {
            normalized.push(' ');
        }
        normalized.push(character);
        (normalized, false)
    }).0
}

#[cfg(feature = "terminology")]
pub(crate) struct FoldedText {
    text: String,
    source_starts: Vec<usize>,
    source_ends: Vec<usize>,
}

#[cfg(feature = "terminology")]
impl FoldedText {
    pub(crate) fn new(text: &str) -> Self {
        CASE_MAPPER.with(|case_mapper| {
            let mut folded_text = String::with_capacity(text.len());
            let mut source_starts = Vec::with_capacity(text.len());
            let mut source_ends = Vec::with_capacity(text.len());
            text.char_indices().for_each(|(source_start, character)| {
                let source_end = source_start + character.len_utf8();
                let mut buffer = [0; 4];
                let folded = case_mapper.fold_string(character.encode_utf8(&mut buffer));
                folded_text.push_str(&folded);
                (0..folded.len()).for_each(|_| {
                    source_starts.push(source_start);
                    source_ends.push(source_end);
                });
            });
            Self { text: folded_text, source_starts, source_ends }
        })
    }
    pub(crate) fn text(&self) -> &str { &self.text }
    pub(crate) fn source_span(&self, folded_span: Range<usize>) -> Option<Range<usize>> {
        let start = *self.source_starts.get(folded_span.start)?;
        let end = *self.source_ends.get(folded_span.end.checked_sub(1)?)?;
        Some(start..end)
    }
}

