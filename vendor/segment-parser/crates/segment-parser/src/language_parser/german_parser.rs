#[cfg(any(test, feature = "localization"))]
use super::COMMA_NUMBER_POLICY;
use segment_rules::months::GERMAN_MONTHS;
use segment_rules::abbreviations::GERMAN_POLICY;
use super::{LanguageProfile, OrdinalStyle, QuotationStyle, SourceGroundedCompoundLexeme, UnitAliasLexeme, UnitLexicon, UnitWordLexeme, DEFAULT_RANGE_SEPARATORS, GERMAN_CURRENCY_LEXICON, NO_MERIDIEMS, WESTERN_QUOTE_PAIRS};
use crate::measurement_unit::BaseUnit;

pub const GERMAN_ALIASES: &[&str] = &["de", "deu", "ger"];
pub(crate) const GERMAN_FUNCTION_WORDS: &[&str] = &["aber", "als", "am", "an", "auf", "aus", "bei", "bin", "bis", "das", "dem", "den", "der", "des", "die", "durch", "ein", "eine", "einer", "eines", "er", "es", "für", "hat", "haben", "ich", "im", "in", "ist", "mit", "nach", "nicht", "oder", "sie", "sind", "über", "um", "und", "uns", "unter", "vom", "von", "vor", "war", "waren", "wir", "zu", "zum", "zur"];
// Indefinite articles (`ein`, `eine`, and their inflections) are deliberately
// excluded. `eins` remains because it is an unambiguous standalone cardinal.
pub(crate) const GERMAN_NUMBER_WORDS: &[(&str, i64)] = &[
    ("null", 0), ("eins", 1),
    ("zwei", 2), ("drei", 3), ("vier", 4), ("fünf", 5), ("sechs", 6), ("sieben", 7), ("acht", 8), ("neun", 9),
    ("zehn", 10), ("elf", 11), ("zwölf", 12), ("dreizehn", 13), ("vierzehn", 14), ("fünfzehn", 15), ("sechzehn", 16), ("siebzehn", 17), ("achtzehn", 18), ("neunzehn", 19), ("zwanzig", 20),
];

