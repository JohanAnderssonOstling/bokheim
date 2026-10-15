use std::ops::Range;
use structure::parser_api::TokenUnit;
use structure::{PunctuationSpacingSide, SegmentTree, UnitKind};

pub(crate) const TRIM_PUNCTUATION: &[char] = &['.', ',', ';', ':', '!', '?', ')', ']', '}', '"', '\''];

pub fn is_trim_punctuation(character: char) -> bool { TRIM_PUNCTUATION.contains(&character) }

pub(crate) fn trim_trailing_punctuation(text: &str) -> &str { text.trim_end_matches(is_trim_punctuation) }

#[cfg(feature = "terminology")]
pub(crate) fn select_non_overlapping<T>(mut items: Vec<T>, span: impl Fn(&T) -> Range<usize>) -> Vec<T> {
    items.sort_by(|left, right| {
        let left = span(left);
        let right = span(right);
        left.start.cmp(&right.start).then_with(|| right.end.cmp(&left.end))
    });
    items.into_iter().fold(Vec::new(), |mut accepted, item| {
        if accepted.last().is_none_or(|previous| span(previous).end <= span(&item).start) {
            accepted.push(item);
        }
        accepted
    })
}

#[cfg(feature = "terminology")]
pub(crate) fn case_insensitive_match_spans(text: &str, term: &str) -> Vec<Range<usize>> {
    let folded_term = crate::unicode_case_fold(term);
    if folded_term.is_empty() {
        return Vec::new();
    }
    let folded_text = crate::text_normalization::FoldedText::new(text);
    folded_text.text().char_indices().filter_map(|(start, _)| {
        let end = start.checked_add(folded_term.len())?;
        folded_text.text().get(start..end).filter(|candidate| *candidate == folded_term).and_then(|_| folded_text.source_span(start..end))
    }).fold(Vec::<Range<usize>>::new(), |mut spans, span| {
        if spans.last() != Some(&span) {
            spans.push(span);
        }
        spans
    })
}

pub fn adjacent_whitespace_span(stream: &SegmentTree, subject_span: Range<usize>, side: PunctuationSpacingSide) -> Range<usize> {
    let boundary = match side {
        PunctuationSpacingSide::Before => subject_span.start,
        PunctuationSpacingSide::After => subject_span.end,
    };
    let units = structure::parser_api::units(stream);
    let adjacent = match side {
        PunctuationSpacingSide::Before => {
            let end = units.partition_point(|unit| unit.end() <= boundary);
            units[..end].iter().rev().find(|unit| !unit.is_tag())
        }
        PunctuationSpacingSide::After => {
            let start = units.partition_point(|unit| unit.start() < boundary);
            units[start..].iter().find(|unit| !unit.is_tag())
        }
    };
    adjacent.filter(|unit| unit.kind() == UnitKind::Whitespace).map(TokenUnit::span).unwrap_or(boundary..boundary)
}

pub(crate) fn unit_range_text(stream: &SegmentTree, start_idx: usize, end_idx: usize) -> Option<&str> {
    if end_idx <= start_idx {
        return None;
    }
    let start = structure::parser_api::unit_start(stream, start_idx)?;
    let end = structure::parser_api::unit_end(stream, end_idx - 1)?;
    stream.text().get(start..end)
}

pub(crate) fn contiguous_stream_end(stream: &SegmentTree, start_idx: usize) -> usize {
    contiguous_slice_end(structure::parser_api::units(stream), start_idx)
}

pub(crate) fn contiguous_slice_end(units: &[TokenUnit], start_idx: usize) -> usize {
    units.iter().enumerate().skip(start_idx).take_while(|(_, unit)| unit.kind() != UnitKind::Whitespace).fold(start_idx, |end_idx, (index, unit)| if unit.is_tag() { end_idx } else { index + 1 })
}

/// Counts of alphanumeric characters directly before and after `span`, i.e. how
/// much of the surrounding word a match leaves uncovered.
#[cfg(any(feature = "terminology", feature = "dictionary"))]
pub(crate) fn surrounding_word_lengths(text: &str, span: &Range<usize>) -> (usize, usize) {
    let prefix = text[..span.start].chars().rev().take_while(|character| character.is_alphanumeric()).count();
    let suffix = text[span.end..].chars().take_while(|character| character.is_alphanumeric()).count();
    (prefix, suffix)
}

#[cfg(any(feature = "terminology", feature = "dictionary"))]
pub(crate) fn is_whole_word_match(text: &str, span: &Range<usize>) -> bool { surrounding_word_lengths(text, span) == (0, 0) }

/// `span` widened to the alphanumeric run it sits in (`konto` in `kontor` → `kontor`).
#[cfg(feature = "dictionary")]
pub(crate) fn containing_word_span(text: &str, span: &Range<usize>) -> Range<usize> {
    let start = text[..span.start].char_indices().rev().take_while(|(_, character)| character.is_alphanumeric()).last().map_or(span.start, |(index, _)| index);
    let end = text[span.end..].char_indices().find(|(_, character)| !character.is_alphanumeric()).map_or(text.len(), |(index, _)| span.end + index);
    start..end
}

