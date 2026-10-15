use crate::language_parser::LanguageTag;
use crate::{replace_range_with_text, ParseLanguage};
use std::ops::Range;
use structure::{NodeKind, NodeRef, NumberFormat, SegmentTree};

/// Applies target-locale decimal and grouping separators to every parsed
/// number in `tree`, preserving value and precision.
pub fn localize_numbers(tree: &mut SegmentTree, target_language: &str) -> Option<usize> { localize_numeric_kind(tree, target_language, NodeKind::Number) }

/// Applies target-locale decimal and grouping separators to every parsed
/// measurement in `tree`, preserving magnitude, precision, range separators,
/// unit text, tags, and surrounding punctuation.
pub fn localize_measurements(tree: &mut SegmentTree, target_language: &str) -> Option<usize> { localize_numeric_kind(tree, target_language, NodeKind::Measurement) }

fn localize_numeric_kind(tree: &mut SegmentTree, target_language: &str, kind: NodeKind) -> Option<usize> {
    if !tree.iter_nodes().any(|node| node.kind() == kind) {
        return Some(0);
    }
    let (target_decimal, target_grouping) = locale_number_separators(target_language)?;
    let mut replacements = Vec::new();
    for node in tree.iter_nodes() {
        if node.kind() == kind {
            let localized = localized_numeric_text(node.text(), node.number_formats(), target_decimal, target_grouping)?;
            if localized != node.text() {
                replacements.push((node_unit_range(tree, &node)?, localized));
            }
        }
    }
    let count = replacements.len();
    let parse_language = ParseLanguage::from_bcp47(target_language);
    replacements.into_iter().rev().for_each(|(range, text)| {
        replace_range_with_text(tree, range, &text, parse_language);
    });
    Some(count)
}

fn locale_number_separators(language: &str) -> Option<(char, char)> {
    let tag = LanguageTag::parse(language);
    tag.supported_profile()?;
    let policy = tag.number_policy();
    Some((policy.decimal_separators()[0], policy.canonical_grouping_separator()))
}

fn node_unit_range(tree: &SegmentTree, node: &NodeRef<'_>) -> Option<Range<usize>> {
    let ids = node.token_unit_ids();
    let first = tree.token_index(*ids.first()?)?;
    let last = tree.token_index(*ids.last()?)?;
    Some(first..last + 1)
}

fn localized_numeric_text(text: &str, formats: &[NumberFormat], target_decimal: char, target_grouping: char) -> Option<String> {
    rendered_numeric_text(text, formats, |_| Some((target_decimal, target_grouping)))
}

/// Renders parsed numeric components using the locale expectations recorded
/// by the parser, preserving values, precision, units, and surrounding text.
pub fn canonical_numeric_text(text: &str, formats: &[NumberFormat]) -> Option<String> {
    rendered_numeric_text(text, formats, |format| Some((format.locale_decimal_separator()?, format.locale_grouping_separator()?)))
}

fn rendered_numeric_text(text: &str, formats: &[NumberFormat], separators: impl Fn(NumberFormat) -> Option<(char, char)>) -> Option<String> {
    let spans = numeric_component_spans(text);
    if spans.len() != formats.len() {
        return None;
    }
    let replacements = spans
        .iter()
        .zip(formats)
        .map(|(span, format)| separators(*format).and_then(|(decimal, grouping)| numeric_separator_replacements(text, span, *format, decimal, grouping)))
        .collect::<Option<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    debug_assert!(replacements.is_sorted_by_key(|(position, _)| *position));
    Some(text.char_indices().map(|(index, ch)| replacements.binary_search_by_key(&index, |(position, _)| *position).ok().map_or(ch, |replacement| replacements[replacement].1)).collect())
}

fn numeric_separator_replacements(text: &str, span: &Range<usize>, format: NumberFormat, target_decimal: char, target_grouping: char) -> Option<Vec<(usize, char)>> {
    let decimal_position = source_decimal_position(text, span, format.decimal_separator())?;
    let component = text.get(span.clone())?;
    Some(component.char_indices().filter_map(|(local, ch)| {
        let position = span.start + local;
        separator_between_digits(text, position, ch).then_some((position, if decimal_position == Some(position) { target_decimal } else { target_grouping }))
    }).collect())
}

