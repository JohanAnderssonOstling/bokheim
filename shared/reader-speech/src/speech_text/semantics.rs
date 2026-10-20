//! Speech rendering of general parser facts. Recognition stays in RustTrados.
use std::borrow::Cow;
use std::cmp::Reverse;
use std::ops::Range;

use segment_parser::{NUMBER_VALUE_SCALE, NodeKind, ParseLanguage, SegmentMeasurementFactRef, SegmentTree, mathematical_operator_spans, measurement_facts, measurement_unit_word_forms, number_range_facts, parse_text_with_language};

use super::calendar::calendar_replacements;
use super::decimals::{number_word_replacements, numeric_replacements};
use super::dimensions::dimension_replacements;
use super::entities::{currency_replacements, date_replacements, ordinal_replacements};
use super::fractions::fraction_replacements;
use super::quantity_abbreviations::quantity_abbreviation_replacements;
use super::ratios::ratio_replacements;
use super::scientific::scientific_replacements;
use super::times::time_replacements;
use super::{TextEdits, normalize_step};

// SegmentTree stores compact source and normalized-word offsets. Keep generous
// room for Unicode case expansion; oversized fragments use the existing rules.
const MAX_PARSE_BYTES: usize = 16_384;

pub(super) struct Replacement {
    span: Range<usize>,
    text: String,
}

impl Replacement {
    pub(super) fn new(span: Range<usize>, text: impl Into<String>) -> Self {
        Self { span, text: text.into() }
    }
}

fn plural_word(singular: &str, candidate: &str) -> bool {
    if candidate == format!("{singular}s") || candidate == format!("{singular}es") || (singular == "foot" && candidate == "feet") {
        return true;
    }
    if singular.strip_suffix('y').is_some_and(|stem| candidate == format!("{stem}ies")) {
        return true;
    }
    // The catalog also includes compounds such as pound-force / pounds-force.
    singular.split_once('-').zip(candidate.split_once('-')).is_some_and(|((first, tail), (plural, plural_tail))| tail == plural_tail && plural_word(first, plural))
}

fn plural_phrase(singular: &str, candidate: &str) -> bool {
    let singular = singular.split_whitespace().collect::<Vec<_>>();
    let candidate = candidate.split_whitespace().collect::<Vec<_>>();
    singular.len() == candidate.len() && singular != candidate && singular.iter().zip(candidate).all(|(word, plural)| *word == plural || plural_word(word, plural))
}

fn vocabulary_form(unit: &str, plural: bool) -> Option<String> {
    let forms = measurement_unit_word_forms(unit, ParseLanguage::English);
    let singular = forms.first()?;
    Some(if plural { forms.iter().find(|candidate| plural_phrase(singular, candidate)).unwrap_or(singular).clone() } else { singular.clone() })
}

fn unit_factor(unit: &str, plural: bool, denominator: bool) -> Option<String> {
    if let Some(form) = vocabulary_form(unit, plural) {
        return Some(form);
    }
    let (base, prefix, suffix) = unit.strip_suffix('²').map(|base| (base, "square", "squared")).or_else(|| unit.strip_suffix('³').map(|base| (base, "cubic", "cubed")))?;
    let form = vocabulary_form(base, plural)?;
    Some(if denominator { format!("{form} {suffix}") } else { format!("{prefix} {form}") })
}

fn unit_product(unit: &str, plural: bool, denominator: bool) -> Option<String> {
    let factors = unit.split('·').collect::<Vec<_>>();
    factors.iter().enumerate().map(|(index, factor)| unit_factor(factor, plural && index + 1 == factors.len(), denominator)).collect::<Option<Vec<_>>>().map(|factors| factors.join(" "))
}

pub(super) fn unit_phrase(unit: &str, plural: bool) -> Option<String> {
    if let Some(form) = vocabulary_form(unit, plural) {
        return Some(form);
    }
    match unit.split_once('/') {
        Some((numerator, denominator)) => Some(format!("{} per {}", unit_product(numerator, plural, false)?, unit_product(denominator, false, true)?)),
        None => unit_product(unit, plural, false),
    }
}

pub(super) fn ambiguous_inch_abbreviation(tree: &SegmentTree, fact: &SegmentMeasurementFactRef<'_>) -> bool {
    // In prose, a separated "in" commonly introduces a place or another
    // phrase ("1988 in New York"). Require an attached abbreviation, a
    // dotted abbreviation, or a full unit name before speaking inches.
    let unit = fact.measurement_unit_span();
    fact.measurement_fact().unit() == "in" && !fact.measurement_separator_span().is_empty() && tree.text()[unit.clone()].eq_ignore_ascii_case("in") && !tree.text()[unit.end..].starts_with('.')
}

