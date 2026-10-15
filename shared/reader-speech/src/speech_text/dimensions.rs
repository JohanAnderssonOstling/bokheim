//! Dimension wording is grounded in parsed geometric measurements.
use segment_parser::{NUMBER_VALUE_SCALE, SegmentMeasurementFactRef, SegmentTree, measurement_facts};

use super::decimals::source_number_phrase;
use super::semantics::{Replacement, unit_phrase};

fn geometry_phrase(tree: &SegmentTree, fact: &SegmentMeasurementFactRef<'_>) -> Option<String> {
    let measurement = fact.measurement_fact();
    let values = measurement.number_values();
    if !(1..=2).contains(&values.len()) || measurement.number_formats().len() != values.len() || values.iter().any(|value| *value < 0) || measurement.number_range_typography().is_some() || fact.number_format_requires_locale_change() {
        return None;
    }
    // Lengths and pixel sizes denote geometry. Other physical quantities can
    // instead be arithmetic operands and retain multiplication wording.
    let length = measurement.values().first().is_some_and(|value| value.dimension().exponents() == &[1, 0, 0, 0, 0, 0, 0]);
    if !length && measurement.unit() != "px" {
        return None;
    }
    let numeric = &tree.text()[fact.span().start..fact.measurement_separator_span().start];
    let parts = numeric.split(['×', 'x', 'X', 'х', 'Х']).collect::<Vec<_>>();
    if parts.len() != values.len() {
        return None;
    }
    let numbers = parts.iter().zip(measurement.number_formats()).map(|(part, format)| source_number_phrase(part.trim(), format.decimal_separator(), format.grouping_separator())).collect::<Option<Vec<_>>>()?;
    let plural = values.len() != 1 || values[0] != NUMBER_VALUE_SCALE;
    let unit = unit_phrase(measurement.unit(), plural)?;
    Some(format!("{} {unit}", numbers.join(" by ")))
}

pub(super) fn dimension_replacements(tree: &SegmentTree) -> Vec<Replacement> {
    let facts = measurement_facts(tree).collect::<Vec<_>>();
    let mut replacements = Vec::new();
    let mut index = 0;
    while let Some(first) = facts.get(index) {
        let Some(first_phrase) = geometry_phrase(tree, first) else {
            index += 1;
            continue;
        };
        let mut phrases = vec![first_phrase];
        let mut end = index + 1;
        while let Some(next) = facts.get(end) {
            let previous = &facts[end - 1];
            let gap = &tree.text()[previous.span().end..next.span().start];
            if gap.contains(['\n', '\r']) || !matches!(gap.trim(), "×" | "x" | "X" | "х" | "Х") || (first.measurement_fact().unit() == "px") != (next.measurement_fact().unit() == "px") {
                break;
            }
            let Some(phrase) = geometry_phrase(tree, next) else { break };
            phrases.push(phrase);
            end += 1;
        }
        let last = &facts[end - 1];
        let equation = tree.text()[last.span().end..].trim_start().starts_with('=');
        if !equation && (phrases.len() > 1 || first.measurement_fact().number_values().len() == 2) {
            replacements.push(Replacement::new(first.span().start..last.span().end, phrases.join(" by ")));
        }
        index = end;
    }
    replacements
}
