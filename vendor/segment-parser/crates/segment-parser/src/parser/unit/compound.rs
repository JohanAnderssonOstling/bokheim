use super::super::*;
use super::catalog::*;
use crate::measurement_unit::BaseUnit;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct AtomicMeasurementUnit {
    prefix: Option<SiPrefix>,
    base: BaseUnit,
}

impl AtomicMeasurementUnit {
    fn new(prefix: Option<SiPrefix>, base: BaseUnit) -> Self { Self { prefix, base } }
    fn canonical(self) -> String { canonical_symbol(self.prefix, self.base) }
    fn quantity_kind(self) -> Option<MeasurementQuantityKind> { self.base.semantics().quantity_kind() }
    fn dimension(self) -> Option<MeasurementDimension> { self.quantity_kind().map(measurement_dimension) }
    fn multiplier(self) -> Option<f64> { Some(self.prefix.map_or(1.0, SiPrefix::multiplier) * self.base.semantics().linear_multiplier()?) }
    fn precision_limited(self) -> bool { self.base.precision_limited() }
    fn normalized_value(self, value: f64) -> Option<f64> { self.base.semantics().normalized_value(self.prefix.map_or(value, |prefix| value * prefix.multiplier())) }
}

#[derive(Clone, Debug, PartialEq)]
pub(in crate::parser) struct ResolvedMeasurementUnit {
    canonical: String,
    dimension: MeasurementDimension,
    quantity_kind: MeasurementQuantityKind,
    multiplier: f64,
    atomic: Option<AtomicMeasurementUnit>,
    precision_limited: bool,
}

impl ResolvedMeasurementUnit {
    pub(in crate::parser) fn simple(prefix: Option<SiPrefix>, base: BaseUnit) -> Option<Self> {
        let atomic = AtomicMeasurementUnit::new(prefix, base);
        Some(Self { canonical: atomic.canonical(), dimension: atomic.dimension()?, quantity_kind: atomic.quantity_kind()?, multiplier: atomic.multiplier()?, atomic: Some(atomic), precision_limited: atomic.precision_limited() })
    }
    fn compound(canonical: String, dimension: MeasurementDimension, quantity_kind: MeasurementQuantityKind, multiplier: f64, precision_limited: bool) -> Self { Self { canonical, dimension, quantity_kind, multiplier, atomic: None, precision_limited } }
    pub(in crate::parser) fn into_canonical(self) -> String { self.canonical }
    pub(in crate::parser) fn canonical(&self) -> &str { &self.canonical }
    pub(in crate::parser) fn dimension(&self) -> MeasurementDimension { self.dimension }
    pub(in crate::parser) fn quantity_kind(&self) -> MeasurementQuantityKind { self.quantity_kind }
    pub(in crate::parser) fn precision_limited(&self) -> bool { self.precision_limited }
    pub(in crate::parser) fn normalized_value(&self, value: f64) -> f64 { self.atomic.and_then(|atomic| atomic.normalized_value(value)).unwrap_or(value * self.multiplier) }
}

fn inferred_quantity_kind(dimension: MeasurementDimension) -> MeasurementQuantityKind {
    [
        MeasurementQuantityKind::Length, MeasurementQuantityKind::Area, MeasurementQuantityKind::Volume, MeasurementQuantityKind::Mass,
        MeasurementQuantityKind::Time, MeasurementQuantityKind::Temperature, MeasurementQuantityKind::DataSize, MeasurementQuantityKind::DataRate,
        MeasurementQuantityKind::AmountOfSubstance, MeasurementQuantityKind::AmountConcentration, MeasurementQuantityKind::ActivePower,
        MeasurementQuantityKind::Voltage, MeasurementQuantityKind::ElectricCurrent, MeasurementQuantityKind::Frequency, MeasurementQuantityKind::Force,
        MeasurementQuantityKind::Pressure, MeasurementQuantityKind::DynamicViscosity, MeasurementQuantityKind::KinematicViscosity, MeasurementQuantityKind::Energy,
        MeasurementQuantityKind::ElectricalResistance, MeasurementQuantityKind::ElectricalConductance, MeasurementQuantityKind::ElectricCharge,
        MeasurementQuantityKind::Capacitance, MeasurementQuantityKind::Inductance, MeasurementQuantityKind::MagneticFlux, MeasurementQuantityKind::MagneticFluxDensity,
        MeasurementQuantityKind::SpectralFluxDensity, MeasurementQuantityKind::Speed, MeasurementQuantityKind::Acceleration, MeasurementQuantityKind::MassConcentration,
        MeasurementQuantityKind::VolumeRate, MeasurementQuantityKind::TemperatureRate, MeasurementQuantityKind::Ratio,
    ]
    .into_iter()
    .find(|kind| measurement_dimension(*kind) == dimension)
    .unwrap_or(MeasurementQuantityKind::Unspecified)
}

