use serde::{Deserialize, Serialize};

use super::{Node, NodeRef, NodeSemantic, SegmentTree, WhitespaceTypographyFact};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NumericTypographySpacing {
    #[default]
    Attached,
    Breaking,
    NonBreaking,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NumberRangeTypography {
    separator_region_start: u16,
    separator_region_end: u16,
    separator: char,
    before_spacing: NumericTypographySpacing,
    after_spacing: NumericTypographySpacing,
    preferred_separator: char,
}

impl NumberRangeTypography {
    pub fn new(separator_region_start: u16, separator_region_end: u16, separator: char, before_spacing: NumericTypographySpacing, after_spacing: NumericTypographySpacing, preferred_separator: char) -> Self { Self { separator_region_start, separator_region_end, separator, before_spacing, after_spacing, preferred_separator } }
    pub fn separator_region_start(self) -> usize { self.separator_region_start as usize }
    pub fn separator_region_end(self) -> usize { self.separator_region_end as usize }
    pub fn separator(self) -> char { self.separator }
    pub fn preferred_separator(self) -> char { self.preferred_separator }
    pub fn is_canonical(self) -> bool { self.separator == self.preferred_separator && self.before_spacing == NumericTypographySpacing::Attached && self.after_spacing == NumericTypographySpacing::Attached }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NumberFormat {
    decimal_separator: Option<char>,
    encoded: [u8; 4],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum NumberFormatAssessment {
    #[default]
    Unknown,
    Canonical,
    Accepted,
    Foreign,
    Invalid,
    Ambiguous,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NumberFormatBuilder {
    decimal_separator: Option<char>,
    grouping_separator: Option<char>,
    locale_decimal_separator: Option<char>,
    locale_grouping_separator: Option<char>,
    fraction_digits: Option<u8>,
    assessment: NumberFormatAssessment,
}

impl NumberFormat {
    pub fn builder() -> NumberFormatBuilder { NumberFormatBuilder::default() }
    pub fn decimal_separator(&self) -> Option<char> { self.decimal_separator }
    pub fn grouping_separator(&self) -> Option<char> { decode_number_separator(self.encoded[0]) }
    pub fn locale_decimal_separator(&self) -> Option<char> { decode_number_separator(self.encoded[1]) }
    pub fn locale_grouping_separator(&self) -> Option<char> { decode_number_separator(self.encoded[2]) }
    pub fn fraction_digits(&self) -> Option<u8> { let encoded = self.encoded[3] & 0x1F; (encoded != 0x1F).then_some(encoded) }
    pub fn assessment(&self) -> NumberFormatAssessment { decode_number_format_assessment(self.encoded[3] >> 5) }
    pub fn decimal_separator_matches_locale(&self) -> Option<bool> { Some(self.decimal_separator()? == self.locale_decimal_separator()?) }
    pub fn requires_locale_change(&self) -> bool { matches!(self.assessment(), NumberFormatAssessment::Foreign | NumberFormatAssessment::Invalid) }
}

impl Default for NumberFormat {
    fn default() -> Self { Self::builder().build() }
}

impl NumberFormatBuilder {
    pub fn decimal_separator(mut self, separator: Option<char>) -> Self { self.decimal_separator = separator; self }
    pub fn grouping_separator(mut self, separator: Option<char>) -> Self { self.grouping_separator = separator; self }
    pub fn locale_decimal_separator(mut self, separator: Option<char>) -> Self { self.locale_decimal_separator = separator; self }
    pub fn locale_grouping_separator(mut self, separator: Option<char>) -> Self { self.locale_grouping_separator = separator; self }
    pub fn fraction_digits(mut self, digits: Option<u8>) -> Self { self.fraction_digits = digits; self }
    pub fn assessment(mut self, assessment: NumberFormatAssessment) -> Self { self.assessment = assessment; self }
    pub fn build(self) -> NumberFormat {
        let fraction_digits = self.fraction_digits.map(|digits| digits.min(0x1E)).unwrap_or(0x1F);
        NumberFormat {
            decimal_separator: self.decimal_separator,
            encoded: [
                encode_number_separator(self.grouping_separator),
                encode_number_separator(self.locale_decimal_separator),
                encode_number_separator(self.locale_grouping_separator),
                encode_number_format_assessment(self.assessment) << 5 | fraction_digits,
            ],
        }
    }
}

fn encode_number_separator(separator: Option<char>) -> u8 {
    match separator {
        None => 0,
        Some('.') => 1,
        Some(',') => 2,
        Some(' ') => 3,
        Some('\u{00A0}') => 4,
        Some('\u{202F}') => 5,
        Some('\'') => 6,
        Some('’') => 7,
        Some('\u{2009}') => 8,
        Some(_) => 0,
    }
}

fn decode_number_separator(encoded: u8) -> Option<char> {
    match encoded {
        1 => Some('.'),
        2 => Some(','),
        3 => Some(' '),
        4 => Some('\u{00A0}'),
        5 => Some('\u{202F}'),
        6 => Some('\''),
        7 => Some('’'),
        8 => Some('\u{2009}'),
        _ => None,
    }
}

fn encode_number_format_assessment(assessment: NumberFormatAssessment) -> u8 {
    match assessment {
        NumberFormatAssessment::Unknown => 0,
        NumberFormatAssessment::Canonical => 1,
        NumberFormatAssessment::Accepted => 2,
        NumberFormatAssessment::Foreign => 3,
        NumberFormatAssessment::Invalid => 4,
        NumberFormatAssessment::Ambiguous => 5,
    }
}

fn decode_number_format_assessment(encoded: u8) -> NumberFormatAssessment {
    match encoded {
        1 => NumberFormatAssessment::Canonical,
        2 => NumberFormatAssessment::Accepted,
        3 => NumberFormatAssessment::Foreign,
        4 => NumberFormatAssessment::Invalid,
        5 => NumberFormatAssessment::Ambiguous,
        _ => NumberFormatAssessment::Unknown,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OrdinalStyle {
    EnglishSuffix,
    SwedishColonSuffix,
    GermanPeriod,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OrdinalAssessment {
    Canonical,
    Foreign,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OrdinalFact {
    value: u32,
    style: OrdinalStyle,
    preferred_style: OrdinalStyle,
}

impl OrdinalFact {
    pub fn new(value: u32, style: OrdinalStyle, preferred_style: OrdinalStyle) -> Self { Self { value, style, preferred_style } }
    pub fn value(self) -> u32 { self.value }
    pub fn style(self) -> OrdinalStyle { self.style }
    pub fn preferred_style(self) -> OrdinalStyle { self.preferred_style }
    pub fn assessment(self) -> OrdinalAssessment { if self.style == self.preferred_style { OrdinalAssessment::Canonical } else { OrdinalAssessment::Foreign } }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DateFact {
    year: u32,
    month: u8,
    day: u8,
}

impl DateFact {
    pub fn new(year: u32, month: u8, day: u8) -> Self { Self { year, month, day } }
    pub fn year(&self) -> u32 { self.year }
    pub fn month(&self) -> u8 { self.month }
    pub fn day(&self) -> u8 { self.day }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TimeFact {
    hour: u8,
    minute: u8,
    second: Option<u8>,
}

impl TimeFact {
    pub fn new(hour: u8, minute: u8, second: Option<u8>) -> Self { Self { hour, minute, second } }
    pub fn hour(&self) -> u8 { self.hour }
    pub fn minute(&self) -> u8 { self.minute }
    pub fn second(&self) -> Option<u8> { self.second }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MeasurementQuantityKind {
    Unspecified,
    Length,
    Area,
    Volume,
    NormalizedVolume,
    NormalizedVolumeRate,
    Mass,
    Time,
    Temperature,
    DataSize,
    DataRate,
    AmountOfSubstance,
    AmountConcentration,
    ActivePower,
    ApparentPower,
    ReactivePower,
    Voltage,
    ElectricCurrent,
    Frequency,
    Radioactivity,
    Force,
    Pressure,
    Energy,
    ElectricalResistance,
    ElectricalConductance,
    ElectricCharge,
    Capacitance,
    Inductance,
    MagneticFlux,
    MagneticFluxDensity,
    Speed,
    Acceleration,
    MassConcentration,
    VolumeRate,
    TemperatureRate,
    Angle,
    AngularVelocity,
    Ratio,
    Count,
    CountRate,
    FrameRate,
    ComputationRate,
    LogarithmicLevel,
    SpatialFrequency,
    AbsorbedDose,
    AbsorbedDoseRate,
    EquivalentDose,
    EquivalentDoseRate,
    RadiationExposure,
    RadiationExposureRate,
    PixelCount,
    PixelRate,
    AlternatingCurrentVoltage,
    LuminousIntensity,
    LuminousFlux,
    Illuminance,
    SolidAngle,
    CatalyticActivity,
    CatalyticActivityConcentration,
    NeperLevel,
    Torque,
    DynamicViscosity,
    ForcePerLength,
    AngularAcceleration,
    PowerPerArea,
    HeatCapacity,
    SpecificHeatCapacity,
    SpecificEnergy,
    ThermalConductivity,
    EnergyDensity,
    ElectricFieldStrength,
    VolumeChargeDensity,
    SurfaceChargeDensity,
    Permittivity,
    Permeability,
    MolarEnergy,
    MolarHeatCapacity,
    RadiantIntensity,
    Radiance,
    SpecificVolume,
    ElectricCurrentDensity,
    MagneticFieldStrength,
    Luminance,
    MassFraction,
    ArealMassDensity,
    KinematicViscosity,
    SpectralFluxDensity,
    ChemicalEquivalentAmount,
    ChemicalEquivalentConcentration,
    OsmoticAmount,
    OsmoticConcentration,
}

impl Default for MeasurementQuantityKind {
    fn default() -> Self { Self::Unspecified }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MeasurementDimension {
    exponents: [i8; 7],
}

#[allow(non_upper_case_globals)]
impl MeasurementDimension {
    pub const Length: Self = Self::new([1, 0, 0, 0, 0, 0, 0]);
    pub const Mass: Self = Self::new([0, 1, 0, 0, 0, 0, 0]);
    pub const Volume: Self = Self::new([3, 0, 0, 0, 0, 0, 0]);
    pub const Time: Self = Self::new([0, 0, 1, 0, 0, 0, 0]);
    pub const Temperature: Self = Self::new([0, 0, 0, 0, 1, 0, 0]);
    pub const Data: Self = Self::new([0, 0, 0, 0, 0, 0, 1]);
    pub const Power: Self = Self::new([2, 1, -3, 0, 0, 0, 0]);
    pub const Voltage: Self = Self::new([2, 1, -3, -1, 0, 0, 0]);
    pub const Current: Self = Self::new([0, 0, 0, 1, 0, 0, 0]);
    pub const Frequency: Self = Self::new([0, 0, -1, 0, 0, 0, 0]);
    pub const AmountOfSubstance: Self = Self::new([0, 0, 0, 0, 0, 1, 0]);

    const fn new(exponents: [i8; 7]) -> Self { Self { exponents } }
    pub fn product(self, other: Self) -> Self { Self::new(std::array::from_fn(|index| self.exponents[index].saturating_add(other.exponents[index]))) }
    pub fn quotient(self, other: Self) -> Self { Self::new(std::array::from_fn(|index| self.exponents[index].saturating_sub(other.exponents[index]))) }
    pub fn pow(self, exponent: i8) -> Self { Self::new(self.exponents.map(|value| value.saturating_mul(exponent))) }
    pub fn exponents(&self) -> &[i8; 7] { &self.exponents }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MeasurementValueFact {
    dimension: MeasurementDimension,
    amount: i128,
    #[serde(default)]
    quantity_kind: MeasurementQuantityKind,
}

impl MeasurementValueFact {
    pub fn new(dimension: MeasurementDimension, amount: i128) -> Self { Self { dimension, amount, quantity_kind: MeasurementQuantityKind::Unspecified } }
    pub fn with_quantity_kind(mut self, quantity_kind: MeasurementQuantityKind) -> Self { self.quantity_kind = quantity_kind; self }
    pub fn dimension(&self) -> MeasurementDimension { self.dimension }
    pub fn amount(&self) -> i128 { self.amount }
    pub fn quantity_kind(&self) -> MeasurementQuantityKind { self.quantity_kind }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MeasurementDegreeStyle {
    #[default]
    NotApplicable,
    Canonical,
    Variant,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MeasurementTypography {
    separator_start: u16,
    unit_start: u16,
    unit_end: u16,
    whitespace: WhitespaceTypographyFact,
    degree_style: MeasurementDegreeStyle,
}

impl MeasurementTypography {
    pub fn new(separator_start: u16, unit_start: u16, unit_end: u16, whitespace: WhitespaceTypographyFact, degree_style: MeasurementDegreeStyle) -> Self { Self { separator_start, unit_start, unit_end, whitespace, degree_style } }
    pub fn separator_start(&self) -> usize { self.separator_start as usize }
    pub fn unit_start(&self) -> usize { self.unit_start as usize }
    pub fn unit_end(&self) -> usize { self.unit_end as usize }
    pub fn whitespace_fact(&self) -> WhitespaceTypographyFact { self.whitespace }
    pub fn degree_style(&self) -> MeasurementDegreeStyle { self.degree_style }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NumberSemantic {
    value: i128,
    format: NumberFormat,
}

impl NumberSemantic {
    pub fn new(value: i128, format: NumberFormat) -> Self { Self { value, format } }
    pub fn value(&self) -> i128 { self.value }
    pub fn format(&self) -> NumberFormat { self.format }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NumberRangeSemantic {
    data: Box<NumberRangeSemanticData>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct NumberRangeSemanticData {
    start_value: i128,
    end_value: i128,
    typography: NumberRangeTypography,
}

impl NumberRangeSemantic {
    pub fn new(start_value: i128, end_value: i128, typography: NumberRangeTypography) -> Self { Self { data: Box::new(NumberRangeSemanticData { start_value, end_value, typography }) } }
    pub fn start_value(&self) -> i128 { self.data.start_value }
    pub fn end_value(&self) -> i128 { self.data.end_value }
    pub fn typography(&self) -> NumberRangeTypography { self.data.typography }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeasurementSemantic {
    data: Box<MeasurementSemanticData>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct MeasurementSemanticData {
    unit: String,
    values: Vec<MeasurementValueFact>,
    number_values: Vec<i128>,
    number_formats: Vec<NumberFormat>,
    #[serde(default)]
    typography: MeasurementTypography,
    #[serde(default)]
    number_range_typography: Option<NumberRangeTypography>,
}

impl MeasurementSemantic {
    pub fn new(unit: String, values: Vec<MeasurementValueFact>, number_values: Vec<i128>, number_formats: Vec<NumberFormat>) -> Self { Self { data: Box::new(MeasurementSemanticData { unit, values, number_values, number_formats, typography: MeasurementTypography::default(), number_range_typography: None }) } }
    pub fn with_typography(mut self, typography: MeasurementTypography) -> Self { self.data.typography = typography; self }
    pub fn with_number_range_typography(mut self, typography: NumberRangeTypography) -> Self { self.data.number_range_typography = Some(typography); self }
    pub fn unit(&self) -> &str { &self.data.unit }
    pub fn values(&self) -> &[MeasurementValueFact] { &self.data.values }
    pub fn number_values(&self) -> &[i128] { &self.data.number_values }
    pub fn number_formats(&self) -> &[NumberFormat] { &self.data.number_formats }
    pub fn typography(&self) -> MeasurementTypography { self.data.typography }
    pub fn number_range_typography(&self) -> Option<NumberRangeTypography> { self.data.number_range_typography }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurrencySemantic {
    data: Box<CurrencySemanticData>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct CurrencySemanticData {
    currency: Option<String>,
    amount: i128,
    #[serde(default)]
    format: NumberFormat,
}

impl CurrencySemantic {
    pub fn new(currency: String, amount: i128, format: NumberFormat) -> Self { Self { data: Box::new(CurrencySemanticData { currency: Some(currency), amount, format }) } }
    pub fn ambiguous(amount: i128, format: NumberFormat) -> Self { Self { data: Box::new(CurrencySemanticData { currency: None, amount, format }) } }
    pub fn currency(&self) -> Option<&str> { self.data.currency.as_deref() }
    pub fn amount(&self) -> i128 { self.data.amount }
    pub fn format(&self) -> NumberFormat { self.data.format }
}

impl NodeSemantic {
    pub(in crate::tree) fn number_values(&self) -> &[i128] {
        match self {
            Self::Number(number) => std::slice::from_ref(&number.value),
            Self::Measurement(measurement) => measurement.number_values(),
            Self::Currency(currency) => std::slice::from_ref(&currency.data.amount),
            _ => &[],
        }
    }

    pub(in crate::tree) fn number_formats(&self) -> &[NumberFormat] {
        match self {
            Self::Number(number) => std::slice::from_ref(&number.format),
            Self::Measurement(measurement) => measurement.number_formats(),
            Self::Currency(currency) => std::slice::from_ref(&currency.data.format),
            _ => &[],
        }
    }
    pub(in crate::tree) fn date_fact(&self) -> Option<DateFact> { match self { Self::Date(date) => Some(*date), _ => None } }
    pub(in crate::tree) fn time_fact(&self) -> Option<TimeFact> { match self { Self::Time(time) => Some(*time), _ => None } }
    pub(in crate::tree) fn ordinal_fact(&self) -> Option<OrdinalFact> { match self { Self::Ordinal(ordinal) => Some(*ordinal), _ => None } }
    pub(in crate::tree) fn currency_fact(&self) -> Option<&CurrencySemantic> { match self { Self::Currency(currency) => Some(currency), _ => None } }
    pub(in crate::tree) fn measurement_fact(&self) -> Option<&MeasurementSemantic> { match self { Self::Measurement(measurement) => Some(measurement), _ => None } }
}

impl Node {
    pub fn number_values(&self) -> &[i128] { self.semantic().map(NodeSemantic::number_values).unwrap_or_default() }
    pub fn number_formats(&self) -> &[NumberFormat] { self.semantic().map(NodeSemantic::number_formats).unwrap_or_default() }
    pub fn first_number_format(&self) -> Option<NumberFormat> { self.number_formats().first().copied() }
    pub fn date_fact(&self) -> Option<DateFact> { self.semantic().and_then(NodeSemantic::date_fact) }
    pub fn time_fact(&self) -> Option<TimeFact> { self.semantic().and_then(NodeSemantic::time_fact) }
    pub fn ordinal_fact(&self) -> Option<OrdinalFact> { self.semantic().and_then(NodeSemantic::ordinal_fact) }
    pub fn currency_fact(&self) -> Option<&CurrencySemantic> { self.semantic().and_then(NodeSemantic::currency_fact) }
    pub fn measurement_fact(&self) -> Option<&MeasurementSemantic> { self.semantic().and_then(NodeSemantic::measurement_fact) }
}

impl<'a> NodeRef<'a> {
    pub fn number_values(&self) -> &'a [i128] { self.semantic().map(NodeSemantic::number_values).unwrap_or_default() }
    pub fn number_formats(&self) -> &'a [NumberFormat] { self.semantic().map(NodeSemantic::number_formats).unwrap_or_default() }
    pub fn first_number_format(&self) -> Option<NumberFormat> { self.number_formats().first().copied() }
    pub fn date_fact(&self) -> Option<DateFact> { self.semantic().and_then(NodeSemantic::date_fact) }
    pub fn time_fact(&self) -> Option<TimeFact> { self.semantic().and_then(NodeSemantic::time_fact) }
    pub fn ordinal_fact(&self) -> Option<OrdinalFact> { self.semantic().and_then(NodeSemantic::ordinal_fact) }
    pub fn currency_fact(&self) -> Option<&'a CurrencySemantic> { self.semantic().and_then(NodeSemantic::currency_fact) }
    pub fn measurement_fact(&self) -> Option<&'a MeasurementSemantic> { self.semantic().and_then(NodeSemantic::measurement_fact) }
}

pub(super) fn collect_numeric_fact_addresses(tree: &SegmentTree) -> Box<[u16]> {
    let mut stack = tree.node_child_addresses(0).iter().rev().copied().collect::<Vec<_>>();
    let mut addresses = Vec::new();
    while let Some(node_address) = stack.pop() {
        let node = NodeRef::new(tree, node_address);
        if node.kind().is_numeric_fact() {
            addresses.push(node_address);
            continue;
        }
        stack.extend(tree.node_child_addresses(node_address).iter().rev().copied());
    }
    addresses.into_boxed_slice()
}
