#[cfg(any(test, feature = "localization"))]
use super::SWEDISH_NUMBER_POLICY;
use segment_rules::months::SWEDISH_MONTHS;
use segment_rules::abbreviations::SWEDISH_POLICY;
use super::{LanguageProfile, OrdinalStyle, QuotationStyle, SourceGroundedCompoundLexeme, UnitAliasLexeme, UnitLexicon, UnitWordLexeme, DEFAULT_RANGE_SEPARATORS, NO_MERIDIEMS, SWEDISH_CURRENCY_LEXICON, WESTERN_QUOTE_PAIRS};
use crate::measurement_unit::BaseUnit;

pub const SWEDISH_ALIASES: &[&str] = &["sv", "swe"];
pub(crate) const SWEDISH_FUNCTION_WORDS: &[&str] = &["att", "av", "blev", "bli", "blir", "de", "den", "denna", "deras", "det", "dessa", "du", "en", "ett", "för", "från", "han", "har", "hade", "henne", "hennes", "honom", "i", "inte", "jag", "kan", "kunde", "man", "med", "men", "mig", "ni", "och", "om", "oss", "på", "ska", "skulle", "som", "till", "under", "ur", "var", "vara", "varit", "vi", "vid", "vår", "våra", "är", "över"];
// `en` and `ett` are deliberately excluded because indefinite articles must
// remain words even when the source segment contains the numeric value 1.
pub(crate) const SWEDISH_NUMBER_WORDS: &[(&str, i64)] = &[
    ("noll", 0), ("två", 2), ("tre", 3), ("fyra", 4), ("fem", 5), ("sex", 6), ("sju", 7), ("åtta", 8), ("nio", 9),
    ("tio", 10), ("elva", 11), ("tolv", 12), ("tretton", 13), ("fjorton", 14), ("femton", 15), ("sexton", 16), ("sjutton", 17), ("arton", 18), ("nitton", 19), ("tjugo", 20),
];