#[derive(Clone, Debug)]
struct MeasurementUnitFactor {
    base: BaseUnit,
    exponent: i8,
    canonical: String,
    dimension: MeasurementDimension,
    quantity_kind: MeasurementQuantityKind,
    multiplier: f64,
    precision_limited: bool,
}

impl MeasurementUnitFactor {
    fn resolved(text: &str, language: Option<ParseLanguage>) -> Option<Self> {
        let (stem, exponent) = split_measurement_exponent(text);
        let (prefix, base) = parse_unit_symbol(stem)
            .or_else(|| language.and_then(|language| parse_unit_alias(stem, language)))
            .or_else(|| (stem == "B").then_some((None, BaseUnit::Byte)))?;
        if language.is_some_and(|language| !unit_spelling_allowed_in_language(stem, language)) {
            return None;
        }
        let atomic = AtomicMeasurementUnit::new(prefix, base);
        let dimension = atomic.dimension()?.pow(exponent);
        let quantity_kind = atomic.quantity_kind()?;
        let multiplier = atomic.multiplier()?.powi(exponent.into());
        let canonical = format!("{}{}", atomic.canonical(), canonical_exponent(exponent));
        let precision_limited = atomic.precision_limited();
        Some(Self { base, exponent, canonical, dimension, quantity_kind, multiplier, precision_limited })
    }
}

fn split_measurement_exponent(text: &str) -> (&str, i8) {
    if let Some(stem) = text.strip_suffix('²') { return (stem, 2); }
    if let Some(stem) = text.strip_suffix('³') { return (stem, 3); }
    if let Some(stem) = text.strip_suffix('2').filter(|stem| parse_unit_symbol(stem).is_some()) { return (stem, 2); }
    if let Some(stem) = text.strip_suffix('3').filter(|stem| parse_unit_symbol(stem).is_some()) { return (stem, 3); }
    (text, 1)
}

fn canonical_exponent(exponent: i8) -> &'static str {
    match exponent {
        2 => "²",
        3 => "³",
        _ => "",
    }
}

fn compound_measurement_parts(text: &str) -> Option<Vec<(&str, bool)>> {
    let separators = text.match_indices(['/', '·', '⋅', '*']).collect::<Vec<_>>();
    if separators.is_empty() && !matches!(text.chars().last(), Some('²' | '³' | '2' | '3')) {
        return None;
    }
    let mut denominator = false;
    let mut start = 0;
    let mut parts = Vec::with_capacity(separators.len() + 1);
    for (index, separator) in separators {
        let part = text.get(start..index)?;
        if part.is_empty() {
            return None;
        }
        parts.push((part, denominator));
        denominator |= separator == "/";
        start = index + separator.len();
    }
    let part = text.get(start..)?;
    (!part.is_empty()).then(|| {
        parts.push((part, denominator));
        parts
    })
}

