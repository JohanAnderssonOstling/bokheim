#[cfg(any(test, feature = "localization"))]
use super::DOT_NUMBER_POLICY;
pub use segment_rules::months::ENGLISH_MONTHS;
use segment_rules::abbreviations::ENGLISH_POLICY;
use super::{LanguageProfile, OrdinalStyle, QuotationStyle, SourceGroundedCompoundLexeme, UnitAliasLexeme, UnitLexicon, UnitWordLexeme, DEFAULT_RANGE_SEPARATORS, ENGLISH_CURRENCY_LEXICON, ENGLISH_MERIDIEMS, WESTERN_QUOTE_PAIRS};
use crate::measurement_unit::BaseUnit;

pub const ENGLISH_ALIASES: &[&str] = &["en", "eng"];
pub(crate) const ENGLISH_FUNCTION_WORDS: &[&str] = &["a", "an", "and", "are", "as", "at", "be", "been", "being", "but", "by", "can", "could", "did", "do", "does", "for", "from", "had", "has", "have", "he", "her", "hers", "him", "his", "i", "if", "in", "into", "is", "it", "its", "may", "me", "might", "must", "nor", "of", "on", "or", "our", "ours", "she", "should", "than", "that", "the", "their", "theirs", "them", "these", "they", "this", "those", "to", "us", "was", "we", "were", "will", "with", "would", "you", "your", "yours"];
pub(crate) const ENGLISH_NUMBER_WORDS: &[(&str, i64)] = &[
    ("zero", 0), ("one", 1), ("two", 2), ("three", 3), ("four", 4), ("five", 5), ("six", 6), ("seven", 7), ("eight", 8), ("nine", 9),
    ("ten", 10), ("eleven", 11), ("twelve", 12), ("thirteen", 13), ("fourteen", 14), ("fifteen", 15), ("sixteen", 16), ("seventeen", 17), ("eighteen", 18), ("nineteen", 19), ("twenty", 20),
];