fn source_decimal_position(text: &str, span: &Range<usize>, separator: Option<char>) -> Option<Option<usize>> {
    let Some(separator) = separator else { return Some(None); };
    let positions = text
        .get(span.clone())?
        .char_indices()
        .filter_map(|(local, ch)| (ch == separator && separator_between_digits(text, span.start + local, ch)).then_some(span.start + local))
        .collect::<Vec<_>>();
    (positions.len() == 1).then(|| Some(positions[0]))
}

fn separator_between_digits(text: &str, position: usize, separator: char) -> bool {
    if !numeric_separator(separator) {
        return false;
    }
    text.get(..position).and_then(|before| before.chars().next_back()).is_some_and(|ch| ch.is_ascii_digit())
        && text.get(position + separator.len_utf8()..).and_then(|after| after.chars().next()).is_some_and(|ch| ch.is_ascii_digit())
}

fn numeric_component_spans(text: &str) -> Vec<Range<usize>> {
    let chars = text.char_indices().collect::<Vec<_>>();
    let mut spans = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        if !chars[index].1.is_ascii_digit() {
            index += 1;
            continue;
        }
        let start = chars[index].0;
        index += 1;
        while index < chars.len() {
            if chars[index].1.is_ascii_digit() {
                index += 1;
                continue;
            }
            if numeric_separator(chars[index].1) && chars.get(index + 1).is_some_and(|(_, next)| next.is_ascii_digit()) {
                index += 2;
                continue;
            }
            break;
        }
        let end = chars.get(index).map_or(text.len(), |(offset, _)| *offset);
        spans.push(start..end);
    }
    spans
}

fn numeric_separator(ch: char) -> bool {
    [ParseLanguage::English, ParseLanguage::Swedish, ParseLanguage::German]
        .into_iter()
        .any(|language| language.profile().number_policy.grouping_separators().contains(&ch) || language.profile().number_policy.decimal_separators().contains(&ch))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    fn parsed(text: &str, language: ParseLanguage) -> SegmentTree { let mut tree = crate::tokenized_text_tree(text); parse(&mut tree, language); tree }

    #[test]
    fn localizes_measurement_decimals_without_changing_sentence_punctuation() {
        let mut tree = parsed("0.1–5%.", ParseLanguage::English);

        assert_eq!(localize_measurements(&mut tree, "sv-SE"), Some(1));
        assert_eq!(tree.text(), "0,1–5%.");
    }

    #[test]
    fn localizes_number_decimal_and_grouping_separators() {
        let mut tree = parsed("1,234.5.", ParseLanguage::English);

        assert_eq!(localize_numbers(&mut tree, "sv-SE"), Some(1));
        assert_eq!(tree.text(), "1\u{00A0}234,5.");
    }

    #[test]
    fn localizes_swiss_curly_apostrophe_grouping() {
        let mut tree = crate::tokenized_text_tree("1’234");
        crate::parse(&mut tree, "de-CH");

        assert_eq!(localize_numbers(&mut tree, "de-DE"), Some(1));
        assert_eq!(tree.text(), "1.234");
    }

    #[test]
    fn leaves_values_already_using_the_target_locale_unchanged() {
        let mut tree = parsed("0.1–5%", ParseLanguage::English);

        assert_eq!(localize_measurements(&mut tree, "en-US"), Some(0));
        assert_eq!(tree.text(), "0.1–5%");
    }

    #[test]
    fn renders_canonical_format_from_parser_semantics() {
        let tree = parsed("1,234.50 kg", ParseLanguage::Swedish);
        let measurement = tree.iter_nodes().find(|node| node.kind() == NodeKind::Measurement).expect("measurement");

        assert_eq!(canonical_numeric_text(measurement.text(), measurement.number_formats()).as_deref(), Some("1\u{00A0}234,50 kg"));
    }

    #[test]
    fn declines_unknown_locale_formatting() {
        let mut tree = parsed("0.1–5%", ParseLanguage::English);

        assert_eq!(localize_measurements(&mut tree, "xx-XX"), None);
        assert_eq!(tree.text(), "0.1–5%");
    }

    #[test]
    fn unknown_locale_does_not_block_trees_without_numeric_localization() {
        let mut tree = parsed("https://example.com", ParseLanguage::English);

        assert_eq!(localize_numbers(&mut tree, "xx-XX"), Some(0));
        assert_eq!(localize_measurements(&mut tree, "xx-XX"), Some(0));
    }
}