const SWEDISH_UNIT_PREFIX_STEMS: &[&str; 14] = &["piko", "nano", "mikro", "milli", "centi", "deci", "deka", "hekto", "kilo", "mega", "giga", "tera", "peta", "exa"];
const SWEDISH_UNIT_WORDS: &[UnitWordLexeme] = &[
    UnitWordLexeme::new(BaseUnit::Meter, &["meter"]),
    UnitWordLexeme::new(BaseUnit::Angstrom, &["ångström", "angstrom"]),
    UnitWordLexeme::new(BaseUnit::Gram, &["gram"]),
    UnitWordLexeme::new(BaseUnit::Second, &["sekund", "sekunder"]),
    UnitWordLexeme::new(BaseUnit::Liter, &["liter"]),
    UnitWordLexeme::new(BaseUnit::Watt, &["watt"]),
    UnitWordLexeme::new(BaseUnit::Volt, &["volt"]),
    UnitWordLexeme::new(BaseUnit::Ampere, &["ampere"]),
    UnitWordLexeme::new(BaseUnit::Hertz, &["hertz"]),
    UnitWordLexeme::new(BaseUnit::Byte, &["byte"]),
    UnitWordLexeme::new(BaseUnit::Bit, &["bit"]),
    UnitWordLexeme::new(BaseUnit::Mole, &["mol"]),
    UnitWordLexeme::new(BaseUnit::Newton, &["newton"]),
    UnitWordLexeme::new(BaseUnit::Pascal, &["pascal"]),
    UnitWordLexeme::new(BaseUnit::StandardAtmosphere, &["standardatmosfär", "standardatmosfärer", "atmosfär", "atmosfärer"]),
    UnitWordLexeme::new(BaseUnit::Torr, &["torr"]),
    UnitWordLexeme::new(BaseUnit::Joule, &["joule"]),
    UnitWordLexeme::new(BaseUnit::Ohm, &["ohm"]),
    UnitWordLexeme::new(BaseUnit::Siemens, &["siemens"]),
    UnitWordLexeme::new(BaseUnit::Bar, &["bar"]),
    UnitWordLexeme::new(BaseUnit::Coulomb, &["coulomb"]),
    UnitWordLexeme::new(BaseUnit::Farad, &["farad"]),
    UnitWordLexeme::new(BaseUnit::Henry, &["henry"]),
    UnitWordLexeme::new(BaseUnit::Weber, &["weber"]),
    UnitWordLexeme::new(BaseUnit::Tesla, &["tesla"]),
    UnitWordLexeme::new(BaseUnit::Calorie, &["kalori", "kalorier"]),
    UnitWordLexeme::new(BaseUnit::Becquerel, &["becquerel"]),
    UnitWordLexeme::new(BaseUnit::Candela, &["candela"]),
    UnitWordLexeme::new(BaseUnit::Lumen, &["lumen"]),
    UnitWordLexeme::new(BaseUnit::Lux, &["lux"]),
    UnitWordLexeme::new(BaseUnit::Radian, &["radian", "radianer"]),
    UnitWordLexeme::new(BaseUnit::Steradian, &["steradian", "steradianer"]),
    UnitWordLexeme::new(BaseUnit::ElectronVolt, &["elektronvolt"]),
    UnitWordLexeme::new(BaseUnit::Dalton, &["dalton"]),
    UnitWordLexeme::new(BaseUnit::RadiationRem, &["rem"]),
    UnitWordLexeme::new(BaseUnit::AstronomicalUnit, &["astronomisk enhet", "astronomiska enheter"]),
    UnitWordLexeme::new(BaseUnit::CatalyticActivity, &["katal"]),
    UnitWordLexeme::new(BaseUnit::Neper, &["neper"]),
    UnitWordLexeme::new(BaseUnit::Celsius, &["grad Celsius"]),
    UnitWordLexeme::new(BaseUnit::Fahrenheit, &["grad Fahrenheit"]),
    UnitWordLexeme::new(BaseUnit::Percent, &["procent"]),
    UnitWordLexeme::new(BaseUnit::Minute, &["minut", "minuter"]),
    UnitWordLexeme::new(BaseUnit::Hour, &["timme", "timmar"]),
    UnitWordLexeme::new(BaseUnit::Day, &["dag", "dagar"]),
    UnitWordLexeme::new(BaseUnit::JulianYear, &["år"]),
    UnitWordLexeme::new(BaseUnit::Foot, &["fot", "fötter"]),
    UnitWordLexeme::new(BaseUnit::Inch, &["tum"]),
    UnitWordLexeme::new(BaseUnit::NauticalMile, &["sjömil"]),
    UnitWordLexeme::new(BaseUnit::Knot, &["knop"]),
    UnitWordLexeme::new(BaseUnit::Hectare, &["hektar"]),
    UnitWordLexeme::new(BaseUnit::Tonne, &["ton"]),
    UnitWordLexeme::new(BaseUnit::ScandinavianMile, &["mil"]),
    UnitWordLexeme::new(BaseUnit::Pound, &["pund"]),
    UnitWordLexeme::new(BaseUnit::Ounce, &["uns"]),
    UnitWordLexeme::new(BaseUnit::MetricHorsepower, &["hästkraft", "hästkrafter"]),
    UnitWordLexeme::new(BaseUnit::Centipoise, &["centipoise"]),
    UnitWordLexeme::new(BaseUnit::Centistokes, &["centistoke", "centistokes"]),
];
const SWEDISH_UNIT_ALIASES: &[UnitAliasLexeme] = &[
    UnitAliasLexeme::unit("hk", BaseUnit::MetricHorsepower),
    UnitAliasLexeme::unit("sek", BaseUnit::Second),
    UnitAliasLexeme::unit("tim", BaseUnit::Hour),
    UnitAliasLexeme::unit("mil", BaseUnit::ScandinavianMile),
];
const SWEDISH_SOURCE_GROUNDED_UNIT_WORDS: &[UnitWordLexeme] = &[
    UnitWordLexeme::new(BaseUnit::PartsPerMillion, &["del per miljon", "delar per miljon"]),
    UnitWordLexeme::new(BaseUnit::CountsPerMinute, &["räkning per minut", "räkningar per minut"]),
    UnitWordLexeme::new(BaseUnit::FramesPerSecond, &["bildruta per sekund", "bildrutor per sekund"]),
    UnitWordLexeme::new(BaseUnit::Hertz, &["cykel per sekund", "cykler per sekund"]),
    UnitWordLexeme::new(BaseUnit::BeatsPerMinute, &["slag per minut", "hjärtslag per minut", "pulsslag per minut"]),
    UnitWordLexeme::new(BaseUnit::MilesPerHour, &["engelsk mil per timme"]),
    UnitWordLexeme::new(BaseUnit::RevolutionsPerMinute, &["varv per minut", "rotationer per minut"]),
    UnitWordLexeme::new(BaseUnit::DegreesPerSecond, &["grad per sekund", "grader per sekund"]),
    UnitWordLexeme::new(BaseUnit::LinePairsPerMillimetre, &["linjepar per millimeter"]),
    UnitWordLexeme::new(BaseUnit::Flops, &["flyttalsoperation per sekund", "flyttalsoperationer per sekund"]),
    UnitWordLexeme::new(BaseUnit::WattHour, &["wattimme", "wattimmar"]),
    UnitWordLexeme::new(BaseUnit::AmpereHour, &["amperetimme", "amperetimmar"]),
];
const SWEDISH_SOURCE_GROUNDED_COMPOUNDS: &[SourceGroundedCompoundLexeme] = &[
    SourceGroundedCompoundLexeme::new("m/s", &["meter per sekund"]),
    SourceGroundedCompoundLexeme::new("km/h", &["kilometer per timme"]),
    SourceGroundedCompoundLexeme::new("l/min", &["liter per minut"]),
    SourceGroundedCompoundLexeme::new("m³/h", &["kubikmeter per timme"]),
    SourceGroundedCompoundLexeme::new("kg/m³", &["kilogram per kubikmeter"]),
    SourceGroundedCompoundLexeme::new("W/m²", &["watt per kvadratmeter"]),
    SourceGroundedCompoundLexeme::new("N·m", &["newtonmeter"]),
    SourceGroundedCompoundLexeme::new("kWh", &["kilowattimme", "kilowattimmar"]),
    SourceGroundedCompoundLexeme::new("mAh", &["milliamperetimme", "milliamperetimmar"]),
];
const SWEDISH_UNIT_LEXICON: UnitLexicon = UnitLexicon::new(SWEDISH_UNIT_PREFIX_STEMS, SWEDISH_UNIT_WORDS, SWEDISH_SOURCE_GROUNDED_UNIT_WORDS, SWEDISH_SOURCE_GROUNDED_COMPOUNDS, SWEDISH_UNIT_ALIASES, &["rad"]);

pub const SWEDISH_PROFILE: LanguageProfile = LanguageProfile {
    parse_language: super::ParseLanguage::Swedish,
    function_words: SWEDISH_FUNCTION_WORDS,
    number_words: SWEDISH_NUMBER_WORDS,
    unit_lexicon: &SWEDISH_UNIT_LEXICON,
    primary_language: "sv",
    aliases: SWEDISH_ALIASES,
    months: SWEDISH_MONTHS,
    currencies: SWEDISH_CURRENCY_LEXICON,
    home_currency: Some("SEK"),
    meridiems: NO_MERIDIEMS,
    range_separators: DEFAULT_RANGE_SEPARATORS,
    #[cfg(any(test, feature = "localization"))]
    number_policy: SWEDISH_NUMBER_POLICY,
    join_apostrophe_words: false,
    quote_pairs: WESTERN_QUOTE_PAIRS,
    preferred_quotation_style: Some(QuotationStyle::CurlyHighHigh),
    preferred_ordinal_style: OrdinalStyle::SwedishColonSuffix,
    abbreviation: SWEDISH_POLICY,
};
