use super::super::*;
use super::catalog::*;
use super::compound::*;
use super::UnitAttachment;
use crate::measurement_unit::BaseUnit;

enum MeasurementUnitResolution {
    Semantic(ResolvedMeasurementUnit),
    Opaque(String),
}

impl MeasurementUnitResolution {
    fn atomic(prefix: Option<SiPrefix>, base: BaseUnit) -> Self { ResolvedMeasurementUnit::simple(prefix, base).map(Self::Semantic).unwrap_or_else(|| Self::Opaque(canonical_symbol(prefix, base))) }
    fn semantic(resolved: ResolvedMeasurementUnit) -> Self { Self::Semantic(resolved) }
    fn into_semantic(self) -> Option<ResolvedMeasurementUnit> { match self { Self::Semantic(resolved) => Some(resolved), Self::Opaque(_) => None } }
    fn into_canonical(self) -> String { match self { Self::Semantic(resolved) => resolved.into_canonical(), Self::Opaque(canonical) => canonical } }
}

fn resolve_measurement_unit_spelling(text: &str, language: ParseLanguage) -> Option<MeasurementUnitResolution> {
    if text == "V/EN" || !unit_spelling_allowed_in_language(text, language) {
        return None;
    }
    if let Some((prefix, base)) = resolve_unit(text, language) {
        return Some(MeasurementUnitResolution::atomic(prefix, base));
    }
    let canonical = canonical_unit_alias(text, language).unwrap_or(text);
    if let Some((prefix, base)) = resolve_unit(canonical, language) {
        return Some(MeasurementUnitResolution::atomic(prefix, base));
    }
    resolve_compound_measurement_unit(canonical, Some(language)).map(MeasurementUnitResolution::semantic)
}

#[cfg(test)]
pub(in crate::parser) fn resolve_measurement_unit(text: &str, language: ParseLanguage) -> Option<ResolvedMeasurementUnit> { resolve_measurement_unit_spelling(text, language)?.into_semantic() }

#[derive(Clone, Copy)]
struct LocaleUnitRule {
    spelling: &'static str,
    measurement_system: MeasurementSystem,
    unit: BaseUnit,
    separated_only: bool,
}

impl LocaleUnitRule {
    const fn new(spelling: &'static str, measurement_system: MeasurementSystem, unit: BaseUnit, separated_only: bool) -> Self { Self { spelling, measurement_system, unit, separated_only } }
    fn matches(self, text: &str, measurement_system: MeasurementSystem, attachment: UnitAttachment) -> bool { self.spelling == text && self.measurement_system == measurement_system && (!self.separated_only || attachment == UnitAttachment::Separated) }
}

const LOCALE_UNIT_RULES: &[LocaleUnitRule] = &[
    LocaleUnitRule::new("fl oz", MeasurementSystem::ImperialUk, BaseUnit::ImperialFluidOunce, false),
    LocaleUnitRule::new("fl oz", MeasurementSystem::UsCustomary, BaseUnit::UsFluidOunce, false),
    LocaleUnitRule::new("pt", MeasurementSystem::ImperialUk, BaseUnit::ImperialPint, true),
    LocaleUnitRule::new("pt", MeasurementSystem::UsCustomary, BaseUnit::UsLiquidPint, true),
    LocaleUnitRule::new("qt", MeasurementSystem::ImperialUk, BaseUnit::ImperialQuart, false),
    LocaleUnitRule::new("qt", MeasurementSystem::UsCustomary, BaseUnit::UsLiquidQuart, false),
    LocaleUnitRule::new("bu", MeasurementSystem::ImperialUk, BaseUnit::ImperialBushel, false),
    LocaleUnitRule::new("bu", MeasurementSystem::UsCustomary, BaseUnit::UsBushel, false),
    LocaleUnitRule::new("gal", MeasurementSystem::ImperialUk, BaseUnit::ImperialGallon, false),
    LocaleUnitRule::new("gal", MeasurementSystem::UsCustomary, BaseUnit::UsLiquidGallon, false),
];

pub(in crate::parser) fn locale_capacity_unit(text: &str, locale: ParseLocale, attachment: UnitAttachment) -> Option<BaseUnit> {
    let measurement_system = locale.measurement_system()?;
    LOCALE_UNIT_RULES.iter().copied().find(|rule| rule.matches(text, measurement_system, attachment)).map(|rule| rule.unit)
}

fn resolve_measurement_unit_spelling_for_locale(text: &str, locale: ParseLocale, attachment: UnitAttachment) -> Option<MeasurementUnitResolution> {
    locale_capacity_unit(text, locale, attachment)
        .map(|base| MeasurementUnitResolution::atomic(None, base))
        .or_else(|| resolve_measurement_unit_spelling(text, locale.language()))
}

pub(in crate::parser) fn resolve_measurement_unit_for_locale(text: &str, locale: ParseLocale, attachment: UnitAttachment) -> Option<ResolvedMeasurementUnit> { resolve_measurement_unit_spelling_for_locale(text, locale, attachment)?.into_semantic() }

fn resolve_canonical_measurement_unit_spelling(text: &str) -> Option<MeasurementUnitResolution> {
    if text == "V/EN" {
        return None;
    }
    if let Some((prefix, base)) = parse_unit_symbol(text) {
        return Some(MeasurementUnitResolution::atomic(prefix, base));
    }
    resolve_compound_measurement_unit(text, None).map(MeasurementUnitResolution::semantic)
}

pub(in crate::parser) fn resolve_canonical_measurement_unit(text: &str) -> Option<ResolvedMeasurementUnit> { resolve_canonical_measurement_unit_spelling(text)?.into_semantic() }

pub(in crate::parser) fn canonical_measurement_unit_for_locale(text: &str, locale: ParseLocale, attachment: UnitAttachment) -> Option<String> {
    resolve_measurement_unit_spelling_for_locale(text, locale, attachment).map(MeasurementUnitResolution::into_canonical)
}

/// Canonical id for any recognized unit spelling — see `resolve_unit`. Used
/// both to recognize an expanded-form word as a unit (when it matches a
/// `known_units` entry) and to compute the measurement node's semantic unit
/// attached to every `Measurement` node, which is how downstream consumers
/// (e.g. QA) compare units without knowing about spellings themselves.
pub(in crate::parser) fn canonical_measurement_unit(text: &str, language: ParseLanguage) -> Option<String> {
    resolve_measurement_unit_spelling(text, language).map(MeasurementUnitResolution::into_canonical)
}

