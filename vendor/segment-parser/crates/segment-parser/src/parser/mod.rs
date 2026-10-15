mod context;
mod boundary;
mod currency;
mod date;
mod magnitude;
mod measurement;
mod nodes;
mod non_translatable;
mod number;
mod numeric_relations;
mod ordinal;
mod phrase;
mod scan;
mod span;
mod time;
mod unit;
mod util;
mod version;
pub(crate) mod web;
mod word;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod test_support;

use std::ops::Range;

use crate::language_parser::{DateFieldOrder, MeasurementSystem, NumberParsingPolicy, ParseLanguage, ParseLocale};
use structure::parser_api::{Node, NodeAnnotation, NodeSemantic, TokenUnit};
use structure::{CurrencySemantic, DateFact, DelimiterAnnotation, DelimiterFamily, DelimiterRole, MeasurementDegreeStyle, MeasurementDimension, MeasurementQuantityKind, MeasurementSemantic, MeasurementTypography, MeasurementValueFact, NodeKind, NumberFormat, NumberFormatAssessment, NumberRangeSemantic, NumberRangeTypography, NumberSemantic, NumericTypographySpacing, OrdinalFact, OrdinalStyle, PunctuationSpacingSide, SegmentTree, TimeFact, UnitKind, VersionPrefixStyle, VersionSemantic, WhitespaceTypographyFact, WhitespaceTypographyKind, WhitespaceTypographyPreference, WhitespaceTypographyRole};

use context::*;
use boundary::*;
use currency::*;
use date::*;
use magnitude::*;
use measurement::*;
use nodes::*;
use non_translatable::*;
use number::*;
pub(crate) use number::NORMALIZED_VALUE_SCALE;
use numeric_relations::*;
use ordinal::*;
use phrase::*;
use scan::*;
use scan::{DetectionPriority, PhraseAmbiguity, PhraseConfidence};
pub(crate) use span::ParseOutput;
use span::{CandidateScore, Detected, Detection, UnitRange};
use time::*;
use unit::*;
pub(crate) use unit::measurement_unit_word_forms;
use util::*;
use version::*;
use web::*;
use word::*;

/// `known_units` are canonical measurement
/// unit ids already found in the *other* side of the segment pair (e.g. the
/// source's units, when parsing target). Localized full unit names are
/// recognized directly after numbers; grounding is retained for otherwise
/// ambiguous unit spellings. Parsing source itself (or target with no
/// counterpart) should pass `&[]`.
pub(crate) fn parse_with_entities(stream: &SegmentTree, locale: ParseLocale, known_units: &[&str]) -> ParseOutput { parse_output(stream, locale, ParseGrounding::new().with_known_units(known_units)) }

pub(crate) fn parse_target_with_entities(stream: &SegmentTree, locale: ParseLocale, known_units: &[&str], known_number_values: &[i128], known_currency_units: &[&str], known_non_translatables: &[&str]) -> ParseOutput {
    let grounding = ParseGrounding::new()
        .with_known_units(known_units)
        .with_known_number_values(known_number_values)
        .with_known_currency_units(known_currency_units)
        .with_known_non_translatables(known_non_translatables);
    parse_output(stream, locale, grounding)
}

fn parse_output(stream: &SegmentTree, locale: ParseLocale, grounding: ParseGrounding<'_>) -> ParseOutput {
    let context = ParseContext::new(stream, locale, grounding);
    let mut nodes = parse_nodes_from_phrases(&context);
    annotate_placeholders(stream, &mut nodes);
    annotate_numeric_relations(&context, &mut nodes);
    ParseOutput { nodes }
}

fn annotate_placeholders(stream: &SegmentTree, nodes: &mut Vec<Node>) {
    crate::detect_placeholders(stream.text()).into_iter().rev().for_each(|detected| {
        let (span, semantic) = detected.into_parts();
        wrap_semantic_span(nodes, span, NodeSemantic::Placeholder(Box::new(semantic)));
    });
}

fn wrap_semantic_span(nodes: &mut Vec<Node>, span: Range<usize>, semantic: NodeSemantic) -> bool {
    let selected = nodes.iter().enumerate().filter(|(_, node)| semantic_span_node_member(node, &span)).map(|(index, _)| index).collect::<Vec<_>>();
    let Some(first) = selected.first().copied() else { return false; };
    let Some(last) = selected.last().copied() else { return false; };
    let visible = nodes[first..=last].iter().filter(|node| node.kind() != NodeKind::Tag).collect::<Vec<_>>();
    if visible.first().is_none_or(|node| node.start() != span.start) || visible.last().is_none_or(|node| node.end() != span.end) {
        return false;
    }
    if visible.iter().any(|node| node.start() < span.start || node.end() > span.end) {
        return false;
    }
    let children = nodes.drain(first..=last).filter(|node| node.kind() == NodeKind::Tag).collect();
    nodes.insert(first, Node::with_semantic(span, None, semantic, children));
    debug_assert!(nodes_are_ordered_without_overlapping_text(nodes));
    true
}

fn semantic_span_node_member(node: &Node, span: &Range<usize>) -> bool {
    if node.kind() == NodeKind::Tag {
        return span.start < node.start() && node.start() < span.end;
    }
    span.start < node.end() && node.start() < span.end
}

#[cfg(test)]
fn parse_nodes(stream: &SegmentTree, language: ParseLanguage) -> Vec<Node> { parse_nodes_with_known_units(stream, language, &[]) }

#[cfg(test)]
fn parse_nodes_with_known_units(stream: &SegmentTree, language: ParseLanguage, known_units: &[&str]) -> Vec<Node> {
    parse_output(stream, ParseLocale::from_language(language), ParseGrounding::new().with_known_units(known_units)).nodes
}
