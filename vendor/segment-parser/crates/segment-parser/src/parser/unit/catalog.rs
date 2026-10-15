use super::super::*;
use crate::language_parser::{unit_alias_is_language_scoped, UnitAliasTarget};
use crate::measurement_unit::BaseUnit;
use std::sync::LazyLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::parser) enum SiPrefix { Pico, Nano, Micro, Milli, Centi, Deci, Deca, Hecto, Kilo, Mega, Giga, Tera, Peta, Exa }

#[derive(Clone, Copy)]
struct PrefixDefinition {
    symbol: &'static str,
    word_index: usize,
    multiplier: f64,
    data: bool,
    limited: bool,
}

impl PrefixDefinition {
    const fn new(symbol: &'static str, word_index: usize, multiplier: f64, data: bool, limited: bool) -> Self { Self { symbol, word_index, multiplier, data, limited } }
}

impl SiPrefix {
    pub(in crate::parser) const ALL: &'static [SiPrefix] = &[Self::Pico, Self::Nano, Self::Micro, Self::Milli, Self::Centi, Self::Deci, Self::Deca, Self::Hecto, Self::Kilo, Self::Mega, Self::Giga, Self::Tera, Self::Peta, Self::Exa];

    fn definition(self) -> PrefixDefinition {
        match self {
            Self::Pico => PrefixDefinition::new("p", 0, 1e-12, false, false), Self::Nano => PrefixDefinition::new("n", 1, 1e-9, false, false), Self::Micro => PrefixDefinition::new("µ", 2, 1e-6, false, false),
            Self::Milli => PrefixDefinition::new("m", 3, 1e-3, false, true), Self::Centi => PrefixDefinition::new("c", 4, 1e-2, false, false), Self::Deci => PrefixDefinition::new("d", 5, 1e-1, false, false),
            Self::Deca => PrefixDefinition::new("da", 6, 1e1, false, false), Self::Hecto => PrefixDefinition::new("h", 7, 1e2, false, false), Self::Kilo => PrefixDefinition::new("k", 8, 1e3, true, true),
            Self::Mega => PrefixDefinition::new("M", 9, 1e6, true, true), Self::Giga => PrefixDefinition::new("G", 10, 1e9, true, true), Self::Tera => PrefixDefinition::new("T", 11, 1e12, true, false),
            Self::Peta => PrefixDefinition::new("P", 12, 1e15, true, false), Self::Exa => PrefixDefinition::new("E", 13, 1e18, true, false),
        }
    }