pub(in crate::parser) fn resolve_compound_measurement_unit(text: &str, language: Option<ParseLanguage>) -> Option<ResolvedMeasurementUnit> {
    let factors = compound_measurement_parts(text)?
        .into_iter()
        .map(|(text, denominator)| MeasurementUnitFactor::resolved(text, language).map(|factor| (factor, denominator)))
        .collect::<Option<Vec<_>>>()?;
    let dimension = factors.iter().fold(MeasurementDimension::default(), |dimension, (factor, denominator)| {
        if *denominator { dimension.quotient(factor.dimension) } else { dimension.product(factor.dimension) }
    });
    let multiplier = factors.iter().fold(1.0, |multiplier, (factor, denominator)| {
        if *denominator { multiplier / factor.multiplier } else { multiplier * factor.multiplier }
    });
    let precision_limited = factors.iter().any(|(factor, _)| factor.precision_limited);
    let numerator = factors.iter().filter(|(_, denominator)| !denominator).map(|(factor, _)| factor.canonical.as_str()).collect::<Vec<_>>().join("·");
    let denominator = factors.iter().filter(|(_, denominator)| *denominator).map(|(factor, _)| factor.canonical.as_str()).collect::<Vec<_>>().join("·");
    let canonical = if denominator.is_empty() { numerator } else { format!("{numerator}/{denominator}") };
    let quantity_kind = inferred_compound_quantity_kind(&factors, dimension);
    Some(ResolvedMeasurementUnit::compound(canonical, dimension, quantity_kind, multiplier, precision_limited))
}

fn compound_factors_match(factors: &[(MeasurementUnitFactor, bool)], expected: &[(BaseUnit, i8, bool)]) -> bool {
    factors.len() == expected.len()
        && factors.iter().zip(expected).all(|((factor, denominator), (base, exponent, expected_denominator))| factor.base == *base && factor.exponent == *exponent && denominator == expected_denominator)
}

