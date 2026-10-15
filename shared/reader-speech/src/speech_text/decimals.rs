//! Exact number wording from recognized numbers, measurements, and ranges.
use std::ops::Range;

use segment_parser::{SegmentTree, currency_facts, measurement_facts, number_facts};

use super::numbers::{cardinal, integer_words, numeric_token_boundary};
use super::semantics::Replacement;

pub(super) fn number_token_end(text: &str) -> Option<usize> {
    let sign = text.chars().next().filter(|character| matches!(character, '+' | '-' | '−')).map_or(0, char::len_utf8);
    let integer = text[sign..].bytes().take_while(u8::is_ascii_digit).count();
    if integer == 0 {
        return None;
    }
    let end = sign + integer;
    if text[end..].starts_with('.') {
        let fraction = text[end + 1..].bytes().take_while(u8::is_ascii_digit).count();
        Some(if fraction > 0 { end + 1 + fraction } else { end })
    } else {
        Some(end)
    }
}

pub(super) fn source_number_phrase(text: &str, decimal: Option<char>, grouping: Option<char>) -> Option<String> {
    number_phrase(text, decimal, grouping, false)
}

fn number_phrase(text: &str, decimal: Option<char>, grouping: Option<char>, spell_integer: bool) -> Option<String> {
    // Keep the original integer digits and spell each fractional digit. Do not
    // pass through a numeric value: trailing zeroes can express precision.
    if decimal.is_some_and(|decimal| decimal != '.') {
        return None;
    }
    let first = text.chars().next()?;
    let (sign, unsigned) = match first {
        '-' | '−' => ("minus ", &text[first.len_utf8()..]),
        '+' => ("plus ", &text[1..]),
        _ => ("", text),
    };
    let (integer, fraction) = if decimal.is_some() { unsigned.split_once('.').map(|(integer, fraction)| (integer, Some(fraction)))? } else { (unsigned, None) };
    if !integer.chars().all(|character| character.is_ascii_digit() || Some(character) == grouping) || fraction.is_some_and(|fraction| fraction.is_empty() || !fraction.bytes().all(|digit| digit.is_ascii_digit())) {
        return None;
    }
    let integer = integer.chars().filter(char::is_ascii_digit).collect::<String>();
    if integer.is_empty() && fraction.is_none() {
        return None;
    }
    let integer = if integer.is_empty() { "0" } else { &integer };
    let integer = if spell_integer { integer_words(integer)? } else { integer.to_string() };
    match fraction {
        Some(fraction) => {
            let fraction = fraction.bytes().map(|digit| cardinal(u32::from(digit - b'0'))).collect::<Vec<_>>().join(" ");
            Some(format!("{sign}{integer} point {fraction}"))
        }
        None => Some(format!("{sign}{integer}")),
    }
}

fn needs_wording(text: &str, decimal: Option<char>) -> bool {
    decimal.is_some() || text.starts_with(['-', '−', '+'])
}

pub(super) fn numeric_replacements(tree: &SegmentTree) -> impl Iterator<Item = Replacement> + '_ {
    let numbers = number_facts(tree).filter_map(|fact| {
        if fact.number_format_requires_locale_change() || !needs_wording(fact.text(), fact.decimal_separator()) {
            return None;
        }
        let phrase = source_number_phrase(fact.text(), fact.decimal_separator(), fact.grouping_separator())?;
        Some(Replacement::new(fact.span(), phrase))
    });
    let measurements = measurement_facts(tree).flat_map(|fact| {
        let measurement = fact.measurement_fact();
        if fact.number_format_requires_locale_change() {
            return Vec::new();
        }
        let start = fact.span().start;
        let end = fact.measurement_separator_span().start;
        let spans = match (measurement.number_values().len(), measurement.number_range_typography()) {
            (1, _) => vec![start..end],
            (2, Some(range)) => vec![start..start + range.separator_region_start(), start + range.separator_region_end()..end],
            _ => return Vec::new(),
        };
        spans
            .into_iter()
            .zip(measurement.number_formats())
            .filter_map(|(span, format)| {
                let text = &tree.text()[span.clone()];
                if !needs_wording(text, format.decimal_separator()) {
                    return None;
                }
                let phrase = source_number_phrase(text, format.decimal_separator(), format.grouping_separator())?;
                Some(Replacement::new(span, phrase))
            })
            .collect::<Vec<_>>()
    });
    numbers.chain(measurements)
}