    /// SI symbols are case-sensitive: "m" is milli and "M" is mega.
    pub(in crate::parser) fn symbol(self) -> &'static str { self.definition().symbol }
    pub(in crate::parser) fn word_stem(self, language: ParseLanguage) -> &'static str { language.profile().unit_lexicon().prefix_stem(self.definition().word_index) }
    pub(in crate::parser) fn multiplier(self) -> f64 { self.definition().multiplier }
}

#[derive(Clone, Copy)]
enum PrefixPolicy { None, Si, Data, Horsepower, Temperature }

impl PrefixPolicy {
    fn accepts(self, prefix: SiPrefix) -> bool {
        match self {
            Self::None => false, Self::Si => true, Self::Data => prefix.definition().data,
            Self::Horsepower | Self::Temperature => prefix.definition().limited,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BareRecognition { Ambiguous, Unambiguous, PrefixRequired }

#[derive(Clone, Copy)]
pub(in crate::parser) enum UnitSemantics {
    Opaque,
    Linear { quantity_kind: MeasurementQuantityKind, multiplier: f64 },
    Affine { quantity_kind: MeasurementQuantityKind, multiplier: f64, input_offset: f64, reference_offset: f64 },
}

impl UnitSemantics {
    pub(in crate::parser) fn quantity_kind(self) -> Option<MeasurementQuantityKind> { match self { Self::Opaque => None, Self::Linear { quantity_kind, .. } | Self::Affine { quantity_kind, .. } => Some(quantity_kind) } }
    pub(in crate::parser) fn linear_multiplier(self) -> Option<f64> { match self { Self::Opaque => None, Self::Linear { multiplier, .. } | Self::Affine { multiplier, .. } => Some(multiplier) } }
    pub(in crate::parser) fn normalized_value(self, value: f64) -> Option<f64> {
        match self {
            Self::Opaque => None,
            Self::Linear { multiplier, .. } => Some(value * multiplier),
            Self::Affine { multiplier, input_offset, reference_offset, .. } => Some((value + input_offset) * multiplier + reference_offset),
        }
    }
}

#[derive(Clone, Copy)]
struct UnitDefinition {
    symbol: &'static str,
    extra_symbols: &'static [&'static str],
    prefix_policy: PrefixPolicy,
    bare_recognition: BareRecognition,
    semantics: UnitSemantics,
    precision_limited: bool,
}

impl UnitDefinition {
    const fn plain(symbol: &'static str) -> Self { Self { symbol, extra_symbols: &[], prefix_policy: PrefixPolicy::None, bare_recognition: BareRecognition::Unambiguous, semantics: UnitSemantics::Opaque, precision_limited: false } }
    const fn ambiguous(symbol: &'static str) -> Self { Self { bare_recognition: BareRecognition::Ambiguous, ..Self::plain(symbol) } }
    const fn si(symbol: &'static str) -> Self { Self { prefix_policy: PrefixPolicy::Si, bare_recognition: BareRecognition::Ambiguous, ..Self::plain(symbol) } }
    const fn si_unambiguous(symbol: &'static str) -> Self { Self { bare_recognition: BareRecognition::Unambiguous, ..Self::si(symbol) } }
    const fn data(symbol: &'static str, bare_recognition: BareRecognition) -> Self { Self { prefix_policy: PrefixPolicy::Data, bare_recognition, ..Self::plain(symbol) } }
    const fn prefixed(symbol: &'static str, prefix_policy: PrefixPolicy, bare_recognition: BareRecognition) -> Self { Self { prefix_policy, bare_recognition, ..Self::plain(symbol) } }
    const fn aliases(mut self, extra_symbols: &'static [&'static str]) -> Self { self.extra_symbols = extra_symbols; self }
    const fn linear(mut self, quantity_kind: MeasurementQuantityKind, multiplier: f64) -> Self { self.semantics = UnitSemantics::Linear { quantity_kind, multiplier }; self }
    const fn affine(mut self, quantity_kind: MeasurementQuantityKind, multiplier: f64, input_offset: f64, reference_offset: f64) -> Self { self.semantics = UnitSemantics::Affine { quantity_kind, multiplier, input_offset, reference_offset }; self }
    const fn precision_limited(mut self) -> Self { self.precision_limited = true; self }
}

/// All language-independent recognition metadata for a unit lives in this
/// catalog entry. Language-specific words remain in the language profiles.
impl BaseUnit {
    fn definition(self) -> UnitDefinition {
        use MeasurementQuantityKind as Q;
        match self {
            Self::Meter => UnitDefinition::si("m").linear(Q::Length, 1.0), Self::Angstrom => UnitDefinition::plain("Å").linear(Q::Length, 1e-10), Self::Gram => UnitDefinition::si("g").linear(Q::Mass, 1.0),
            Self::Second => UnitDefinition::si("s").aliases(&["sec"]).linear(Q::Time, 1.0), Self::Liter => UnitDefinition::si("l").aliases(&["L"]).linear(Q::Volume, 0.001),
            Self::Watt => UnitDefinition::si("W").linear(Q::ActivePower, 1_000.0), Self::Volt => UnitDefinition::si("V").linear(Q::Voltage, 1_000.0), Self::Ampere => UnitDefinition::si("A").linear(Q::ElectricCurrent, 1.0),
            Self::Hertz => UnitDefinition::si_unambiguous("Hz").linear(Q::Frequency, 1.0), Self::Byte => UnitDefinition::data("B", BareRecognition::PrefixRequired).linear(Q::DataSize, 1.0), Self::Bit => UnitDefinition::data("bit", BareRecognition::Unambiguous).linear(Q::DataSize, 1.0 / 8.0),
            Self::Mole => UnitDefinition::si_unambiguous("mol").linear(Q::AmountOfSubstance, 1.0), Self::Newton => UnitDefinition::si_unambiguous("N").linear(Q::Force, 1_000.0), Self::Pascal => UnitDefinition::si_unambiguous("Pa").linear(Q::Pressure, 1_000.0),
            Self::StandardAtmosphere => UnitDefinition::plain("atm").linear(Q::Pressure, 101_325_000.0), Self::Torr => UnitDefinition::plain("Torr").linear(Q::Pressure, 101_325_000.0 / 760.0), Self::Joule => UnitDefinition::si_unambiguous("J").linear(Q::Energy, 1_000.0),
            Self::Ohm => UnitDefinition::si_unambiguous("Ω").aliases(&["Ohm", "ohm"]).linear(Q::ElectricalResistance, 1_000.0), Self::Siemens => UnitDefinition::si_unambiguous("S").linear(Q::ElectricalConductance, 0.001), Self::Bar => UnitDefinition::si_unambiguous("bar").linear(Q::Pressure, 100_000_000.0),
            Self::Coulomb => UnitDefinition::si("C").linear(Q::ElectricCharge, 1.0), Self::Farad => UnitDefinition::si("F").linear(Q::Capacitance, 0.001), Self::Henry => UnitDefinition::si("H").linear(Q::Inductance, 1_000.0), Self::Weber => UnitDefinition::si_unambiguous("Wb").linear(Q::MagneticFlux, 1_000.0), Self::Tesla => UnitDefinition::si("T").linear(Q::MagneticFluxDensity, 1_000.0),
            Self::Calorie => UnitDefinition::si_unambiguous("cal").linear(Q::Energy, 4_184.0), Self::WattHour => UnitDefinition::si_unambiguous("Wh").linear(Q::Energy, 3_600_000.0), Self::AmpereHour => UnitDefinition::si_unambiguous("Ah").linear(Q::ElectricCharge, 3_600.0),
            Self::VoltAmpere => UnitDefinition::si_unambiguous("VA").linear(Q::ApparentPower, 1_000.0), Self::VoltAmpereReactive => UnitDefinition::si_unambiguous("VAr").linear(Q::ReactivePower, 1_000.0), Self::ElectricWatt => UnitDefinition::si_unambiguous("We").linear(Q::ActivePower, 1_000.0),
            Self::Gray => UnitDefinition::si_unambiguous("Gy").linear(Q::AbsorbedDose, 1.0), Self::Sievert => UnitDefinition::si_unambiguous("Sv").linear(Q::EquivalentDose, 1.0), Self::Roentgen => UnitDefinition::si("R").linear(Q::RadiationExposure, 0.000_000_258), Self::Becquerel => UnitDefinition::si_unambiguous("Bq").linear(Q::Radioactivity, 1.0),
            Self::Candela => UnitDefinition::si_unambiguous("cd").linear(Q::LuminousIntensity, 1.0), Self::Lumen => UnitDefinition::si_unambiguous("lm").linear(Q::LuminousFlux, 1.0), Self::Lux => UnitDefinition::si_unambiguous("lx").linear(Q::Illuminance, 1.0), Self::Radian => UnitDefinition::si_unambiguous("rad").linear(Q::Angle, 1.0), Self::Steradian => UnitDefinition::si_unambiguous("sr").linear(Q::SolidAngle, 1.0),
            Self::Decibel => UnitDefinition::plain("dB").linear(Q::LogarithmicLevel, 1.0), Self::AWeightedDecibel => UnitDefinition::plain("dBA").linear(Q::LogarithmicLevel, 1.0), Self::AlternatingCurrentVolt => UnitDefinition::plain("VAC").linear(Q::AlternatingCurrentVoltage, 1_000.0),
            Self::NormalLitre => UnitDefinition::plain("Nl").linear(Q::NormalizedVolume, 0.001), Self::PieceCount => UnitDefinition::plain("pcs").linear(Q::Count, 1.0), Self::GreatGross => UnitDefinition::plain("ggr").linear(Q::Count, 1_728.0), Self::Pixel => UnitDefinition::plain("px").linear(Q::PixelCount, 1.0), Self::Megapixel => UnitDefinition::plain("MP").linear(Q::PixelCount, 1_000_000.0),
            Self::CountsPerMinute => UnitDefinition::plain("ct/min").linear(Q::CountRate, 1.0 / 60.0), Self::FramesPerSecond => UnitDefinition::plain("fr/s").aliases(&["f/s"]).linear(Q::FrameRate, 1.0), Self::Flops => UnitDefinition::si_unambiguous("FLOPS").linear(Q::ComputationRate, 1.0), Self::LinePairsPerMillimetre => UnitDefinition::plain("LP/mm").linear(Q::SpatialFrequency, 1_000.0),
            Self::Celsius => UnitDefinition::plain("°C").affine(Q::Temperature, 1.0, 0.0, 273.15), Self::Fahrenheit => UnitDefinition::plain("°F").aliases(&["℉"]).affine(Q::Temperature, 5.0 / 9.0, -32.0, 273.15), Self::Percent => UnitDefinition::plain("%").linear(Q::Ratio, 0.01), Self::PartsPerMillion => UnitDefinition::plain("ppm").linear(Q::Ratio, 0.000_001),
            Self::Minute => UnitDefinition::plain("min").linear(Q::Time, 60.0), Self::Hour => UnitDefinition::plain("h").aliases(&["hr"]).linear(Q::Time, 3_600.0), Self::Day => UnitDefinition::ambiguous("d").linear(Q::Time, 86_400.0), Self::JulianYear => UnitDefinition::ambiguous("a").linear(Q::Time, 31_557_600.0), Self::BeatsPerMinute => UnitDefinition::plain("bpm").linear(Q::Frequency, 1.0 / 60.0),
            Self::Foot => UnitDefinition::plain("ft").linear(Q::Length, 0.3048), Self::Inch => UnitDefinition::plain("in").linear(Q::Length, 0.0254), Self::Yard => UnitDefinition::plain("yd").linear(Q::Length, 0.9144), Self::Mile => UnitDefinition::plain("mi").linear(Q::Length, 1_609.344), Self::NauticalMile => UnitDefinition::plain("nmi").linear(Q::Length, 1_852.0), Self::Knot => UnitDefinition::plain("kn").linear(Q::Speed, 1_852.0 / 3_600.0),
            Self::Pound => UnitDefinition::plain("lb").aliases(&["lbs"]).linear(Q::Mass, 453.59237), Self::Ounce => UnitDefinition::plain("oz").linear(Q::Mass, 28.349_523_125), Self::Grain => UnitDefinition::plain("gr").linear(Q::Mass, 0.064_798_91), Self::Point => UnitDefinition::plain("pt").linear(Q::Length, 0.0254 / 72.0), Self::Em => UnitDefinition::plain("em"),
            Self::Kelvin => UnitDefinition::prefixed("K", PrefixPolicy::Temperature, BareRecognition::Unambiguous).aliases(&["K"]).linear(Q::Temperature, 1.0), Self::PoundForcePerSquareInch => UnitDefinition::plain("psi").linear(Q::Pressure, 6_894_757.293_168),
            Self::ElectronVolt => UnitDefinition::si_unambiguous("eV").linear(Q::Energy, 1.602_176_634e-16).precision_limited(), Self::Dalton => UnitDefinition::si_unambiguous("Da").linear(Q::Mass, 1.660_539_068_92e-21).precision_limited(), Self::RadiationRem => UnitDefinition::si_unambiguous("rem").linear(Q::EquivalentDose, 0.01),
            Self::AstronomicalUnit => UnitDefinition::plain("au").linear(Q::Length, 149_597_870_700.0), Self::CatalyticActivity => UnitDefinition::si_unambiguous("kat").linear(Q::CatalyticActivity, 1.0), Self::Neper => UnitDefinition::plain("Np").linear(Q::NeperLevel, 1.0),
            Self::BritishThermalUnit => UnitDefinition::plain("Btu").aliases(&["BTU"]), Self::Therm => UnitDefinition::plain("therm"),
            Self::ShortHundredweight => UnitDefinition::plain("cwt (short)").linear(Q::Mass, 45_359.237), Self::LongHundredweight => UnitDefinition::plain("cwt (long)").linear(Q::Mass, 50_802.345_44),
            Self::ImperialFluidOunce => UnitDefinition::plain("fl oz (imp)").linear(Q::Volume, 0.000_028_413_062_5), Self::UsFluidOunce => UnitDefinition::plain("fl oz (US)").linear(Q::Volume, 0.000_029_573_529_562_5), Self::ImperialPint => UnitDefinition::plain("pt (imp)").linear(Q::Volume, 0.000_568_261_25), Self::UsLiquidPint => UnitDefinition::plain("pt (US)").linear(Q::Volume, 0.000_473_176_473),
            Self::ImperialQuart => UnitDefinition::plain("qt (imp)").linear(Q::Volume, 0.001_136_522_5), Self::UsLiquidQuart => UnitDefinition::plain("qt (US)").linear(Q::Volume, 0.000_946_352_946), Self::ImperialBushel => UnitDefinition::plain("bu (imp)").linear(Q::Volume, 0.036_368_72), Self::UsBushel => UnitDefinition::plain("bu (US)").linear(Q::Volume, 0.035_239_070_166_88),
            Self::ImperialGallon => UnitDefinition::plain("gal (imp)").linear(Q::Volume, 0.004_546_09), Self::UsLiquidGallon => UnitDefinition::plain("gal (US)").linear(Q::Volume, 0.003_785_411_784),
            Self::Degree => UnitDefinition::plain("°").linear(Q::Angle, std::f64::consts::PI / 180.0), Self::ArcMinute => UnitDefinition::plain("′").linear(Q::Angle, std::f64::consts::PI / 10_800.0), Self::ArcSecond => UnitDefinition::plain("″").linear(Q::Angle, std::f64::consts::PI / 648_000.0), Self::ArcThird => UnitDefinition::plain("‴").linear(Q::Angle, std::f64::consts::PI / 38_880_000.0), Self::DegreesPerSecond => UnitDefinition::plain("°/s").linear(Q::AngularVelocity, std::f64::consts::PI / 180.0),
            Self::Hectare => UnitDefinition::plain("ha").linear(Q::Area, 10_000.0), Self::Acre => UnitDefinition::plain("acre").linear(Q::Area, 4_046.856_422_4), Self::Furlong => UnitDefinition::plain("fur").linear(Q::Length, 201.168), Self::Tonne => UnitDefinition::plain("t").linear(Q::Mass, 1_000_000.0), Self::ScandinavianMile => UnitDefinition::plain("mil").linear(Q::Length, 10_000.0),
            Self::MillimetreOfMercury => UnitDefinition::plain("mmHg").linear(Q::Pressure, 133_322.387_415), Self::InchOfMercury => UnitDefinition::plain("inHg").linear(Q::Pressure, 3_386_389.0), Self::MillimetreOfWater => UnitDefinition::plain("mmH₂O").aliases(&["mmH2O"]).linear(Q::Pressure, 9_806.65),
            Self::CentimetreOfWater => UnitDefinition::plain("cmH₂O").aliases(&["cmH2O"]).linear(Q::Pressure, 98_066.5), Self::InchOfWater => UnitDefinition::plain("inH₂O").aliases(&["inH2O"]).linear(Q::Pressure, 249_088.91),
            Self::Centipoise => UnitDefinition::plain("cP").linear(Q::DynamicViscosity, 1.0), Self::Centistokes => UnitDefinition::plain("cSt").linear(Q::KinematicViscosity, 0.000_001), Self::MilesPerHour => UnitDefinition::plain("mph").linear(Q::Speed, 0.44704), Self::RevolutionsPerMinute => UnitDefinition::plain("rpm").linear(Q::Frequency, 1.0 / 60.0),
            Self::MetricHorsepower => UnitDefinition::prefixed("PS", PrefixPolicy::Horsepower, BareRecognition::Unambiguous).linear(Q::ActivePower, 735_498.75), Self::MechanicalHorsepower => UnitDefinition::plain("HP").linear(Q::ActivePower, 745_699.871_582),
            Self::Curie => UnitDefinition::si_unambiguous("Ci").linear(Q::Radioactivity, 37_000_000_000.0), Self::Jansky => UnitDefinition::si_unambiguous("Jy").linear(Q::SpectralFluxDensity, 1e-23).precision_limited(), Self::Dyne => UnitDefinition::si_unambiguous("dyn").linear(Q::Force, 0.01), Self::Erg => UnitDefinition::si_unambiguous("erg").linear(Q::Energy, 0.0001),
            Self::Oersted => UnitDefinition::si_unambiguous("Oe").linear(Q::MagneticFieldStrength, 1_000.0 / (4.0 * std::f64::consts::PI)), Self::Maxwell => UnitDefinition::si_unambiguous("Mx").linear(Q::MagneticFlux, 0.000_01), Self::Equivalent => UnitDefinition::si_unambiguous("Eq").linear(Q::ChemicalEquivalentAmount, 1.0), Self::Osmole => UnitDefinition::si_unambiguous("Osm").linear(Q::OsmoticAmount, 1.0),
        }
    }

    fn accepts_prefix(self, prefix: SiPrefix) -> bool { self.definition().prefix_policy.accepts(prefix) && !matches!((self, prefix), (Self::Siemens, SiPrefix::Peta)) }
    fn accepts_any_prefix(self) -> bool { SiPrefix::ALL.iter().copied().any(|prefix| self.accepts_prefix(prefix)) }
    pub(in crate::parser) fn requires_prefix(self) -> bool { self.definition().bare_recognition == BareRecognition::PrefixRequired }
    pub(in crate::parser) fn bare_unambiguous(self) -> bool { self.definition().bare_recognition == BareRecognition::Unambiguous }

    pub(in crate::parser) fn symbol(self) -> &'static str { self.definition().symbol }
    pub(in crate::parser) fn extra_symbols(self) -> &'static [&'static str] { self.definition().extra_symbols }
    pub(in crate::parser) fn semantics(self) -> UnitSemantics { self.definition().semantics }
    pub(in crate::parser) fn precision_limited(self) -> bool { self.definition().precision_limited }

}

pub(in crate::parser) fn measurement_dimension(kind: MeasurementQuantityKind) -> MeasurementDimension {
    let length = MeasurementDimension::Length;
    let mass = MeasurementDimension::Mass;
    let time = MeasurementDimension::Time;
    let temperature = MeasurementDimension::Temperature;
    let current = MeasurementDimension::Current;
    let voltage = MeasurementDimension::Voltage;
    let amount = MeasurementDimension::AmountOfSubstance;
    let volume = MeasurementDimension::Volume;
    let frequency = MeasurementDimension::Frequency;
    let force = mass.product(length).quotient(time.pow(2));
    let pressure = force.quotient(length.pow(2));
    let energy = force.product(length);
    let power = MeasurementDimension::Power;
    let charge = current.product(time);
    match kind {
        MeasurementQuantityKind::Length => length,
        MeasurementQuantityKind::Area => length.pow(2),
        MeasurementQuantityKind::Volume | MeasurementQuantityKind::NormalizedVolume => volume,
        MeasurementQuantityKind::NormalizedVolumeRate | MeasurementQuantityKind::VolumeRate => volume.quotient(time),
        MeasurementQuantityKind::Mass => mass,
        MeasurementQuantityKind::Time => time,
        MeasurementQuantityKind::Temperature => temperature,
        MeasurementQuantityKind::DataSize => MeasurementDimension::Data,
        MeasurementQuantityKind::DataRate => MeasurementDimension::Data.quotient(time),
        MeasurementQuantityKind::AmountOfSubstance | MeasurementQuantityKind::ChemicalEquivalentAmount | MeasurementQuantityKind::OsmoticAmount => amount,
        MeasurementQuantityKind::AmountConcentration | MeasurementQuantityKind::ChemicalEquivalentConcentration | MeasurementQuantityKind::OsmoticConcentration => amount.quotient(volume),
        MeasurementQuantityKind::ActivePower | MeasurementQuantityKind::ApparentPower | MeasurementQuantityKind::ReactivePower | MeasurementQuantityKind::RadiantIntensity => power,
        MeasurementQuantityKind::Voltage | MeasurementQuantityKind::AlternatingCurrentVoltage => voltage,
        MeasurementQuantityKind::ElectricCurrent => current,
        MeasurementQuantityKind::Frequency | MeasurementQuantityKind::Radioactivity | MeasurementQuantityKind::AngularVelocity | MeasurementQuantityKind::CountRate | MeasurementQuantityKind::FrameRate | MeasurementQuantityKind::ComputationRate | MeasurementQuantityKind::PixelRate => frequency,
        MeasurementQuantityKind::Force => force,
        MeasurementQuantityKind::Pressure => pressure,
        MeasurementQuantityKind::Energy | MeasurementQuantityKind::Torque => energy,
        MeasurementQuantityKind::ElectricalResistance => voltage.quotient(current),
        MeasurementQuantityKind::ElectricalConductance => current.quotient(voltage),
        MeasurementQuantityKind::ElectricCharge => charge,
        MeasurementQuantityKind::Capacitance => charge.quotient(voltage),
        MeasurementQuantityKind::Inductance => voltage.product(time).quotient(current),
        MeasurementQuantityKind::MagneticFlux => voltage.product(time),
        MeasurementQuantityKind::MagneticFluxDensity => voltage.product(time).quotient(length.pow(2)),
        MeasurementQuantityKind::MagneticFieldStrength => current.quotient(length),
        MeasurementQuantityKind::SpectralFluxDensity => power.product(time).quotient(length.pow(2)),
        MeasurementQuantityKind::Speed => length.quotient(time),
        MeasurementQuantityKind::Acceleration => length.quotient(time.pow(2)),
        MeasurementQuantityKind::MassConcentration => mass.quotient(volume),
        MeasurementQuantityKind::TemperatureRate => temperature.quotient(time),
        MeasurementQuantityKind::SpatialFrequency => MeasurementDimension::default().quotient(length),
        MeasurementQuantityKind::Illuminance | MeasurementQuantityKind::Luminance => MeasurementDimension::default().quotient(length.pow(2)),
        MeasurementQuantityKind::AbsorbedDose | MeasurementQuantityKind::EquivalentDose => length.pow(2).quotient(time.pow(2)),
        MeasurementQuantityKind::AbsorbedDoseRate | MeasurementQuantityKind::EquivalentDoseRate => length.pow(2).quotient(time.pow(3)),
        MeasurementQuantityKind::RadiationExposure => charge.quotient(mass),
        MeasurementQuantityKind::RadiationExposureRate => current.quotient(mass),
        MeasurementQuantityKind::CatalyticActivity => amount.quotient(time),
        MeasurementQuantityKind::CatalyticActivityConcentration => amount.quotient(time).quotient(volume),
        MeasurementQuantityKind::DynamicViscosity => pressure.product(time),
        MeasurementQuantityKind::KinematicViscosity => length.pow(2).quotient(time),
        MeasurementQuantityKind::ForcePerLength => force.quotient(length),
        MeasurementQuantityKind::AngularAcceleration => frequency.quotient(time),
        MeasurementQuantityKind::PowerPerArea => power.quotient(length.pow(2)),
        MeasurementQuantityKind::HeatCapacity => energy.quotient(temperature),
        MeasurementQuantityKind::SpecificHeatCapacity => energy.quotient(mass).quotient(temperature),
        MeasurementQuantityKind::SpecificEnergy => energy.quotient(mass),
        MeasurementQuantityKind::ThermalConductivity => power.quotient(length).quotient(temperature),
        MeasurementQuantityKind::EnergyDensity => energy.quotient(volume),
        MeasurementQuantityKind::ElectricFieldStrength => voltage.quotient(length),
        MeasurementQuantityKind::VolumeChargeDensity => charge.quotient(volume),
        MeasurementQuantityKind::SurfaceChargeDensity => charge.quotient(length.pow(2)),
        MeasurementQuantityKind::Permittivity => charge.quotient(voltage).quotient(length),
        MeasurementQuantityKind::Permeability => voltage.product(time).quotient(current).quotient(length),
        MeasurementQuantityKind::MolarEnergy => energy.quotient(amount),
        MeasurementQuantityKind::MolarHeatCapacity => energy.quotient(amount).quotient(temperature),
        MeasurementQuantityKind::Radiance => power.quotient(length.pow(2)),
        MeasurementQuantityKind::SpecificVolume => volume.quotient(mass),
        MeasurementQuantityKind::ElectricCurrentDensity => current.quotient(length.pow(2)),
        MeasurementQuantityKind::MassFraction => MeasurementDimension::default(),
        MeasurementQuantityKind::ArealMassDensity => mass.quotient(length.pow(2)),
        MeasurementQuantityKind::Unspecified | MeasurementQuantityKind::Angle | MeasurementQuantityKind::Ratio | MeasurementQuantityKind::Count | MeasurementQuantityKind::LogarithmicLevel | MeasurementQuantityKind::PixelCount | MeasurementQuantityKind::LuminousIntensity | MeasurementQuantityKind::LuminousFlux | MeasurementQuantityKind::SolidAngle | MeasurementQuantityKind::NeperLevel => MeasurementDimension::default(),
    }
}

/// Canonical symbol for a (possibly prefixed) unit — "km" for
/// (Kilo, Meter), "m" for (None, Meter), "kB" for (Kilo, Byte).
pub(in crate::parser) fn canonical_symbol(prefix: Option<SiPrefix>, base: BaseUnit) -> String {
    match (prefix, base) {
        (Some(SiPrefix::Kilo), BaseUnit::Byte) => "kB".to_string(),
        (Some(SiPrefix::Mega), BaseUnit::Byte) => "MB".to_string(),
        (Some(SiPrefix::Giga), BaseUnit::Byte) => "GB".to_string(),
        (Some(SiPrefix::Tera), BaseUnit::Byte) => "TB".to_string(),
        (Some(SiPrefix::Peta), BaseUnit::Byte) => "PB".to_string(),
        (Some(SiPrefix::Exa), BaseUnit::Byte) => "EB".to_string(),
        (Some(_), BaseUnit::Byte) => unreachable!("unsupported byte prefix"),
        (Some(p), _) => format!("{}{}", p.symbol(), base.symbol()),
        (None, _) => base.symbol().to_string(),
    }
}

/// Parses `text` as an exact (case-sensitive) unit symbol, prefixed or bare.
pub(in crate::parser) fn parse_unit_symbol(text: &str) -> Option<(Option<SiPrefix>, BaseUnit)> {
    UNIT_SYMBOLS
        .binary_search_by(|(symbol, _)| symbol.as_str().cmp(text))
        .ok()
        .map(|idx| UNIT_SYMBOLS[idx].1)
}

#[derive(Clone, Copy)]
struct UnitAlias {
    spelling: &'static str,
    target: UnitAliasTarget,
}

impl UnitAlias {
    const fn unit(spelling: &'static str, unit: BaseUnit) -> Self { Self { spelling, target: UnitAliasTarget::Unit(unit) } }
    const fn canonical(spelling: &'static str, canonical: &'static str) -> Self { Self { spelling, target: UnitAliasTarget::Canonical(canonical) } }
}

const UNIVERSAL_UNIT_ALIASES: &[UnitAlias] = &[
    UnitAlias::unit("HA", BaseUnit::Hectare),
    UnitAlias::unit("u", BaseUnit::Dalton),
    UnitAlias::unit("ua", BaseUnit::AstronomicalUnit),
    UnitAlias::canonical("J/(kg K)", "J/kg·K"),
    UnitAlias::canonical("J/(mol K)", "J/mol·K"),
    UnitAlias::canonical("KN", "kN"),
    UnitAlias::canonical("KNm", "kN·m"),
    UnitAlias::canonical("KA", "kA"),
    UnitAlias::canonical("KJ", "kJ"),
    UnitAlias::canonical("KK", "kK"),
    UnitAlias::canonical("KPS", "kPS"),
    UnitAlias::canonical("KV", "kV"),
    UnitAlias::canonical("KW", "kW"),
    UnitAlias::canonical("MNm", "MN·m"),
    UnitAlias::canonical("GNm", "GN·m"),
    UnitAlias::canonical("Nm", "N·m"),
    UnitAlias::canonical("Pa s", "Pa·s"),
    UnitAlias::canonical("W/(m K)", "W/m·K"),
    UnitAlias::canonical("W/(m² sr)", "W/m²·sr"),
    UnitAlias::canonical("kNm", "kN·m"),
    UnitAlias::canonical("mNm", "mN·m"),
    UnitAlias::canonical("V AC", "VAC"),
    UnitAlias::canonical("V\u{00A0}AC", "VAC"),
    UnitAlias::canonical("V\u{202F}AC", "VAC"),
    UnitAlias::canonical("Gb/s", "Gbit/s"),
    UnitAlias::canonical("Gbyte", "GB"),
    UnitAlias::canonical("Gflops", "GFLOPS"),
    UnitAlias::canonical("KHz", "kHz"),
    UnitAlias::canonical("KWh/m", "kWh/m"),
    UnitAlias::canonical("Kbit/s", "kbit/s"),
    UnitAlias::canonical("Pbyte", "PB"),
    UnitAlias::canonical("Pflops", "PFLOPS"),
    UnitAlias::canonical("Tb/s", "Tbit/s"),
    UnitAlias::canonical("Tbyte", "TB"),
    UnitAlias::canonical("Tflops", "TFLOPS"),
    UnitAlias::canonical("gigaflops", "GFLOPS"),
    UnitAlias::canonical("kb/s", "kbit/s"),
    UnitAlias::canonical("kbps", "kbit/s"),
    UnitAlias::canonical("mAs", "mA·s"),
    UnitAlias::canonical("oC", "°C"),
    UnitAlias::canonical("oF", "°F"),
    UnitAlias::canonical("petaflops", "PFLOPS"),
    UnitAlias::canonical("terraflops", "TFLOPS"),
    UnitAlias::canonical("ug", "µg"),
    UnitAlias::canonical("ul/min", "µl/min"),
    UnitAlias::canonical("um", "µm"),
    UnitAlias::canonical("us", "µs"),
    UnitAlias::canonical("°K", "K"),
    UnitAlias::canonical("°K/h", "K/h"),
    UnitAlias::canonical("°С", "°C"),
    UnitAlias::canonical("ºC", "°C"),
    UnitAlias::canonical("ºF", "°F"),
    UnitAlias::canonical("˚", "°"),
    UnitAlias::canonical("˚C", "°C"),
    UnitAlias::canonical("˚F", "°F"),
    UnitAlias::canonical("℃", "°C"),
];

fn universal_unit_alias(text: &str) -> Option<UnitAliasTarget> { UNIVERSAL_UNIT_ALIASES.iter().find(|alias| alias.spelling == text).map(|alias| alias.target) }

pub(in crate::parser) fn unit_spelling_is_universally_ambiguous_after_number(text: &str) -> bool { matches!(text, "u" | "GPS") }

fn localized_unit_alias(text: &str, language: ParseLanguage) -> Option<UnitAliasTarget> { language.profile().unit_lexicon().alias(text).map(|alias| alias.target()) }

pub(in crate::parser) fn parse_unit_alias(text: &str, language: ParseLanguage) -> Option<(Option<SiPrefix>, BaseUnit)> {
    localized_unit_alias(text, language)
        .or_else(|| universal_unit_alias(text))
        .and_then(|target| match target { UnitAliasTarget::Unit(base) => Some((None, base)), UnitAliasTarget::Canonical(_) | UnitAliasTarget::Scoped => None })
}

pub(in crate::parser) fn unit_spelling_allowed_in_language(text: &str, language: ParseLanguage) -> bool { !unit_alias_is_language_scoped(text) || localized_unit_alias(text, language).is_some() }

pub(in crate::parser) fn canonical_unit_alias(text: &str, language: ParseLanguage) -> Option<&'static str> {
    localized_unit_alias(text, language)
        .or_else(|| universal_unit_alias(text))
        .and_then(|target| match target { UnitAliasTarget::Canonical(canonical) => Some(canonical), UnitAliasTarget::Unit(_) | UnitAliasTarget::Scoped => None })
}

static UNIT_SYMBOLS: LazyLock<Vec<(String, (Option<SiPrefix>, BaseUnit))>> = LazyLock::new(|| {
    let mut symbols = BaseUnit::ALL
        .iter()
        .copied()
        .flat_map(unit_symbol_entries)
        .collect::<Vec<_>>();
    symbols.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.0.is_some().cmp(&right.1.0.is_some())));
    symbols.dedup_by(|left, right| left.0 == right.0);
    debug_assert!(symbols.windows(2).all(|entries| entries[0].0 != entries[1].0), "duplicate unit symbol");
    symbols
});

#[cfg(test)]
pub(in crate::parser) fn unit_symbols_are_unique() -> bool { UNIT_SYMBOLS.windows(2).all(|entries| entries[0].0 != entries[1].0) }

#[cfg(test)]
pub(in crate::parser) fn unit_semantics_are_complete() -> bool { BaseUnit::ALL.iter().all(|base| base.semantics().quantity_kind().is_some() || matches!(base, BaseUnit::Em | BaseUnit::BritishThermalUnit | BaseUnit::Therm)) }

fn unit_symbol_entries(base: BaseUnit) -> Vec<(String, (Option<SiPrefix>, BaseUnit))> {
    let mut entries = Vec::new();
    if !base.requires_prefix() {
        entries.push((base.symbol().to_string(), (None, base)));
        entries.extend(base.extra_symbols().iter().map(|symbol| ((*symbol).to_string(), (None, base))));
    }
    if base.accepts_any_prefix() {
        entries.extend(SiPrefix::ALL.iter().copied().filter(|prefix| base.accepts_prefix(*prefix)).map(|prefix| (canonical_symbol(Some(prefix), base), (Some(prefix), base))));
        entries.extend(base.extra_symbols().iter().flat_map(|symbol| {
            SiPrefix::ALL.iter().copied().filter(move |prefix| base.accepts_prefix(*prefix)).map(move |prefix| (format!("{}{symbol}", prefix.symbol()), (Some(prefix), base)))
        }));
    }
    if base == BaseUnit::Byte {
        entries.push(("KB".to_string(), (Some(SiPrefix::Kilo), base)));
    }
    entries
}

/// Parses `text` (case-insensitively) as a spelled-out unit word in
/// `language`, prefixed or bare — scoped to that one language's spellings
/// (including its own plural forms) rather than matching any supported
/// language, so "minuter" only resolves when parsing Swedish and "Sekunden"
/// only when parsing German.
pub(in crate::parser) fn parse_unit_word(text: &str, language: ParseLanguage) -> Option<(Option<SiPrefix>, BaseUnit)> {
    let normalized = crate::canonical_search_text(text);
    let table = unit_word_table(language);
    table
        .binary_search_by(|(word, _)| word.as_str().cmp(normalized.as_str()))
        .ok()
        .map(|idx| table[idx].1)
}

/// Parses an expanded unit phrase that is deliberately recognized only when
/// the source segment supplies matching canonical measurement evidence.
pub(in crate::parser) fn parse_source_grounded_unit_canonical(text: &str, language: ParseLanguage) -> Option<String> {
    let normalized = crate::canonical_search_text(text);
    let lexicon = language
        .profile()
        .unit_lexicon();
    lexicon
        .source_grounded_words()
        .iter()
        .find(|lexeme| lexeme.spellings().iter().any(|spelling| crate::canonical_search_text(spelling) == normalized))
        .map(|lexeme| lexeme.unit().symbol().to_string())
        .or_else(|| lexicon.source_grounded_compounds().iter().find(|lexeme| lexeme.spellings().iter().any(|spelling| crate::canonical_search_text(spelling) == normalized)).map(|lexeme| lexeme.canonical().to_string()))
}

static UNIT_WORDS_ENGLISH: LazyLock<Vec<(String, (Option<SiPrefix>, BaseUnit))>> = LazyLock::new(|| unit_word_entries(ParseLanguage::English));
static UNIT_WORDS_SWEDISH: LazyLock<Vec<(String, (Option<SiPrefix>, BaseUnit))>> = LazyLock::new(|| unit_word_entries(ParseLanguage::Swedish));
static UNIT_WORDS_GERMAN: LazyLock<Vec<(String, (Option<SiPrefix>, BaseUnit))>> = LazyLock::new(|| unit_word_entries(ParseLanguage::German));
static UNIT_WORDS_NEUTRAL: LazyLock<Vec<(String, (Option<SiPrefix>, BaseUnit))>> = LazyLock::new(|| unit_word_entries(ParseLanguage::Neutral));

fn unit_word_table(language: ParseLanguage) -> &'static [(String, (Option<SiPrefix>, BaseUnit))] {
    match language {
        ParseLanguage::English => &UNIT_WORDS_ENGLISH,
        ParseLanguage::Swedish => &UNIT_WORDS_SWEDISH,
        ParseLanguage::German => &UNIT_WORDS_GERMAN,
        ParseLanguage::Neutral => &UNIT_WORDS_NEUTRAL,
    }
}

fn unit_word_entries(language: ParseLanguage) -> Vec<(String, (Option<SiPrefix>, BaseUnit))> {
    let mut entries = language
        .profile()
        .unit_lexicon()
        .words()
        .iter()
        .copied()
        .flat_map(|lexeme| localized_unit_word_entries(lexeme.unit(), lexeme.spellings(), language))
        .collect::<Vec<_>>();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries.dedup_by(|a, b| a.0 == b.0);
    entries
}

fn localized_unit_word_entries(base: BaseUnit, spellings: &[&str], language: ParseLanguage) -> Vec<(String, (Option<SiPrefix>, BaseUnit))> {
    spellings.iter().flat_map(|&base_form| {
        let mut entries = (!base.requires_prefix()).then(|| (crate::canonical_search_text(base_form), (None, base))).into_iter().collect::<Vec<_>>();
        if !base.accepts_any_prefix() {
            return entries;
        }
        entries.extend(SiPrefix::ALL.iter().copied().filter(|prefix| base.accepts_prefix(*prefix)).map(|prefix| {
            let mut word = String::with_capacity(prefix.word_stem(language).len() + base_form.len());
            word.push_str(prefix.word_stem(language));
            word.push_str(base_form);
            (crate::canonical_search_text(&word), (Some(prefix), base))
        }));
        entries
    }).collect()
}

/// Resolves `text` as any recognized unit spelling — a symbol ("km", "mV"),
/// or a spelled-out form in `language` (abbreviated-language word like
/// "minuter", or a systematically generated SI prefix + base unit like
/// "millisekunder"). International symbols are language-independent, while
/// locale-dependent abbreviations such as English "in" are filtered by the
/// active language before the spelled-out word fallback is tried.
pub(in crate::parser) fn resolve_unit(text: &str, language: ParseLanguage) -> Option<(Option<SiPrefix>, BaseUnit)> {
    parse_unit_symbol(text)
        .filter(|_| unit_spelling_allowed_in_language(text, language))
        .or_else(|| parse_unit_alias(text, language))
        .or_else(|| parse_unit_word(text, language))
}

/// Existing vocabulary for a canonical unit, in catalog order. This is a
/// recognition lookup; variants are not assigned grammatical number here.
pub(crate) fn measurement_unit_word_forms(unit: &str, language: ParseLanguage) -> Vec<String> {
    let lexicon = language.profile().unit_lexicon();
    let compound = lexicon.source_grounded_compounds().iter().filter(|entry| entry.canonical() == unit).flat_map(|entry| entry.spellings().iter().map(|spelling| (*spelling).to_string()));
    let atomic = parse_unit_symbol(unit).or_else(|| BaseUnit::ALL.iter().copied().find(|base| base.symbol() == unit).map(|base| (None, base)));
    let words = atomic.into_iter().flat_map(|(prefix, base)| {
        lexicon.words().iter().chain(lexicon.source_grounded_words()).filter(move |entry| entry.unit() == base).flat_map(move |entry| {
            entry.spellings().iter().map(move |spelling| format!("{}{spelling}", prefix.map_or("", |prefix| prefix.word_stem(language))))
        })
    });
    compound.chain(words).fold(Vec::new(), |mut forms, form| {
        if !forms.contains(&form) { forms.push(form); }
        forms
    })
}
