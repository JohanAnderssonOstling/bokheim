use crate::{SegmentMeasurementFactRef, SegmentNumberRangeFactRef, SegmentNumericFactRef, SegmentNumericKind};
use structure::{DateFact, MeasurementValueFact};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum NumericComparisonKey<'a> {
    NumberValues(&'a [i128]),
    Date(DateFact),
    Time(u8, u8, u8),
    Text(&'a str),
}

pub fn numeric_facts_value_equivalent<'a, 'b, L, R>(left: &L, right: &R) -> bool where L: Copy + Into<SegmentNumericFactRef<'a>>, R: Copy + Into<SegmentNumericFactRef<'b>> {
    numeric_comparison_key((*left).into()) == numeric_comparison_key((*right).into())
}

pub fn currency_presence_equivalent<'a, 'b, L, R>(left: &L, right: &R) -> bool where L: Copy + Into<SegmentNumericFactRef<'a>>, R: Copy + Into<SegmentNumericFactRef<'b>> {
    let left: SegmentNumericFactRef<'a> = (*left).into();
    let right: SegmentNumericFactRef<'b> = (*right).into();
    left.kind() == SegmentNumericKind::Currency && right.kind() == SegmentNumericKind::Currency
}

pub fn measurement_units_equivalent(left: &SegmentMeasurementFactRef<'_>, right: &SegmentMeasurementFactRef<'_>) -> bool {
    let left = left.measurement_fact();
    let right = right.measurement_fact();
    left.unit() == right.unit()
        || !measurement_values_require_unit_identity(left.values())
            && !measurement_values_require_unit_identity(right.values())
            && measurement_values_equivalent(left.values(), right.values())
}

fn measurement_values_require_unit_identity(values: &[MeasurementValueFact]) -> bool { values.iter().any(|value| value.quantity_kind() == structure::MeasurementQuantityKind::LogarithmicLevel) }

fn measurement_values_equivalent(left: &[MeasurementValueFact], right: &[MeasurementValueFact]) -> bool {
    !left.is_empty()
        && left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            left.dimension() == right.dimension()
                && left.amount().abs_diff(right.amount()) <= 1
                && left.quantity_kind() == right.quantity_kind()
        })
}

pub fn measurement_numbers_equal(left: &SegmentMeasurementFactRef<'_>, right: &SegmentMeasurementFactRef<'_>) -> bool { left.number_values() == right.number_values() }

pub fn number_range_values_equal(left: &SegmentNumberRangeFactRef<'_>, right: &SegmentNumberRangeFactRef<'_>) -> bool {
    left.values() == right.values()
}

fn numeric_comparison_key(fact: SegmentNumericFactRef<'_>) -> NumericComparisonKey<'_> {
    match (fact.date_fact(), fact.time_fact(), fact.number_values()) {
        (Some(date), _, _) => NumericComparisonKey::Date(date),
        (_, Some(time), _) => NumericComparisonKey::Time(time.hour(), time.minute(), time.second().unwrap_or(0)),
        (_, _, values) if !values.is_empty() => NumericComparisonKey::NumberValues(values),
        _ => NumericComparisonKey::Text(normalize_numeric_comparison_text(fact.text())),
    }
}

fn normalize_numeric_comparison_text(text: &str) -> &str { crate::stream_util::trim_trailing_punctuation(text) }

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(text: &str, language: &str) -> structure::SegmentTree {
        let mut tree = crate::tokenized_text_tree(text);
        crate::parse(&mut tree, language);
        tree
    }

    #[test]
    fn numeric_equivalence_uses_parsed_values_instead_of_surface_form() {
        let source = parsed("At 14:00 on 12 June 2024", "en");
        let target = parsed("At 2 PM on June 12, 2024", "en");
        let source_time = crate::time_facts(&source).next().expect("source time");
        let target_time = crate::time_facts(&target).next().expect("target time");
        assert!(numeric_facts_value_equivalent(&source_time, &target_time));
    }

    #[test]
    fn measurement_value_equivalence_requires_compatible_quantity_kinds() {
        let active_power = MeasurementValueFact::new(structure::MeasurementDimension::Power, 100).with_quantity_kind(structure::MeasurementQuantityKind::ActivePower);
        let apparent_power = MeasurementValueFact::new(structure::MeasurementDimension::Power, 100).with_quantity_kind(structure::MeasurementQuantityKind::ApparentPower);
        let equivalent_active_power = MeasurementValueFact::new(structure::MeasurementDimension::Power, 100).with_quantity_kind(structure::MeasurementQuantityKind::ActivePower);

        assert!(measurement_values_equivalent(&[active_power], &[equivalent_active_power]));
        assert!(!measurement_values_equivalent(&[active_power], &[apparent_power]));
    }

    #[test]
    fn logarithmic_measurements_preserve_their_canonical_qualifier() {
        let decibels = parsed("20 dB", "en");
        let weighted_decibels = parsed("20 dBA", "en");
        let decibels = crate::measurement_facts(&decibels).next().expect("decibel measurement");
        let weighted_decibels = crate::measurement_facts(&weighted_decibels).next().expect("A-weighted decibel measurement");

        assert!(!measurement_units_equivalent(&decibels, &weighted_decibels));
        assert!(measurement_units_equivalent(&decibels, &decibels));
    }
}