/// Final wording runs after calendar, currency, notation, and abbreviation
/// expansion, so numeric references still provide context to those rules.
pub(super) fn number_word_replacements(tree: &SegmentTree) -> Vec<Replacement> {
    let text = tree.text();
    let mut replacements = Vec::new();
    let mut add = |span: Range<usize>, decimal, grouping| {
        if !numeric_token_boundary(text, span.start, span.end) || text[span.end..].strip_prefix('.').is_some_and(|tail| tail.starts_with(char::is_alphanumeric)) {
            return;
        }
        // Keep a mixed slash fraction intact when the earlier unit-based
        // fraction rule found insufficient context to expand it.
        if let Some(tail) = text[span.end..].strip_prefix([' ', '\u{00a0}', '\u{202f}']) {
            let digits = tail.bytes().take_while(u8::is_ascii_digit).count();
            if digits > 0 && tail[digits..].starts_with('/') {
                return;
            }
        }
        if let Some(phrase) = number_phrase(&text[span.clone()], decimal, grouping, true) {
            replacements.push(Replacement::new(span, phrase));
        }
    };
    for fact in number_facts(tree).filter(|fact| !fact.number_format_requires_locale_change()) {
        add(fact.span(), fact.decimal_separator(), fact.grouping_separator());
    }
    for fact in measurement_facts(tree).filter(|fact| !fact.number_format_requires_locale_change()) {
        let measurement = fact.measurement_fact();
        let start = fact.span().start;
        let end = fact.measurement_separator_span().start;
        let spans = match (measurement.number_values().len(), measurement.number_range_typography()) {
            (1, _) => vec![start..end],
            (2, Some(range)) => vec![start..start + range.separator_region_start(), start + range.separator_region_end()..end],
            _ => continue,
        };
        for (span, format) in spans.into_iter().zip(measurement.number_formats()) {
            add(span, format.decimal_separator(), format.grouping_separator());
        }
    }
    // A generated phrase such as `60000 dollars` may be parsed as a currency
    // rather than a standalone number. Retain its source decimal/grouping facts.
    for fact in currency_facts(tree).filter(|fact| !fact.number_format_requires_locale_change()) {
        // Remaining symbols were deliberately left literal by the currency
        // rules; do not partially expand an unsupported amount or identifier.
        if fact.text().contains(['$', '€', '£', '¢']) || !numeric_token_boundary(text, fact.span().start, fact.span().end) {
            continue;
        }
        let Some((offset, _)) = fact.text().char_indices().find(|(_, character)| character.is_ascii_digit()) else { continue };
        let mut start = fact.span().start + offset;
        let format = fact.currency_fact().format();
        if let Some(character) = text[fact.span().start..start].chars().next_back().filter(|character| matches!(character, '+' | '-' | '−') || Some(*character) == format.decimal_separator()) {
            start -= character.len_utf8();
        }
        let length = text[start..fact.span().end]
            .trim_end()
            .char_indices()
            .take_while(|(_, character)| character.is_ascii_digit() || Some(*character) == format.decimal_separator() || Some(*character) == format.grouping_separator() || matches!(character, '+' | '-' | '−'))
            .map(|(index, character)| index + character.len_utf8())
            .last()
            .unwrap_or(0);
        let end = start + text[start..start + length].trim_end().len();
        add(start..end, format.decimal_separator(), format.grouping_separator());
    }
    replacements
}