const GERMAN_UNIT_PREFIX_STEMS: &[&str; 14] = &["piko", "nano", "mikro", "milli", "zenti", "dezi", "deka", "hekto", "kilo", "mega", "giga", "tera", "peta", "exa"];
const GERMAN_UNIT_WORDS: &[UnitWordLexeme] = &[
    UnitWordLexeme::new(BaseUnit::Meter, &["meter", "metern"]),
    UnitWordLexeme::new(BaseUnit::Angstrom, &["Ångström", "Angström", "Angstrom"]),
    UnitWordLexeme::new(BaseUnit::Gram, &["gramm"]),
    UnitWordLexeme::new(BaseUnit::Second, &["sekunde", "sekunden"]),
    UnitWordLexeme::new(BaseUnit::Liter, &["liter", "litern"]),
    UnitWordLexeme::new(BaseUnit::Watt, &["watt"]),
    UnitWordLexeme::new(BaseUnit::Volt, &["volt"]),
    UnitWordLexeme::new(BaseUnit::Ampere, &["ampere"]),
    UnitWordLexeme::new(BaseUnit::Hertz, &["hertz"]),
    UnitWordLexeme::new(BaseUnit::Byte, &["byte", "bytes"]),
    UnitWordLexeme::new(BaseUnit::Bit, &["bit"]),
    UnitWordLexeme::new(BaseUnit::Mole, &["mol"]),
    UnitWordLexeme::new(BaseUnit::Newton, &["newton"]),
    UnitWordLexeme::new(BaseUnit::Pascal, &["pascal"]),
    UnitWordLexeme::new(BaseUnit::StandardAtmosphere, &["Standardatmosphäre", "Standardatmosphären", "Atmosphäre", "Atmosphären"]),
    UnitWordLexeme::new(BaseUnit::Torr, &["Torr"]),
    UnitWordLexeme::new(BaseUnit::Joule, &["joule"]),
    UnitWordLexeme::new(BaseUnit::Ohm, &["ohm"]),
    UnitWordLexeme::new(BaseUnit::Siemens, &["siemens"]),
    UnitWordLexeme::new(BaseUnit::Bar, &["bar"]),
    UnitWordLexeme::new(BaseUnit::Coulomb, &["coulomb"]),
    UnitWordLexeme::new(BaseUnit::Farad, &["farad"]),
    UnitWordLexeme::new(BaseUnit::Henry, &["henry"]),
    UnitWordLexeme::new(BaseUnit::Weber, &["weber"]),
    UnitWordLexeme::new(BaseUnit::Tesla, &["tesla"]),
    UnitWordLexeme::new(BaseUnit::Calorie, &["kalorie", "kalorien"]),
    UnitWordLexeme::new(BaseUnit::Becquerel, &["becquerel"]),
    UnitWordLexeme::new(BaseUnit::Candela, &["candela"]),
    UnitWordLexeme::new(BaseUnit::Lumen, &["lumen"]),
    UnitWordLexeme::new(BaseUnit::Lux, &["lux"]),
    UnitWordLexeme::new(BaseUnit::Radian, &["radiant", "radianten"]),
    UnitWordLexeme::new(BaseUnit::Steradian, &["steradiant", "steradianten"]),
    UnitWordLexeme::new(BaseUnit::ElectronVolt, &["elektronenvolt"]),
    UnitWordLexeme::new(BaseUnit::Dalton, &["dalton"]),
    UnitWordLexeme::new(BaseUnit::RadiationRem, &["rem"]),
    UnitWordLexeme::new(BaseUnit::AstronomicalUnit, &["astronomische Einheit", "astronomische Einheiten"]),
    UnitWordLexeme::new(BaseUnit::CatalyticActivity, &["Katal"]),
    UnitWordLexeme::new(BaseUnit::Neper, &["Neper"]),
    UnitWordLexeme::new(BaseUnit::Celsius, &["Grad Celsius"]),
    UnitWordLexeme::new(BaseUnit::Fahrenheit, &["Grad Fahrenheit"]),
    UnitWordLexeme::new(BaseUnit::Percent, &["Prozent"]),
    UnitWordLexeme::new(BaseUnit::Minute, &["Minute", "Minuten"]),
    UnitWordLexeme::new(BaseUnit::Hour, &["Stunde", "Stunden"]),
    UnitWordLexeme::new(BaseUnit::Day, &["Tag", "Tage", "Tagen"]),
    UnitWordLexeme::new(BaseUnit::JulianYear, &["Jahr", "Jahre", "Jahren"]),
    UnitWordLexeme::new(BaseUnit::Foot, &["Fuß", "Füße", "Füßen", "Fuss", "Füsse", "Füssen"]),
    UnitWordLexeme::new(BaseUnit::Inch, &["Zoll"]),
    UnitWordLexeme::new(BaseUnit::Mile, &["Meile", "Meilen"]),
    UnitWordLexeme::new(BaseUnit::NauticalMile, &["Seemeile", "Seemeilen"]),
    UnitWordLexeme::new(BaseUnit::Knot, &["Knoten"]),
    UnitWordLexeme::new(BaseUnit::Hectare, &["hektar"]),
    UnitWordLexeme::new(BaseUnit::Tonne, &["Tonne", "Tonnen"]),
    UnitWordLexeme::new(BaseUnit::Pound, &["Pfund"]),
    UnitWordLexeme::new(BaseUnit::Ounce, &["Unze", "Unzen"]),
    UnitWordLexeme::new(BaseUnit::MetricHorsepower, &["Pferdestärke", "Pferdestärken"]),
    UnitWordLexeme::new(BaseUnit::Centipoise, &["Centipoise"]),
    UnitWordLexeme::new(BaseUnit::Centistokes, &["Centistoke", "Centistokes"]),
];
const GERMAN_UNIT_ALIASES: &[UnitAliasLexeme] = &[
    UnitAliasLexeme::unit("sec", BaseUnit::Second),
    UnitAliasLexeme::unit("sek", BaseUnit::Second),
    UnitAliasLexeme::unit("Sek", BaseUnit::Second),
    UnitAliasLexeme::unit("Std", BaseUnit::Hour),
    UnitAliasLexeme::unit("std", BaseUnit::Hour),
    UnitAliasLexeme::scoped("m/sec"),
    UnitAliasLexeme::scoped("KB/sec"),
    UnitAliasLexeme::scoped("K/sec"),
    UnitAliasLexeme::scoped("°C/sec"),
    UnitAliasLexeme::canonical("qm", "m²"),
    UnitAliasLexeme::canonical("mg/qm", "mg/m²"),
    UnitAliasLexeme::canonical("g/qm", "g/m²"),
    UnitAliasLexeme::canonical("U/min", "rpm"),
    UnitAliasLexeme::canonical("h/Tag", "h/d"),
    UnitAliasLexeme::canonical("Mbit/Sek", "Mbit/s"),
];
const GERMAN_SOURCE_GROUNDED_UNIT_WORDS: &[UnitWordLexeme] = &[
    UnitWordLexeme::new(BaseUnit::PartsPerMillion, &["Teil pro Million", "Teile pro Million"]),
    UnitWordLexeme::new(BaseUnit::CountsPerMinute, &["Zählung pro Minute", "Zählungen pro Minute"]),
    UnitWordLexeme::new(BaseUnit::FramesPerSecond, &["Bild pro Sekunde", "Bilder pro Sekunde"]),
    UnitWordLexeme::new(BaseUnit::Hertz, &["Zyklus pro Sekunde", "Zyklen pro Sekunde"]),
    UnitWordLexeme::new(BaseUnit::BeatsPerMinute, &["Schlag pro Minute", "Schläge pro Minute", "Herzschlag pro Minute", "Herzschläge pro Minute", "Puls pro Minute", "Pulse pro Minute"]),
    UnitWordLexeme::new(BaseUnit::MilesPerHour, &["Meile pro Stunde", "Meilen pro Stunde"]),
    UnitWordLexeme::new(BaseUnit::RevolutionsPerMinute, &["Umdrehung pro Minute", "Umdrehungen pro Minute", "Rotation pro Minute", "Rotationen pro Minute"]),
    UnitWordLexeme::new(BaseUnit::DegreesPerSecond, &["Grad pro Sekunde"]),
    UnitWordLexeme::new(BaseUnit::LinePairsPerMillimetre, &["Linienpaar pro Millimeter", "Linienpaare pro Millimeter"]),
    UnitWordLexeme::new(BaseUnit::Flops, &["Gleitkommaoperation pro Sekunde", "Gleitkommaoperationen pro Sekunde"]),
    UnitWordLexeme::new(BaseUnit::WattHour, &["Wattstunde", "Wattstunden"]),
    UnitWordLexeme::new(BaseUnit::AmpereHour, &["Amperestunde", "Amperestunden"]),
];
const GERMAN_SOURCE_GROUNDED_COMPOUNDS: &[SourceGroundedCompoundLexeme] = &[
    SourceGroundedCompoundLexeme::new("m/s", &["Meter pro Sekunde"]),
    SourceGroundedCompoundLexeme::new("km/h", &["Kilometer pro Stunde"]),
    SourceGroundedCompoundLexeme::new("l/min", &["Liter pro Minute"]),
    SourceGroundedCompoundLexeme::new("m³/h", &["Kubikmeter pro Stunde"]),
    SourceGroundedCompoundLexeme::new("kg/m³", &["Kilogramm pro Kubikmeter"]),
    SourceGroundedCompoundLexeme::new("W/m²", &["Watt pro Quadratmeter"]),
    SourceGroundedCompoundLexeme::new("N·m", &["Newtonmeter"]),
    SourceGroundedCompoundLexeme::new("kWh", &["Kilowattstunde", "Kilowattstunden"]),
    SourceGroundedCompoundLexeme::new("mAh", &["Milliampere-Stunde", "Milliampere-Stunden"]),
];
const GERMAN_UNIT_LEXICON: UnitLexicon = UnitLexicon::new(GERMAN_UNIT_PREFIX_STEMS, GERMAN_UNIT_WORDS, GERMAN_SOURCE_GROUNDED_UNIT_WORDS, GERMAN_SOURCE_GROUNDED_COMPOUNDS, GERMAN_UNIT_ALIASES, &["Grad"]);

pub const GERMAN_PROFILE: LanguageProfile = LanguageProfile {
    parse_language: super::ParseLanguage::German,
    function_words: GERMAN_FUNCTION_WORDS,
    number_words: GERMAN_NUMBER_WORDS,
    unit_lexicon: &GERMAN_UNIT_LEXICON,
    primary_language: "de",
    aliases: GERMAN_ALIASES,
    months: GERMAN_MONTHS,
    currencies: GERMAN_CURRENCY_LEXICON,
    home_currency: Some("EUR"),
    meridiems: NO_MERIDIEMS,
    range_separators: DEFAULT_RANGE_SEPARATORS,
    #[cfg(any(test, feature = "localization"))]
    number_policy: COMMA_NUMBER_POLICY,
    join_apostrophe_words: false,
    quote_pairs: WESTERN_QUOTE_PAIRS,
    preferred_quotation_style: Some(QuotationStyle::CurlyLowHigh),
    preferred_ordinal_style: OrdinalStyle::GermanPeriod,
    abbreviation: GERMAN_POLICY,
};