fn measurement_replacements(tree: &SegmentTree) -> impl Iterator<Item = Replacement> + '_ {
    measurement_facts(tree).filter_map(|fact| {
        let measurement = fact.measurement_fact();
        let values = measurement.number_values();
        // Multi-value dimensions and repeated-unit ranges need individual unit
        // spans. Leave those intact rather than expand only their final unit.
        if values.is_empty() || fact.number_format_requires_locale_change() || ambiguous_inch_abbreviation(tree, &fact) || (values.len() > 1 && measurement.number_range_typography().is_none()) {
            return None;
        }
        let plural = values.len() != 1 || (values[0] != NUMBER_VALUE_SCALE && values[0] != -NUMBER_VALUE_SCALE);
        let phrase = unit_phrase(measurement.unit(), plural)?;
        Some(Replacement::new(fact.measurement_separator_span().start..fact.measurement_unit_span().end, format!(" {phrase}")))
    })
}

fn operator_phrase(operator: &str) -> Option<&'static str> {
    match operator {
        "+" => Some("plus"),
        "-" | "‐" | "‑" | "‒" | "–" | "—" | "―" | "−" => Some("minus"),
        "*" | "×" | "x" | "X" | "х" | "Х" => Some("times"),
        "/" | "÷" => Some("divided by"),
        "=" => Some("equals"),
        "±" => Some("plus or minus"),
        "≈" | "≃" => Some("approximately equals"),
        "≅" => Some("is congruent to"),
        "≠" | "!=" => Some("does not equal"),
        "<" => Some("is less than"),
        ">" => Some("is greater than"),
        "≤" | "<=" => Some("is less than or equal to"),
        "≥" | ">=" => Some("is greater than or equal to"),
        _ => None,
    }
}

fn protected_spans(tree: &SegmentTree) -> Vec<Range<usize>> {
    let mut spans = tree.preorder_iter().filter(|node| node.protected_kind().is_some()).map(|node| node.span()).collect::<Vec<_>>();
    // Explicit backtick delimiters also protect short examples that the
    // parser does not classify as programming syntax, such as `$60k`.
    let mut backticks = tree.text().match_indices('`').peekable();
    let mut opening = None;
    while let Some((start, _)) = backticks.next() {
        let mut end = start + 1;
        while backticks.peek().is_some_and(|(index, _)| *index == end) {
            backticks.next();
            end += 1;
        }
        let length = end - start;
        match opening {
            Some((open_start, open_length)) if open_length == length => {
                spans.push(open_start..end);
                opening = None;
            }
            None => opening = Some((start, length)),
            _ => {}
        }
    }
    spans.sort_by_key(|span| (span.start, Reverse(span.end)));
    spans.into_iter().fold(Vec::<Range<usize>>::new(), |mut protected, span| {
        if let Some(previous) = protected.last_mut().filter(|previous| span.start <= previous.end) {
            previous.end = previous.end.max(span.end);
        } else {
            protected.push(span);
        }
        protected
    })
}

fn ellipsis_replacements(tree: &SegmentTree) -> Vec<Replacement> {
    let mut result = Vec::new();
    let mut nodes = tree.iter_nodes().peekable();
    while let Some(node) = nodes.next() {
        if node.kind() != NodeKind::Ellipsis && !(node.kind() == NodeKind::Punct && node.text() == ".") {
            continue;
        }
        let mut span = node.span();
        let mut ellipsis = node.kind() == NodeKind::Ellipsis;
        while let Some(next) = nodes.peek().filter(|next| next.span().start == span.end && (next.kind() == NodeKind::Ellipsis || (next.kind() == NodeKind::Punct && next.text() == "."))) {
            span.end = next.span().end;
            ellipsis |= next.kind() == NodeKind::Ellipsis;
            nodes.next();
        }
        // A comma gives the voice a hesitation within the sentence. Merge
        // adjacent punctuation without touching dots inside semantic nodes.
        if ellipsis || span.len() > 1 {
            result.push(Replacement::new(span, if ellipsis { "," } else { "." }));
        }
    }
    result
}

