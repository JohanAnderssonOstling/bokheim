use std::ops::Range;

use serde::{Deserialize, Serialize};

use super::{FormattingKind, TagAnchor, TagData};
mod view;
pub use view::UnitRef;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TokenUnitId(u64);

impl TokenUnitId {
    pub fn new(value: u64) -> Self { Self(value) }
    pub fn value(self) -> u64 { self.0 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SegmentTreeFingerprint(u64);

impl SegmentTreeFingerprint {
    pub(super) fn new(value: u64) -> Self { Self(value) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SemanticUnitId(u64);

impl SemanticUnitId {
    #[cfg(test)]
    pub(crate) fn new(value: u64) -> Self { Self(value) }
    pub(super) fn from_raw(value: u64) -> Self { Self(value) }
    pub fn value(self) -> u64 { self.0 }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Run {
    text: String,
    tags: Vec<TagAnchor>,
    inherited_formatting: Vec<FormattingKind>,
}

impl Run {
    pub fn new(text: impl Into<String>) -> Self { Self { text: text.into(), tags: Vec::new(), inherited_formatting: Vec::new() } }
    pub fn empty() -> Self { Self::default() }
    pub fn tag(tag: TagData) -> Self { let mut run = Self::default(); run.push_tag(tag); run }
    pub fn text(&self) -> &str { &self.text }
    pub fn tags(&self) -> &[TagAnchor] { &self.tags }
    pub fn inherited_formatting(&self) -> &[FormattingKind] { &self.inherited_formatting }
    pub fn into_parts(self) -> (String, Vec<TagAnchor>, Vec<FormattingKind>) { (self.text, self.tags, self.inherited_formatting) }
    pub fn push_text(&mut self, text: impl AsRef<str>) { self.text.push_str(text.as_ref()); }
    pub fn push_tag(&mut self, tag: TagData) { self.push_tag_at(self.text.len(), tag); }
    pub fn push_tag_at(&mut self, offset: usize, tag: TagData) { self.tags.push(TagAnchor::new(offset, tag)); }
    pub fn extend_inherited_formatting(&mut self, kinds: impl IntoIterator<Item = FormattingKind>) { self.inherited_formatting.extend(kinds); }
    pub fn append(&mut self, other: Run) { let base = self.text.len(); self.text.push_str(&other.text); self.tags.extend(other.tags.into_iter().map(|anchor| { let (offset, tag) = anchor.into_parts(); TagAnchor::new(base + offset, tag) })); self.inherited_formatting.extend(other.inherited_formatting); }
}

/// Compact normalized content words derived from a tokenized text value.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TokenizedContentWords {
    normalized: String,
    spans: Vec<Range<u16>>,
}

impl TokenizedContentWords {
    pub fn from_normalized_content_units(units: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        let (normalized, spans) = units.into_iter().fold((String::new(), Vec::new()), |(mut normalized, mut spans), unit| {
            let start = normalized.len();
            normalized.push_str(unit.as_ref());
            spans.push(Range { start: u16::try_from(start).expect("tokenized content exceeds u16 range"), end: u16::try_from(normalized.len()).expect("tokenized content exceeds u16 range") });
            (normalized, spans)
        });
        Self { normalized, spans }
    }

    pub fn len(&self) -> usize { self.spans.len() }
    pub fn is_empty(&self) -> bool { self.spans.is_empty() }
    pub fn word(&self, index: usize) -> Option<&str> { self.spans.get(index).and_then(|span| self.normalized.get(span.start as usize..span.end as usize)) }
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &str> + DoubleEndedIterator + '_ { self.spans.iter().map(|span| &self.normalized[span.start as usize..span.end as usize]) }
    pub fn slice(&self, range: Range<usize>) -> Option<Self> {
        if range.start > range.end || range.end > self.len() { return None; }
        let byte_start = self.spans.get(range.start).map(|span| span.start).unwrap_or_else(|| u16::try_from(self.normalized.len()).expect("tokenized content exceeds u16 range"));
        let byte_end = self.spans.get(range.end).map(|span| span.start).unwrap_or_else(|| u16::try_from(self.normalized.len()).expect("tokenized content exceeds u16 range"));
        let normalized = self.normalized.get(byte_start as usize..byte_end as usize)?.to_string();
        let spans = self.spans[range].iter().map(|span| span.start - byte_start..span.end - byte_start).collect();
        Some(Self { normalized, spans })
    }
    pub fn matching_starts<'a>(&'a self, candidate: &'a Self) -> impl Iterator<Item = usize> + 'a {
        let candidate_fits = !candidate.is_empty() && candidate.len() <= self.len();
        (0..=self.len().saturating_sub(candidate.len())).filter(move |start| candidate_fits && candidate.iter().enumerate().all(|(offset, word)| self.word(start + offset) == Some(word)))
    }
    pub fn occurrence_count(&self, candidate: &Self) -> usize { self.matching_starts(candidate).count() }
}

pub(super) fn encode_optional_index(index: Option<u16>) -> u16 { index.map(|value| value.checked_add(1).expect("index exceeds compact optional range")).unwrap_or(0) }
pub(super) fn decode_optional_index(encoded: u16) -> Option<u16> { encoded.checked_sub(1) }