const ENGLISH_UNIT_PREFIX_STEMS: &[&str; 14] = &["pico", "nano", "micro", "milli", "centi", "deci", "deca", "hecto", "kilo", "mega", "giga", "tera", "peta", "exa"];
const ENGLISH_UNIT_WORDS: &[UnitWordLexeme] = &[
    UnitWordLexeme::new(BaseUnit::Meter, &["meter", "meters", "metre", "metres"]),
    UnitWordLexeme::new(BaseUnit::Angstrom, &["angstrom", "angstroms", "ångström", "ångströms"]),
    UnitWordLexeme::new(BaseUnit::Gram, &["gram", "grams"]),
    UnitWordLexeme::new(BaseUnit::Second, &["second", "seconds"]),
    UnitWordLexeme::new(BaseUnit::Liter, &["liter", "liters", "litre", "litres"]),
    UnitWordLexeme::new(BaseUnit::Watt, &["watt", "watts"]),
    UnitWordLexeme::new(BaseUnit::Volt, &["volt", "volts"]),
    UnitWordLexeme::new(BaseUnit::Ampere, &["ampere", "amperes", "amp", "amps"]),
    UnitWordLexeme::new(BaseUnit::Hertz, &["hertz"]),
    UnitWordLexeme::new(BaseUnit::Byte, &["byte", "bytes"]),
    UnitWordLexeme::new(BaseUnit::Bit, &["bit", "bits"]),
    UnitWordLexeme::new(BaseUnit::Mole, &["mole", "moles"]),
    UnitWordLexeme::new(BaseUnit::Newton, &["newton", "newtons"]),
    UnitWordLexeme::new(BaseUnit::Pascal, &["pascal", "pascals"]),
    UnitWordLexeme::new(BaseUnit::StandardAtmosphere, &["standard atmosphere", "standard atmospheres", "atmosphere", "atmospheres"]),
    UnitWordLexeme::new(BaseUnit::Torr, &["torr"]),
    UnitWordLexeme::new(BaseUnit::Joule, &["joule", "joules"]),
    UnitWordLexeme::new(BaseUnit::Ohm, &["ohm", "ohms"]),
    UnitWordLexeme::new(BaseUnit::Siemens, &["siemens"]),
    UnitWordLexeme::new(BaseUnit::Bar, &["bar"]),
    UnitWordLexeme::new(BaseUnit::Coulomb, &["coulomb", "coulombs"]),
    UnitWordLexeme::new(BaseUnit::Farad, &["farad", "farads"]),
    UnitWordLexeme::new(BaseUnit::Henry, &["henry", "henries"]),
    UnitWordLexeme::new(BaseUnit::Weber, &["weber", "webers"]),
    UnitWordLexeme::new(BaseUnit::Tesla, &["tesla", "teslas"]),
    UnitWordLexeme::new(BaseUnit::Calorie, &["calorie", "calories"]),
    UnitWordLexeme::new(BaseUnit::Becquerel, &["becquerel", "becquerels"]),
    UnitWordLexeme::new(BaseUnit::Candela, &["candela", "candelas"]),
    UnitWordLexeme::new(BaseUnit::Lumen, &["lumen", "lumens"]),
    UnitWordLexeme::new(BaseUnit::Lux, &["lux"]),
    UnitWordLexeme::new(BaseUnit::Radian, &["radian", "radians"]),
    UnitWordLexeme::new(BaseUnit::Steradian, &["steradian", "steradians"]),
    UnitWordLexeme::new(BaseUnit::ElectronVolt, &["electronvolt", "electronvolts"]),
    UnitWordLexeme::new(BaseUnit::Dalton, &["dalton", "daltons"]),
    UnitWordLexeme::new(BaseUnit::RadiationRem, &["rem"]),
    UnitWordLexeme::new(BaseUnit::AstronomicalUnit, &["astronomical unit", "astronomical units"]),
    UnitWordLexeme::new(BaseUnit::CatalyticActivity, &["katal", "katals"]),
    UnitWordLexeme::new(BaseUnit::Neper, &["neper", "nepers"]),
    UnitWordLexeme::new(BaseUnit::BritishThermalUnit, &["British thermal unit", "British thermal units"]),
    UnitWordLexeme::new(BaseUnit::Therm, &["therm", "therms"]),
    UnitWordLexeme::new(BaseUnit::ShortHundredweight, &["short hundredweight", "short hundredweights"]),
    UnitWordLexeme::new(BaseUnit::LongHundredweight, &["long hundredweight", "long hundredweights"]),
    UnitWordLexeme::new(BaseUnit::ImperialFluidOunce, &["imperial fluid ounce", "imperial fluid ounces"]),
    UnitWordLexeme::new(BaseUnit::UsFluidOunce, &["US fluid ounce", "US fluid ounces"]),
    UnitWordLexeme::new(BaseUnit::ImperialPint, &["imperial pint", "imperial pints"]),
    UnitWordLexeme::new(BaseUnit::UsLiquidPint, &["US liquid pint", "US liquid pints"]),
    UnitWordLexeme::new(BaseUnit::ImperialQuart, &["imperial quart", "imperial quarts"]),
    UnitWordLexeme::new(BaseUnit::UsLiquidQuart, &["US liquid quart", "US liquid quarts"]),
    UnitWordLexeme::new(BaseUnit::ImperialBushel, &["imperial bushel", "imperial bushels"]),
    UnitWordLexeme::new(BaseUnit::UsBushel, &["US bushel", "US bushels"]),
    UnitWordLexeme::new(BaseUnit::ImperialGallon, &["imperial gallon", "imperial gallons"]),
    UnitWordLexeme::new(BaseUnit::UsLiquidGallon, &["US liquid gallon", "US liquid gallons"]),
    UnitWordLexeme::new(BaseUnit::Celsius, &["degree Celsius", "degrees Celsius"]),
    UnitWordLexeme::new(BaseUnit::Fahrenheit, &["degree Fahrenheit", "degrees Fahrenheit"]),
    UnitWordLexeme::new(BaseUnit::Percent, &["percent", "per cent"]),
    UnitWordLexeme::new(BaseUnit::Minute, &["minute", "minutes"]),
    UnitWordLexeme::new(BaseUnit::Hour, &["hour", "hours"]),
    UnitWordLexeme::new(BaseUnit::Day, &["day", "days"]),
    UnitWordLexeme::new(BaseUnit::JulianYear, &["year", "years"]),
    UnitWordLexeme::new(BaseUnit::Grain, &["grain", "grains"]),
    UnitWordLexeme::new(BaseUnit::Foot, &["foot", "feet"]),
    UnitWordLexeme::new(BaseUnit::Inch, &["inch", "inches"]),
    UnitWordLexeme::new(BaseUnit::Yard, &["yard", "yards"]),
    UnitWordLexeme::new(BaseUnit::Mile, &["mile", "miles"]),
    UnitWordLexeme::new(BaseUnit::NauticalMile, &["nautical mile", "nautical miles"]),
    UnitWordLexeme::new(BaseUnit::Knot, &["knot", "knots"]),
    UnitWordLexeme::new(BaseUnit::Hectare, &["hectare", "hectares"]),
    UnitWordLexeme::new(BaseUnit::Acre, &["acre", "acres"]),
    UnitWordLexeme::new(BaseUnit::Furlong, &["furlong", "furlongs"]),
    UnitWordLexeme::new(BaseUnit::Tonne, &["tonne", "tonnes", "metric ton", "metric tons"]),
    UnitWordLexeme::new(BaseUnit::Pound, &["pound", "pounds"]),
    UnitWordLexeme::new(BaseUnit::Ounce, &["ounce", "ounces"]),
    UnitWordLexeme::new(BaseUnit::MechanicalHorsepower, &["horsepower"]),
    UnitWordLexeme::new(BaseUnit::PoundForcePerSquareInch, &["pound-force per square inch", "pounds-force per square inch"]),
    UnitWordLexeme::new(BaseUnit::Centipoise, &["centipoise", "centipoises"]),
    UnitWordLexeme::new(BaseUnit::Centistokes, &["centistoke", "centistokes"]),
];
const ENGLISH_UNIT_ALIASES: &[UnitAliasLexeme] = &[
    UnitAliasLexeme::unit("hp", BaseUnit::MechanicalHorsepower),
    UnitAliasLexeme::unit("sec", BaseUnit::Second),
    UnitAliasLexeme::unit("in", BaseUnit::Inch),
    UnitAliasLexeme::unit("inch", BaseUnit::Inch),
    UnitAliasLexeme::unit("hr", BaseUnit::Hour),
    UnitAliasLexeme::unit("amp", BaseUnit::Ampere),
    UnitAliasLexeme::unit("short cwt", BaseUnit::ShortHundredweight),
    UnitAliasLexeme::unit("long cwt", BaseUnit::LongHundredweight),
    UnitAliasLexeme::canonical("sq in", "in²"),
    UnitAliasLexeme::canonical("sq ft", "ft²"),
    UnitAliasLexeme::canonical("sq yd", "yd²"),
    UnitAliasLexeme::canonical("sq mi", "mi²"),
    UnitAliasLexeme::canonical("oz avdp", "oz"),
    UnitAliasLexeme::canonical("lb avdp", "lb"),
    UnitAliasLexeme::scoped("mm/hr"),
    UnitAliasLexeme::scoped("m/sec"),
    UnitAliasLexeme::scoped("KB/sec"),
    UnitAliasLexeme::scoped("K/sec"),
    UnitAliasLexeme::scoped("°C/sec"),
];
const ENGLISH_SOURCE_GROUNDED_UNIT_WORDS: &[UnitWordLexeme] = &[
    UnitWordLexeme::new(BaseUnit::PartsPerMillion, &["part per million", "parts per million"]),
    UnitWordLexeme::new(BaseUnit::CountsPerMinute, &["count per minute", "counts per minute"]),
    UnitWordLexeme::new(BaseUnit::FramesPerSecond, &["frame per second", "frames per second"]),
    UnitWordLexeme::new(BaseUnit::Hertz, &["cycle per second", "cycles per second"]),
    UnitWordLexeme::new(BaseUnit::BeatsPerMinute, &["beat per minute", "beats per minute", "heartbeat per minute", "heartbeats per minute", "pulse per minute", "pulses per minute"]),
    UnitWordLexeme::new(BaseUnit::MilesPerHour, &["mile per hour", "miles per hour"]),
    UnitWordLexeme::new(BaseUnit::RevolutionsPerMinute, &["revolution per minute", "revolutions per minute", "rotation per minute", "rotations per minute"]),
    UnitWordLexeme::new(BaseUnit::DegreesPerSecond, &["degree per second", "degrees per second"]),
    UnitWordLexeme::new(BaseUnit::LinePairsPerMillimetre, &["line pair per millimetre", "line pairs per millimetre", "line pair per millimeter", "line pairs per millimeter"]),
    UnitWordLexeme::new(BaseUnit::Flops, &["floating-point operation per second", "floating-point operations per second"]),
    UnitWordLexeme::new(BaseUnit::WattHour, &["watt hour", "watt hours"]),
    UnitWordLexeme::new(BaseUnit::AmpereHour, &["ampere hour", "ampere hours"]),
];
const ENGLISH_SOURCE_GROUNDED_COMPOUNDS: &[SourceGroundedCompoundLexeme] = &[
    SourceGroundedCompoundLexeme::new("m/s", &["metre per second", "metres per second", "meter per second", "meters per second"]),
    SourceGroundedCompoundLexeme::new("km/h", &["kilometre per hour", "kilometres per hour", "kilometer per hour", "kilometers per hour"]),
    SourceGroundedCompoundLexeme::new("l/min", &["litre per minute", "litres per minute", "liter per minute", "liters per minute"]),
    SourceGroundedCompoundLexeme::new("m³/h", &["cubic metre per hour", "cubic metres per hour", "cubic meter per hour", "cubic meters per hour"]),
    SourceGroundedCompoundLexeme::new("kg/m³", &["kilogram per cubic metre", "kilograms per cubic metre", "kilogram per cubic meter", "kilograms per cubic meter"]),
    SourceGroundedCompoundLexeme::new("W/m²", &["watt per square metre", "watts per square metre", "watt per square meter", "watts per square meter"]),
    SourceGroundedCompoundLexeme::new("N·m", &["newton metre", "newton metres", "newton meter", "newton meters"]),
    SourceGroundedCompoundLexeme::new("kWh", &["kilowatt hour", "kilowatt hours"]),
    SourceGroundedCompoundLexeme::new("mAh", &["milliampere hour", "milliampere hours"]),
];
const ENGLISH_UNIT_LEXICON: UnitLexicon = UnitLexicon::new(ENGLISH_UNIT_PREFIX_STEMS, ENGLISH_UNIT_WORDS, ENGLISH_SOURCE_GROUNDED_UNIT_WORDS, ENGLISH_SOURCE_GROUNDED_COMPOUNDS, ENGLISH_UNIT_ALIASES, &["pt"]);