fn replacements(tree: &SegmentTree, protected: &[Range<usize>]) -> Vec<Replacement> {
    let ranges = number_range_facts(tree).filter_map(|fact| fact.separator_region_span().map(|span| Replacement::new(span, " to ")));
    let operators = mathematical_operator_spans(tree).into_iter().filter_map(|span| {
        let operator = &tree.text()[span.clone()];
        let spaced_before = tree.text()[..span.start].chars().next_back().is_some_and(char::is_whitespace);
        let spaced_after = tree.text()[span.end..].chars().next().is_some_and(char::is_whitespace);
        // Compact slashes can denote fractions or incomplete dates such as
        // 03/04. Explicit division signs and spaced arithmetic are clearer.
        if operator == "/" && !(spaced_before && spaced_after) {
            return None;
        }
        let phrase = operator_phrase(operator)?;
        let before = if spaced_before { "" } else { " " };
        let after = if spaced_after { "" } else { " " };
        Some(Replacement::new(span, format!("{before}{phrase}{after}")))
    });
    let mut replacements = measurement_replacements(tree)
        .chain(ranges)
        .chain(operators)
        .chain(currency_replacements(tree))
        .chain(date_replacements(tree))
        .chain(ordinal_replacements(tree))
        .chain(time_replacements(tree))
        .chain(numeric_replacements(tree))
        .chain(dimension_replacements(tree))
        .chain(fraction_replacements(tree))
        .chain(scientific_replacements(tree))
        .chain(quantity_abbreviation_replacements(tree))
        .chain(calendar_replacements(tree))
        .chain(ratio_replacements(tree))
        .chain(ellipsis_replacements(tree))
        .filter(|replacement| !protected.iter().any(|span| span.start < replacement.span.end && replacement.span.start < span.end))
        .collect::<Vec<_>>();
    replacements.sort_by_key(|replacement| (replacement.span.start, Reverse(replacement.span.end)));
    replacements.into_iter().fold(Vec::<Replacement>::new(), |mut accepted, replacement| {
        if accepted.last().is_none_or(|previous| previous.span.end <= replacement.span.start) {
            accepted.push(replacement);
        }
        accepted
    })
}

fn render_prose<'a>(text: &'a str, span: Range<usize>, replacements: &[Replacement], normalize: &impl for<'b> Fn(&'b str) -> Cow<'b, str>) -> Cow<'a, str> {
    let mut result = TextEdits::new(&text[span.clone()]);
    for replacement in replacements.iter().filter(|replacement| span.start <= replacement.span.start && replacement.span.end <= span.end) {
        result.replace(replacement.span.start - span.start..replacement.span.end - span.start, &replacement.text);
    }
    normalize_step(result.finish(), normalize)
}

pub(super) fn normalize_semantics(text: &str, normalize: impl for<'a> Fn(&'a str) -> Cow<'a, str>) -> Cow<'_, str> {
    if text.len() > MAX_PARSE_BYTES {
        return normalize(text);
    }
    let tree = parse_text_with_language(text, ParseLanguage::English);
    let protected = protected_spans(&tree);
    let replacements = replacements(&tree, &protected);
    if protected.is_empty() {
        return render_prose(text, 0..text.len(), &replacements, &normalize);
    }
    let mut result = TextEdits::new(text);
    let mut cursor = 0;
    for span in protected {
        result.replace(cursor..span.start, &render_prose(text, cursor..span.start, &replacements, &normalize));
        cursor = span.end;
    }
    result.replace(cursor..text.len(), &render_prose(text, cursor..text.len(), &replacements, &normalize));
    result.finish()
}

pub(super) fn normalize_number_words(text: &str) -> Cow<'_, str> {
    if text.len() > MAX_PARSE_BYTES || !text.bytes().any(|byte| byte.is_ascii_digit()) {
        return Cow::Borrowed(text);
    }
    let tree = parse_text_with_language(text, ParseLanguage::English);
    let protected = protected_spans(&tree);
    let mut replacements = number_word_replacements(&tree);
    replacements.sort_by_key(|replacement| (replacement.span.start, Reverse(replacement.span.end)));
    let mut result = TextEdits::new(text);
    let mut cursor = 0;
    for replacement in replacements {
        if replacement.span.start < cursor || protected.iter().any(|span| span.start < replacement.span.end && replacement.span.start < span.end) {
            continue;
        }
        cursor = replacement.span.end;
        result.replace(replacement.span, &replacement.text);
    }
    result.finish()
}