fn inferred_compound_quantity_kind(factors: &[(MeasurementUnitFactor, bool)], dimension: MeasurementDimension) -> MeasurementQuantityKind {
    let exact = [
        (&[(BaseUnit::Newton, 1, false), (BaseUnit::Meter, 1, false)][..], MeasurementQuantityKind::Torque),
        (&[(BaseUnit::Pascal, 1, false), (BaseUnit::Second, 1, false)][..], MeasurementQuantityKind::DynamicViscosity),
        (&[(BaseUnit::Newton, 1, false), (BaseUnit::Meter, 1, true)][..], MeasurementQuantityKind::ForcePerLength),
        (&[(BaseUnit::Radian, 1, false), (BaseUnit::Second, 2, true)][..], MeasurementQuantityKind::AngularAcceleration),
        (&[(BaseUnit::Watt, 1, false), (BaseUnit::Meter, 2, true)][..], MeasurementQuantityKind::PowerPerArea),
        (&[(BaseUnit::Joule, 1, false), (BaseUnit::Kelvin, 1, true)][..], MeasurementQuantityKind::HeatCapacity),
        (&[(BaseUnit::Joule, 1, false), (BaseUnit::Gram, 1, true), (BaseUnit::Kelvin, 1, true)][..], MeasurementQuantityKind::SpecificHeatCapacity),
        (&[(BaseUnit::Joule, 1, false), (BaseUnit::Gram, 1, true)][..], MeasurementQuantityKind::SpecificEnergy),
        (&[(BaseUnit::Calorie, 1, false), (BaseUnit::Gram, 1, true)][..], MeasurementQuantityKind::SpecificEnergy),
        (&[(BaseUnit::Watt, 1, false), (BaseUnit::Meter, 1, true), (BaseUnit::Kelvin, 1, true)][..], MeasurementQuantityKind::ThermalConductivity),
        (&[(BaseUnit::Joule, 1, false), (BaseUnit::Meter, 3, true)][..], MeasurementQuantityKind::EnergyDensity),
        (&[(BaseUnit::Volt, 1, false), (BaseUnit::Meter, 1, true)][..], MeasurementQuantityKind::ElectricFieldStrength),
        (&[(BaseUnit::Coulomb, 1, false), (BaseUnit::Meter, 3, true)][..], MeasurementQuantityKind::VolumeChargeDensity),
        (&[(BaseUnit::Coulomb, 1, false), (BaseUnit::Meter, 2, true)][..], MeasurementQuantityKind::SurfaceChargeDensity),
        (&[(BaseUnit::Farad, 1, false), (BaseUnit::Meter, 1, true)][..], MeasurementQuantityKind::Permittivity),
        (&[(BaseUnit::Henry, 1, false), (BaseUnit::Meter, 1, true)][..], MeasurementQuantityKind::Permeability),
        (&[(BaseUnit::Joule, 1, false), (BaseUnit::Mole, 1, true)][..], MeasurementQuantityKind::MolarEnergy),
        (&[(BaseUnit::Joule, 1, false), (BaseUnit::Mole, 1, true), (BaseUnit::Kelvin, 1, true)][..], MeasurementQuantityKind::MolarHeatCapacity),
        (&[(BaseUnit::Watt, 1, false), (BaseUnit::Steradian, 1, true)][..], MeasurementQuantityKind::RadiantIntensity),
        (&[(BaseUnit::Watt, 1, false), (BaseUnit::Meter, 2, true), (BaseUnit::Steradian, 1, true)][..], MeasurementQuantityKind::Radiance),
        (&[(BaseUnit::Liter, 1, false), (BaseUnit::Gram, 1, true)][..], MeasurementQuantityKind::SpecificVolume),
        (&[(BaseUnit::Meter, 3, false), (BaseUnit::Gram, 1, true)][..], MeasurementQuantityKind::SpecificVolume),
        (&[(BaseUnit::Ampere, 1, false), (BaseUnit::Meter, 2, true)][..], MeasurementQuantityKind::ElectricCurrentDensity),
        (&[(BaseUnit::Ampere, 1, false), (BaseUnit::Meter, 1, true)][..], MeasurementQuantityKind::MagneticFieldStrength),
        (&[(BaseUnit::Candela, 1, false), (BaseUnit::Meter, 2, true)][..], MeasurementQuantityKind::Luminance),
        (&[(BaseUnit::CatalyticActivity, 1, false), (BaseUnit::Meter, 3, true)][..], MeasurementQuantityKind::CatalyticActivityConcentration),
        (&[(BaseUnit::Gram, 1, false), (BaseUnit::Gram, 1, true)][..], MeasurementQuantityKind::MassFraction),
        (&[(BaseUnit::Gram, 1, false), (BaseUnit::Meter, 2, true)][..], MeasurementQuantityKind::ArealMassDensity),
        (&[(BaseUnit::Equivalent, 1, false), (BaseUnit::Liter, 1, true)][..], MeasurementQuantityKind::ChemicalEquivalentConcentration),
        (&[(BaseUnit::Equivalent, 1, false), (BaseUnit::Meter, 3, true)][..], MeasurementQuantityKind::ChemicalEquivalentConcentration),
        (&[(BaseUnit::Osmole, 1, false), (BaseUnit::Liter, 1, true)][..], MeasurementQuantityKind::OsmoticConcentration),
        (&[(BaseUnit::Osmole, 1, false), (BaseUnit::Meter, 3, true)][..], MeasurementQuantityKind::OsmoticConcentration),
    ]
    .into_iter()
    .find_map(|(expected, kind)| compound_factors_match(factors, expected).then_some(kind));
    if let Some(kind) = exact {
        return kind;
    }
    let timed = factors.iter().any(|(factor, denominator)| *denominator && factor.quantity_kind == MeasurementQuantityKind::Time);
    let mass_denominated = factors.iter().any(|(factor, denominator)| *denominator && factor.quantity_kind == MeasurementQuantityKind::Mass);
    let numerator_kind = factors.iter().find(|(_, denominator)| !denominator).map(|(factor, _)| factor.quantity_kind);
    match (numerator_kind, timed, mass_denominated) {
        (Some(MeasurementQuantityKind::ElectricCharge), false, true) => MeasurementQuantityKind::RadiationExposure,
        (Some(MeasurementQuantityKind::ElectricCharge), true, true) => MeasurementQuantityKind::RadiationExposureRate,
        (Some(MeasurementQuantityKind::PixelCount), true, _) => MeasurementQuantityKind::PixelRate,
        (Some(MeasurementQuantityKind::Angle), true, _) => MeasurementQuantityKind::AngularVelocity,
        (Some(MeasurementQuantityKind::NormalizedVolume), true, _) => MeasurementQuantityKind::NormalizedVolumeRate,
        (Some(MeasurementQuantityKind::AbsorbedDose), true, _) => MeasurementQuantityKind::AbsorbedDoseRate,
        (Some(MeasurementQuantityKind::EquivalentDose), true, _) => MeasurementQuantityKind::EquivalentDoseRate,
        (Some(MeasurementQuantityKind::RadiationExposure), true, _) => MeasurementQuantityKind::RadiationExposureRate,
        _ => inferred_quantity_kind(dimension),
    }
}