pub const ENGLISH_PROFILE: LanguageProfile = LanguageProfile {
    parse_language: super::ParseLanguage::English,
    function_words: ENGLISH_FUNCTION_WORDS,
    number_words: ENGLISH_NUMBER_WORDS,
    unit_lexicon: &ENGLISH_UNIT_LEXICON,
    primary_language: "en",
    aliases: ENGLISH_ALIASES,
    months: ENGLISH_MONTHS,
    currencies: ENGLISH_CURRENCY_LEXICON,
    home_currency: None,
    meridiems: ENGLISH_MERIDIEMS,
    range_separators: DEFAULT_RANGE_SEPARATORS,
    #[cfg(any(test, feature = "localization"))]
    number_policy: DOT_NUMBER_POLICY,
    join_apostrophe_words: true,
    quote_pairs: WESTERN_QUOTE_PAIRS,
    preferred_quotation_style: Some(QuotationStyle::CurlyOpenClose),
    preferred_ordinal_style: OrdinalStyle::EnglishSuffix,
    abbreviation: ENGLISH_POLICY,
};

#[cfg(test)]
mod tests {
    #[cfg(feature = "hunspell")]
    use crate::DictionaryLookup;
    use structure::{NodeKind, Run};
    use nlprule::{Rules, Tokenizer};
    use std::path::PathBuf;

