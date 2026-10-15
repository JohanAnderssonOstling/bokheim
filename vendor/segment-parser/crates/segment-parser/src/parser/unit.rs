mod catalog;
mod compound;
mod resolution;

pub(crate) use catalog::measurement_unit_word_forms;

pub(super) use catalog::{parse_source_grounded_unit_canonical, parse_unit_alias, parse_unit_symbol, parse_unit_word, unit_spelling_allowed_in_language, unit_spelling_is_universally_ambiguous_after_number};
#[cfg(test)]
pub(super) use catalog::{unit_semantics_are_complete, unit_symbols_are_unique};
pub(super) use compound::ResolvedMeasurementUnit;
pub(super) use resolution::{canonical_measurement_unit, canonical_measurement_unit_for_locale, locale_capacity_unit, resolve_canonical_measurement_unit, resolve_measurement_unit_for_locale};
#[cfg(test)]
pub(super) use resolution::resolve_measurement_unit;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum UnitAttachment {
    Attached,
    Separated,
}