    fn data_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../testdata/nlprule/english")
    }

    fn tokenizer() -> Option<Tokenizer> {
        Tokenizer::new(data_dir().join("en_tokenizer.bin")).ok()
    }

    fn rules() -> Option<Rules> {
        Rules::new(data_dir().join("en_rules.bin")).ok()
    }

    fn structure_spans(text: &str) -> Vec<(String, std::ops::Range<usize>)> {
        let stream = crate::tokenized_runs_tree(vec![Run::new(text)]);
        structure::parser_api::units(&stream)
            .iter()
            .enumerate()
            .map(|(idx, unit)| {
                (
                    format!("{idx}:{:?}:{}", unit.kind(), structure::parser_api::unit_text(&stream, idx).unwrap_or("")),
                    structure::parser_api::unit_span(&stream, idx).expect("unit span"),
                )
            })
            .collect()
    }

    fn nlprule_spans(text: &str) -> Vec<(String, std::ops::Range<usize>)> {
        let Some(tokenizer) = tokenizer() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for sentence in tokenizer.pipe(text) {
            for token in sentence.tokens() {
                out.push((
                    token.word().as_str().to_string(),
                    token.span().byte().clone(),
                ));
            }
        }
        out
    }

    #[test]
    fn nlprule_tokenizer_shows_sentence_and_token_structure() {
        let text = "Dr. Smith went home. He slept.";
        let Some(tokenizer) = tokenizer() else {
            return;
        };
        let sentences: Vec<_> = tokenizer.pipe(text).collect();

        assert_eq!(sentences.len(), 3);
        assert_eq!(&text[sentences[0].span().byte().clone()], "Dr. ");
        assert_eq!(&text[sentences[1].span().byte().clone()], "Smith went home. ");
        assert_eq!(&text[sentences[2].span().byte().clone()], "He slept.");
    }

    #[test]
    fn nlprule_suggests_grammar_and_typo_fixes() {
        let Some(tokenizer) = tokenizer() else {
            return;
        };
        let Some(rules) = rules() else {
            return;
        };

        let typo = "I can due his homework.";
        let typo_suggestions = rules.suggest(typo, &tokenizer);
        assert!(!typo_suggestions.is_empty());

        let grammar = "She was not been here since Monday.";
        let grammar_suggestions = rules.suggest(grammar, &tokenizer);
        assert!(!grammar_suggestions.is_empty());
    }

    #[test]
    fn nlprule_sentence_end_abbreviation_is_tokenized_as_part_of_sentence() {
        let text = "We met with Dr. Smith.";
        let Some(tokenizer) = tokenizer() else {
            return;
        };
        let sentences: Vec<_> = tokenizer.pipe(text).collect();

        assert_eq!(sentences.len(), 2);
        assert_eq!(&text[sentences[0].span().byte().clone()], "We met with Dr. ");
        assert_eq!(&text[sentences[1].span().byte().clone()], "Smith.");
        assert!(sentences[0]
            .tokens()
            .iter()
            .any(|token| token.word().as_str() == "Dr"));
        assert!(sentences[0]
            .tokens()
            .iter()
            .any(|token| token.word().as_str() == "."));
    }

    #[test]
    fn nlprule_token_boundaries_vs_structure_token_boundaries() {
        let cases = [
            "Dr. Smith went home.",
            "We met with Dr. Smith.",
            "Hello!!",
            "SkyShowtime?",
            "e.g. this and U.S. that",
            "Contact support@example.com now.",
            "Visit https://example.com/path?x=1&y=2.",
        ];

        for text in cases {
            let structure = structure_spans(text);
            let nlprule = nlprule_spans(text);
            assert!(!structure.is_empty(), "expected structure spans for {text}");
            if !nlprule.is_empty() {
                assert!(nlprule.iter().all(|(_, span)| span.start < span.end), "expected valid nlprule spans for {text}");
            }
        }
    }

    #[test]
    fn apostrophe_words_are_compact_source_tree_words() {
        let mut tree = crate::tokenized_text_tree("don't you’ve rock'n'roll");
        crate::rebuild(&mut tree, crate::ParseLanguage::English);
        let nodes = tree.iter_nodes().map(|node| (node.kind(), node.text().to_string())).collect::<Vec<_>>();
        assert_eq!(nodes, vec![(NodeKind::Word, "don't".to_string()), (NodeKind::Whitespace, " ".to_string()), (NodeKind::Word, "you’ve".to_string()), (NodeKind::Whitespace, " ".to_string()), (NodeKind::Word, "rock'n'roll".to_string())]);
    }

    #[test]
    #[cfg(feature = "hunspell")]
    fn hunspell_dictionary_word_recognition_uses_baseform() {
        let parser = crate::language_parser::HunspellDictionary::default();
        if !parser.is_dictionary_word("cat") && !parser.is_dictionary_word("cats") {
            return;
        }
        assert!(parser.is_dictionary_word("cat"));
        assert!(parser.is_dictionary_word("cats"));
        assert!(parser.is_dictionary_word("programs"));
        assert!(!parser.is_dictionary_word("catz"));
    }
}
