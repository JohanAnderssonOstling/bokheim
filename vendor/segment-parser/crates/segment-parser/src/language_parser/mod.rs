pub use segment_rules::abbreviations::AbbreviationPolicy;
use segment_rules::abbreviations::{AbbreviationEvidence, has_continuation_evidence, is_dotted_initialism};
use std::ops::Range;

use crate::char_class::is_closing_enclosure;
use crate::stream_util::{contiguous_stream_end, unit_range_text};
use crate::measurement_unit::BaseUnit;
use structure::{DelimiterFamily, OrdinalStyle, PunctuationSpacingSide, QuotationStyle, SegmentTree, UnitKind, WhitespaceTypographyKind, WhitespaceTypographyPreference};
#[cfg(feature = "hunspell")]
use hunspell_rs::Hunspell;

mod abbreviations;
#[cfg(feature = "hunspell")]
mod dictionary;
mod english_parser;
mod german_parser;
mod swedish_parser;

use abbreviations::{abbreviation_policy_contains, abbreviation_policy_evidence, abbreviation_policy_has_prefix, profile_from_alias};
#[cfg(test)]
pub(crate) use abbreviations::LANGUAGE_PROFILES;
#[cfg(not(test))]
use abbreviations::LANGUAGE_PROFILES;

#[cfg(feature = "hunspell")]
pub(crate) use dictionary::{dictionary_analysis_with_hunspell, dictionary_baseform_with_hunspell, is_dictionary_word_with_hunspell, load_bundled_dictionary};

use english_parser::{ENGLISH_MONTHS, ENGLISH_PROFILE};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ParseLanguage {
    English,
    Swedish,
    German,
    Neutral,
}

impl ParseLanguage {
    pub fn from_bcp47(lang_tag: &str) -> Self { LanguageTag::parse(lang_tag).parse_language() }
    pub(crate) fn profile(self) -> &'static LanguageProfile { if self == Self::Neutral { return &NEUTRAL_PROFILE; } LANGUAGE_PROFILES.iter().find(|profile| profile.parse_language == self).unwrap_or(&ENGLISH_PROFILE) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MeasurementSystem {
    ImperialUk,
    UsCustomary,
}

#[derive(Clone, Copy, Debug)]
pub struct ParseLocale {
    language: ParseLanguage,
    number_policy: NumberParsingPolicy,
    time_format: TimeFormatPolicy,
    date_field_order: DateFieldOrder,
    punctuation_policy: &'static PunctuationPolicy,
    measurement_system: Option<MeasurementSystem>,
}

impl ParseLocale {
    pub fn from_bcp47(lang_tag: &str) -> Self { let tag = LanguageTag::parse(lang_tag); Self::new(tag.parse_language(), tag.number_policy(), tag.time_format(), tag.date_field_order(), tag.punctuation_policy(), tag.measurement_system()) }
    pub fn from_language(language: ParseLanguage) -> Self { Self { language, ..Self::from_bcp47(language.profile().primary_language) } }
    pub fn language(self) -> ParseLanguage { self.language }
    pub fn measurement_system(self) -> Option<MeasurementSystem> { self.measurement_system }
    pub(crate) fn number_policy(self) -> NumberParsingPolicy { self.number_policy }
    pub(crate) fn time_format(self) -> TimeFormatPolicy { self.time_format }
    pub(crate) fn date_field_order(self) -> DateFieldOrder { self.date_field_order }
    pub(crate) fn punctuation_policy(self) -> &'static PunctuationPolicy { self.punctuation_policy }
    fn new(language: ParseLanguage, number_policy: NumberParsingPolicy, time_format: TimeFormatPolicy, date_field_order: DateFieldOrder, punctuation_policy: &'static PunctuationPolicy, measurement_system: Option<MeasurementSystem>) -> Self { Self { language, number_policy, time_format, date_field_order, punctuation_policy, measurement_system } }
}

impl From<ParseLanguage> for ParseLocale {
    fn from(language: ParseLanguage) -> Self { Self::from_language(language) }
}

impl From<&str> for ParseLocale {
    fn from(lang_tag: &str) -> Self { Self::from_bcp47(lang_tag) }
}

impl From<&String> for ParseLocale {
    fn from(lang_tag: &String) -> Self { Self::from_bcp47(lang_tag) }
}

const SWEDISH_CODES: &[&str] = &["sv", "swe"];
const GERMAN_CODES: &[&str] = &["de", "ger", "deu"];
const SWISS_REGIONS: &[&str] = &["CH", "LI"];
const SWISS_NUMBER_LANGUAGES: &[&str] = &["de", "deu", "ger", "it", "ita", "rm", "roh"];
const ENGLISH_GERMAN_PUNCTUATION_CODES: &[&str] = &["en", "eng", "de", "ger", "deu"];
const SPANISH_CODES: &[&str] = &["es", "spa"];
const FRENCH_CODES: &[&str] = &["fr", "fre", "fra"];
const COMMA_DECIMAL_LANGUAGES: &[&str] = &[
    "sq", "alb", "sqi", "bs", "bos", "ca", "cat", "cs", "cze", "ces", "da", "dan", "de", "ger", "deu", "es", "spa", "et", "est",
    "eu", "baq", "eus", "fi", "fin", "fo", "fao", "fr", "fre", "fra", "hr", "hrv", "hu", "hun", "is", "ice", "isl", "it", "ita",
    "lb", "ltz", "lt", "lit", "lv", "lav", "nl", "dut", "nld", "no", "nor", "nb", "nob", "nn", "nno", "pl", "pol", "pt", "por",
    "ro", "rum", "ron", "sk", "slo", "slk", "sl", "slv", "sv", "swe",
];
const ENGLISH_MONTH_DAY_YEAR_REGIONS: &[&str] = &["US", "AS", "FM", "GU", "MH", "MP", "PH", "PR", "PW", "UM", "VI"];

/// A BCP-47-ish tag split once into the parts every locale policy is chosen from.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LanguageTag<'a> {
    primary: &'a str,
    region: Option<&'a str>,
}

impl<'a> LanguageTag<'a> {
    pub(crate) fn parse(lang_tag: &'a str) -> Self { Self { primary: primary_language(lang_tag), region: locale_region(lang_tag) } }
    pub(crate) fn supported_profile(self) -> Option<&'static LanguageProfile> { profile_from_alias(self.primary) }
    fn profile(self) -> &'static LanguageProfile { self.supported_profile().unwrap_or(&ENGLISH_PROFILE) }
    fn parse_language(self) -> ParseLanguage { self.profile().parse_language }
    fn is_one_of(self, codes: &[&str]) -> bool { codes.iter().any(|code| code.eq_ignore_ascii_case(self.primary)) }
    fn region_is_one_of(self, regions: &[&str]) -> bool { self.region.is_some_and(|region| regions.iter().any(|candidate| candidate.eq_ignore_ascii_case(region))) }

    pub(crate) fn number_policy(self) -> NumberParsingPolicy {
        if self.region_is_one_of(SWISS_REGIONS) && self.is_one_of(SWISS_NUMBER_LANGUAGES) { return SWISS_NUMBER_POLICY; }
        if self.is_one_of(SWEDISH_CODES) { return SWEDISH_NUMBER_POLICY; }
        if self.is_one_of(COMMA_DECIMAL_LANGUAGES) { COMMA_NUMBER_POLICY } else { DOT_NUMBER_POLICY }
    }

    fn time_format(self) -> TimeFormatPolicy {
        if self.parse_language() != ParseLanguage::German { return DEFAULT_TIME_FORMAT; }
        if self.region_is_one_of(SWISS_REGIONS) { GERMAN_SWISS_TIME_FORMAT } else { GERMAN_TIME_FORMAT }
    }

    fn date_field_order(self) -> DateFieldOrder {
        match self.supported_profile().map(|profile| profile.parse_language) {
            None => DateFieldOrder::MonthDayYear,
            Some(ParseLanguage::English) if self.region.is_none() || self.region_is_one_of(ENGLISH_MONTH_DAY_YEAR_REGIONS) => DateFieldOrder::MonthDayYear,
            Some(_) => DateFieldOrder::DayMonthYear,
        }
    }

    fn punctuation_policy(self) -> &'static PunctuationPolicy {
        if self.is_one_of(ENGLISH_GERMAN_PUNCTUATION_CODES) { return &ENGLISH_GERMAN_PUNCTUATION_POLICY; }
        if self.is_one_of(SWEDISH_CODES) { return &SWEDISH_PUNCTUATION_POLICY; }
        if self.is_one_of(SPANISH_CODES) { return &SPANISH_PUNCTUATION_POLICY; }
        if self.is_one_of(FRENCH_CODES) { &EUROPEAN_FRENCH_PUNCTUATION_POLICY } else { &DEFAULT_PUNCTUATION_POLICY }
    }

    fn measurement_system(self) -> Option<MeasurementSystem> {
        if self.region_is_one_of(&["GB"]) { return Some(MeasurementSystem::ImperialUk); }
        self.region_is_one_of(&["US"]).then_some(MeasurementSystem::UsCustomary)
    }
}

pub fn parenthetical_dash_whitespace_preference(lang_tag: &str) -> Option<WhitespaceTypographyPreference> {
    let tag = LanguageTag::parse(lang_tag);
    (tag.is_one_of(SWEDISH_CODES) || tag.is_one_of(GERMAN_CODES)).then_some(WhitespaceTypographyPreference::any_present(WhitespaceTypographyKind::Regular))
}

#[derive(Clone, Copy, Debug)]
pub struct NumberParsingPolicy {
    decimal_separators: &'static [char],
    grouping_separators: &'static [char],
    preferred_grouping_separators: &'static [char],
    canonical_grouping_separator: char,
}

#[derive(Clone, Copy, Debug)]
pub struct TimeFormatPolicy {
    separators: &'static [char],
    suffixes: &'static [&'static str],
}

impl TimeFormatPolicy {
    pub(crate) fn separators(self) -> &'static [char] { self.separators }
    pub(crate) fn suffixes(self) -> &'static [&'static str] { self.suffixes }
    pub(crate) fn allows_suffix(self, suffix: &str) -> bool { self.suffixes.iter().any(|candidate| suffix.eq_ignore_ascii_case(candidate)) }
}

impl NumberParsingPolicy {
    pub(crate) fn decimal_separators(self) -> &'static [char] { self.decimal_separators }
    pub(crate) fn grouping_separators(self) -> &'static [char] { self.grouping_separators }
    pub(crate) fn preferred_grouping_separators(self) -> &'static [char] { self.preferred_grouping_separators }
    pub(crate) fn canonical_grouping_separator(self) -> char { self.canonical_grouping_separator }
}

#[derive(Clone, Copy, Debug)]
pub struct CurrencyLexeme {
    pub text: &'static str,
    pub canonical: &'static str,
}

#[derive(Clone, Copy, Debug)]
pub struct CurrencyLexicon {
    pub words: &'static [CurrencyLexeme],
    pub ambiguous_words: &'static [AmbiguousCurrencyLexeme],
}

#[derive(Clone, Copy, Debug)]
pub struct AmbiguousCurrencyLexeme {
    pub text: &'static str,
    pub candidates: &'static [&'static str],
}

#[derive(Clone, Copy, Debug)]
pub struct MeridiemLexeme {
    pub text: &'static str,
    pub pm: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct RangeSeparatorLexicon {
    pub symbolic: &'static [&'static str],
}

impl RangeSeparatorLexicon {
    pub(crate) fn contains(self, text: &str) -> bool { self.symbolic.contains(&text) }
    pub(crate) fn contains_character(self, character: char) -> bool { self.symbolic.iter().any(|separator| separator.starts_with(character) && separator.chars().count() == 1) }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct QuotePairLexeme {
    pub open: char,
    pub close: char,
    pub family: DelimiterFamily,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PunctuationSpacingRule {
    punctuation: PunctuationPattern,
    side: PunctuationSpacingSide,
    preference: WhitespaceTypographyPreference,
}

impl PunctuationSpacingRule {
    pub(crate) fn applies_to_character(self, character: char) -> bool { self.punctuation == PunctuationPattern::Character(character) }
    pub(crate) fn applies_to_ellipsis(self) -> bool { self.punctuation == PunctuationPattern::Ellipsis }
    pub(crate) fn side(self) -> PunctuationSpacingSide { self.side }
    pub(crate) fn preference(self) -> WhitespaceTypographyPreference { self.preference }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PunctuationPattern {
    Character(char),
    Ellipsis,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PunctuationPolicy {
    pub paired_marks: &'static [QuotePairLexeme],
    spacing_rules: &'static [PunctuationSpacingRule],
    spacing_rule_delta: &'static [PunctuationSpacingRule],
    quote_interior_whitespace: Option<WhitespaceTypographyPreference>,
    bracket_interior_whitespace: Option<WhitespaceTypographyPreference>,
}

impl PunctuationPolicy {
    pub(crate) fn spacing_rules(self) -> impl Iterator<Item = &'static PunctuationSpacingRule> { self.spacing_rules.iter().chain(self.spacing_rule_delta) }
    pub(crate) fn quote_interior_whitespace(self) -> Option<WhitespaceTypographyPreference> { self.quote_interior_whitespace }
    pub(crate) fn bracket_interior_whitespace(self) -> Option<WhitespaceTypographyPreference> { self.bracket_interior_whitespace }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DateFieldOrder {
    MonthDayYear,
    DayMonthYear,
}

const DOT_FIRST_DECIMAL_SEPARATORS: &[char; 2] = &['.', ','];
const COMMA_FIRST_DECIMAL_SEPARATORS: &[char; 2] = &[',', '.'];
const NUMBER_GROUPING_SEPARATORS: &[char; 7] = &[',', '.', ' ', '\u{00A0}', '\u{202F}', '\'', '’'];
const DOT_LOCALE_GROUPING_SEPARATORS: &[char; 4] = &[',', ' ', '\u{00A0}', '\u{202F}'];
const COMMA_LOCALE_GROUPING_SEPARATORS: &[char; 4] = &['.', ' ', '\u{00A0}', '\u{202F}'];
const SWISS_GROUPING_SEPARATORS: &[char; 5] = &['\'', '’', ' ', '\u{00A0}', '\u{202F}'];
const DOT_NUMBER_POLICY: NumberParsingPolicy = NumberParsingPolicy { decimal_separators: DOT_FIRST_DECIMAL_SEPARATORS, grouping_separators: NUMBER_GROUPING_SEPARATORS, preferred_grouping_separators: DOT_LOCALE_GROUPING_SEPARATORS, canonical_grouping_separator: ',' };
const COMMA_NUMBER_POLICY: NumberParsingPolicy = NumberParsingPolicy { decimal_separators: COMMA_FIRST_DECIMAL_SEPARATORS, grouping_separators: NUMBER_GROUPING_SEPARATORS, preferred_grouping_separators: COMMA_LOCALE_GROUPING_SEPARATORS, canonical_grouping_separator: '.' };
const SWEDISH_NUMBER_POLICY: NumberParsingPolicy = NumberParsingPolicy { decimal_separators: COMMA_FIRST_DECIMAL_SEPARATORS, grouping_separators: NUMBER_GROUPING_SEPARATORS, preferred_grouping_separators: COMMA_LOCALE_GROUPING_SEPARATORS, canonical_grouping_separator: '\u{00A0}' };
const SWISS_NUMBER_POLICY: NumberParsingPolicy = NumberParsingPolicy { decimal_separators: DOT_FIRST_DECIMAL_SEPARATORS, grouping_separators: NUMBER_GROUPING_SEPARATORS, preferred_grouping_separators: SWISS_GROUPING_SEPARATORS, canonical_grouping_separator: '\'' };
const TIME_SEPARATORS: &[char; 1] = &[':'];
const TIME_SEPARATORS_WITH_DOT: &[char; 2] = &[':', '.'];
const DEFAULT_TIME_FORMAT: TimeFormatPolicy = TimeFormatPolicy { separators: TIME_SEPARATORS, suffixes: &[] };
const GERMAN_TIME_FORMAT: TimeFormatPolicy = TimeFormatPolicy { separators: TIME_SEPARATORS_WITH_DOT, suffixes: &["Uhr"] };
const GERMAN_SWISS_TIME_FORMAT: TimeFormatPolicy = TimeFormatPolicy { separators: TIME_SEPARATORS_WITH_DOT, suffixes: &["Uhr", "h"] };

#[derive(Clone, Copy, Debug)]
pub struct LanguageProfile {
    parse_language: ParseLanguage,
    function_words: &'static [&'static str],
    number_words: &'static [(&'static str, i64)],
    unit_lexicon: &'static UnitLexicon,
    pub primary_language: &'static str,
    pub aliases: &'static [&'static str],
    pub abbreviation: AbbreviationPolicy,
    pub months: &'static [(&'static str, u32)],
    pub currencies: CurrencyLexicon,
    pub home_currency: Option<&'static str>,
    pub meridiems: &'static [MeridiemLexeme],
    pub range_separators: RangeSeparatorLexicon,
    #[cfg(any(test, feature = "localization"))]
    pub number_policy: NumberParsingPolicy,
    pub join_apostrophe_words: bool,
    pub(crate) quote_pairs: &'static [QuotePairLexeme],
    pub(crate) preferred_quotation_style: Option<QuotationStyle>,
    pub(crate) preferred_ordinal_style: OrdinalStyle,
}

impl LanguageProfile {
    pub(crate) fn number_word_integer(&self, word: &str) -> Option<i64> {
        let normalized = word.to_lowercase();
        self.number_words.iter().find_map(|&(candidate, value)| (candidate == normalized.as_str()).then_some(value))
    }
    pub(crate) fn unit_lexicon(&self) -> &'static UnitLexicon { self.unit_lexicon }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct UnitWordLexeme {
    unit: BaseUnit,
    spellings: &'static [&'static str],
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SourceGroundedCompoundLexeme {
    canonical: &'static str,
    spellings: &'static [&'static str],
}

impl SourceGroundedCompoundLexeme {
    pub(crate) const fn new(canonical: &'static str, spellings: &'static [&'static str]) -> Self { Self { canonical, spellings } }
    pub(crate) fn canonical(self) -> &'static str { self.canonical }
    pub(crate) fn spellings(self) -> &'static [&'static str] { self.spellings }
}

impl UnitWordLexeme {
    pub(crate) const fn new(unit: BaseUnit, spellings: &'static [&'static str]) -> Self { Self { unit, spellings } }
    pub(crate) fn unit(self) -> BaseUnit { self.unit }
    pub(crate) fn spellings(self) -> &'static [&'static str] { self.spellings }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum UnitAliasTarget {
    Unit(BaseUnit),
    Canonical(&'static str),
    Scoped,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct UnitAliasLexeme {
    spelling: &'static str,
    target: UnitAliasTarget,
}

impl UnitAliasLexeme {
    pub(crate) const fn unit(spelling: &'static str, unit: BaseUnit) -> Self { Self { spelling, target: UnitAliasTarget::Unit(unit) } }
    pub(crate) const fn canonical(spelling: &'static str, canonical: &'static str) -> Self { Self { spelling, target: UnitAliasTarget::Canonical(canonical) } }
    pub(crate) const fn scoped(spelling: &'static str) -> Self { Self { spelling, target: UnitAliasTarget::Scoped } }
    pub(crate) fn spelling(self) -> &'static str { self.spelling }
    pub(crate) fn target(self) -> UnitAliasTarget { self.target }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct UnitLexicon {
    prefix_stems: &'static [&'static str; 14],
    words: &'static [UnitWordLexeme],
    source_grounded_words: &'static [UnitWordLexeme],
    source_grounded_compounds: &'static [SourceGroundedCompoundLexeme],
    aliases: &'static [UnitAliasLexeme],
    ambiguous_after_number: &'static [&'static str],
}

impl UnitLexicon {
    pub(crate) const fn new(prefix_stems: &'static [&'static str; 14], words: &'static [UnitWordLexeme], source_grounded_words: &'static [UnitWordLexeme], source_grounded_compounds: &'static [SourceGroundedCompoundLexeme], aliases: &'static [UnitAliasLexeme], ambiguous_after_number: &'static [&'static str]) -> Self { Self { prefix_stems, words, source_grounded_words, source_grounded_compounds, aliases, ambiguous_after_number } }
    pub(crate) fn prefix_stem(self, index: usize) -> &'static str { self.prefix_stems[index] }
    pub(crate) fn words(self) -> &'static [UnitWordLexeme] { self.words }
    pub(crate) fn source_grounded_words(self) -> &'static [UnitWordLexeme] { self.source_grounded_words }
    pub(crate) fn source_grounded_compounds(self) -> &'static [SourceGroundedCompoundLexeme] { self.source_grounded_compounds }
    pub(crate) fn alias(self, spelling: &str) -> Option<UnitAliasLexeme> { self.aliases.iter().copied().find(|alias| alias.spelling() == spelling) }
    pub(crate) fn is_ambiguous_after_number(self, spelling: &str) -> bool { self.ambiguous_after_number.contains(&spelling) }
}

const NEUTRAL_PREFIX_STEMS: &[&str; 14] = &["", "", "", "", "", "", "", "", "", "", "", "", "", ""];
const NEUTRAL_UNIT_LEXICON: UnitLexicon = UnitLexicon::new(NEUTRAL_PREFIX_STEMS, &[], &[], &[], &[], &[]);

pub(crate) fn unit_alias_is_language_scoped(spelling: &str) -> bool { LANGUAGE_PROFILES.iter().any(|profile| profile.unit_lexicon().alias(spelling).is_some()) }

pub(crate) const UNIVERSAL_CURRENCY_SYMBOLS: &[CurrencyLexeme] = &[
    CurrencyLexeme { text: "€", canonical: "EUR" },
    CurrencyLexeme { text: "$", canonical: "USD" },
    CurrencyLexeme { text: "£", canonical: "GBP" },
    CurrencyLexeme { text: "¥", canonical: "JPY" },
    CurrencyLexeme { text: "kr", canonical: "SEK" },
    CurrencyLexeme { text: "¢", canonical: "USD" },
    CurrencyLexeme { text: "¤", canonical: "USD" },
    CurrencyLexeme { text: "₡", canonical: "CRC" },
    CurrencyLexeme { text: "₫", canonical: "VND" },
    CurrencyLexeme { text: "₣", canonical: "CHF" },
    CurrencyLexeme { text: "₤", canonical: "GBP" },
    CurrencyLexeme { text: "₧", canonical: "ESP" },
    CurrencyLexeme { text: "₪", canonical: "ILS" },
];
pub(crate) const UNIVERSAL_CURRENCY_CODES: &[CurrencyLexeme] = &[
    CurrencyLexeme { text: "EUR", canonical: "EUR" },
    CurrencyLexeme { text: "USD", canonical: "USD" },
    CurrencyLexeme { text: "GBP", canonical: "GBP" },
    CurrencyLexeme { text: "JPY", canonical: "JPY" },
    CurrencyLexeme { text: "SEK", canonical: "SEK" },
    CurrencyLexeme { text: "NOK", canonical: "NOK" },
    CurrencyLexeme { text: "DKK", canonical: "DKK" },
];

pub(crate) const ENGLISH_CURRENCY_WORDS: &[CurrencyLexeme] = &[
    CurrencyLexeme { text: "euro", canonical: "EUR" },
    CurrencyLexeme { text: "euros", canonical: "EUR" },
    CurrencyLexeme { text: "dollar", canonical: "USD" },
    CurrencyLexeme { text: "dollars", canonical: "USD" },
    CurrencyLexeme { text: "yen", canonical: "JPY" },
    CurrencyLexeme { text: "norwegian krone", canonical: "NOK" },
    CurrencyLexeme { text: "norwegian kroner", canonical: "NOK" },
    CurrencyLexeme { text: "danish krone", canonical: "DKK" },
    CurrencyLexeme { text: "danish kroner", canonical: "DKK" },
];

pub(crate) const SWEDISH_CURRENCY_WORDS: &[CurrencyLexeme] = &[
    CurrencyLexeme { text: "euro", canonical: "EUR" },
    CurrencyLexeme { text: "dollar", canonical: "USD" },
    CurrencyLexeme { text: "yen", canonical: "JPY" },
    CurrencyLexeme { text: "norsk krona", canonical: "NOK" },
    CurrencyLexeme { text: "norska kronor", canonical: "NOK" },
    CurrencyLexeme { text: "dansk krona", canonical: "DKK" },
    CurrencyLexeme { text: "danska kronor", canonical: "DKK" },
];

pub(crate) const GERMAN_CURRENCY_WORDS: &[CurrencyLexeme] = &[
    CurrencyLexeme { text: "euro", canonical: "EUR" },
    CurrencyLexeme { text: "dollar", canonical: "USD" },
    CurrencyLexeme { text: "yen", canonical: "JPY" },
    CurrencyLexeme { text: "norwegische krone", canonical: "NOK" },
    CurrencyLexeme { text: "norwegische kronen", canonical: "NOK" },
    CurrencyLexeme { text: "dänische krone", canonical: "DKK" },
    CurrencyLexeme { text: "dänische kronen", canonical: "DKK" },
];

const KRONE_CANDIDATES: &[&str] = &["NOK", "DKK", "SEK", "CZK"];
const ENGLISH_AMBIGUOUS_CURRENCY_WORDS: &[AmbiguousCurrencyLexeme] = &[
    AmbiguousCurrencyLexeme { text: "krona", candidates: KRONE_CANDIDATES },
    AmbiguousCurrencyLexeme { text: "kronor", candidates: KRONE_CANDIDATES },
    AmbiguousCurrencyLexeme { text: "krone", candidates: KRONE_CANDIDATES },
    AmbiguousCurrencyLexeme { text: "kroner", candidates: KRONE_CANDIDATES },
];
const SWEDISH_AMBIGUOUS_CURRENCY_WORDS: &[AmbiguousCurrencyLexeme] = &[
    AmbiguousCurrencyLexeme { text: "krona", candidates: KRONE_CANDIDATES },
    AmbiguousCurrencyLexeme { text: "kronor", candidates: KRONE_CANDIDATES },
];
const GERMAN_AMBIGUOUS_CURRENCY_WORDS: &[AmbiguousCurrencyLexeme] = &[
    AmbiguousCurrencyLexeme { text: "krone", candidates: KRONE_CANDIDATES },
    AmbiguousCurrencyLexeme { text: "kronen", candidates: KRONE_CANDIDATES },
];

pub(crate) const ENGLISH_CURRENCY_LEXICON: CurrencyLexicon = CurrencyLexicon { words: ENGLISH_CURRENCY_WORDS, ambiguous_words: ENGLISH_AMBIGUOUS_CURRENCY_WORDS };
pub(crate) const SWEDISH_CURRENCY_LEXICON: CurrencyLexicon = CurrencyLexicon { words: SWEDISH_CURRENCY_WORDS, ambiguous_words: SWEDISH_AMBIGUOUS_CURRENCY_WORDS };
pub(crate) const GERMAN_CURRENCY_LEXICON: CurrencyLexicon = CurrencyLexicon { words: GERMAN_CURRENCY_WORDS, ambiguous_words: GERMAN_AMBIGUOUS_CURRENCY_WORDS };

pub(crate) const ENGLISH_MERIDIEMS: &[MeridiemLexeme] = &[
    MeridiemLexeme { text: "a.m.", pm: false },
    MeridiemLexeme { text: "am", pm: false },
    MeridiemLexeme { text: "p.m.", pm: true },
    MeridiemLexeme { text: "pm", pm: true },
];

pub(crate) const NO_MERIDIEMS: &[MeridiemLexeme] = &[];
pub(crate) const SYMBOLIC_RANGE_SEPARATORS: &[&str] = &["-", "‐", "‑", "‒", "–", "—", "―", "−", "~", "～"];
pub(crate) const DEFAULT_RANGE_SEPARATORS: RangeSeparatorLexicon = RangeSeparatorLexicon { symbolic: SYMBOLIC_RANGE_SEPARATORS };
pub(crate) const WESTERN_QUOTE_PAIRS: &[QuotePairLexeme] = &[
    QuotePairLexeme { open: '“', close: '”', family: DelimiterFamily::DoubleQuote },
    QuotePairLexeme { open: '„', close: '“', family: DelimiterFamily::DoubleQuote },
    QuotePairLexeme { open: '”', close: '”', family: DelimiterFamily::DoubleQuote },
    QuotePairLexeme { open: '«', close: '»', family: DelimiterFamily::DoubleQuote },
    QuotePairLexeme { open: '»', close: '«', family: DelimiterFamily::DoubleQuote },
    QuotePairLexeme { open: '"', close: '"', family: DelimiterFamily::DoubleQuote },
    QuotePairLexeme { open: '‘', close: '’', family: DelimiterFamily::SingleQuote },
    QuotePairLexeme { open: '‚', close: '‘', family: DelimiterFamily::SingleQuote },
    QuotePairLexeme { open: '’', close: '’', family: DelimiterFamily::SingleQuote },
    QuotePairLexeme { open: '‹', close: '›', family: DelimiterFamily::SingleQuote },
    QuotePairLexeme { open: '›', close: '‹', family: DelimiterFamily::SingleQuote },
    QuotePairLexeme { open: '\'', close: '\'', family: DelimiterFamily::SingleQuote },
];
const SPANISH_PAIRED_MARKS: &[QuotePairLexeme] = &[
    QuotePairLexeme { open: '¿', close: '?', family: DelimiterFamily::Question },
    QuotePairLexeme { open: '¡', close: '!', family: DelimiterFamily::Exclamation },
];
const NO_PUNCTUATION_SPACE: WhitespaceTypographyPreference = WhitespaceTypographyPreference::exact(WhitespaceTypographyKind::Absent);
const REGULAR_PUNCTUATION_SPACE: WhitespaceTypographyPreference = WhitespaceTypographyPreference::any_present(WhitespaceTypographyKind::Regular);
const NON_BREAKING_PUNCTUATION_SPACE: WhitespaceTypographyPreference = WhitespaceTypographyPreference::any_non_breaking(WhitespaceTypographyKind::NonBreaking);
const COMMON_SPACING_RULES: &[PunctuationSpacingRule] = &[
    PunctuationSpacingRule { punctuation: PunctuationPattern::Character('.'), side: PunctuationSpacingSide::Before, preference: NO_PUNCTUATION_SPACE },
    PunctuationSpacingRule { punctuation: PunctuationPattern::Character(','), side: PunctuationSpacingSide::Before, preference: NO_PUNCTUATION_SPACE },
    PunctuationSpacingRule { punctuation: PunctuationPattern::Character(':'), side: PunctuationSpacingSide::Before, preference: NO_PUNCTUATION_SPACE },
    PunctuationSpacingRule { punctuation: PunctuationPattern::Character(';'), side: PunctuationSpacingSide::Before, preference: NO_PUNCTUATION_SPACE },
    PunctuationSpacingRule { punctuation: PunctuationPattern::Character('?'), side: PunctuationSpacingSide::Before, preference: NO_PUNCTUATION_SPACE },
    PunctuationSpacingRule { punctuation: PunctuationPattern::Character('!'), side: PunctuationSpacingSide::Before, preference: NO_PUNCTUATION_SPACE },
    PunctuationSpacingRule { punctuation: PunctuationPattern::Character('.'), side: PunctuationSpacingSide::After, preference: REGULAR_PUNCTUATION_SPACE },
    PunctuationSpacingRule { punctuation: PunctuationPattern::Character(','), side: PunctuationSpacingSide::After, preference: REGULAR_PUNCTUATION_SPACE },
    PunctuationSpacingRule { punctuation: PunctuationPattern::Character(':'), side: PunctuationSpacingSide::After, preference: REGULAR_PUNCTUATION_SPACE },
    PunctuationSpacingRule { punctuation: PunctuationPattern::Character(';'), side: PunctuationSpacingSide::After, preference: REGULAR_PUNCTUATION_SPACE },
    PunctuationSpacingRule { punctuation: PunctuationPattern::Character('?'), side: PunctuationSpacingSide::After, preference: REGULAR_PUNCTUATION_SPACE },
    PunctuationSpacingRule { punctuation: PunctuationPattern::Character('!'), side: PunctuationSpacingSide::After, preference: REGULAR_PUNCTUATION_SPACE },
];
const ENGLISH_GERMAN_SPACING_DELTA: &[PunctuationSpacingRule] = &[
    PunctuationSpacingRule { punctuation: PunctuationPattern::Ellipsis, side: PunctuationSpacingSide::Before, preference: NO_PUNCTUATION_SPACE },
    PunctuationSpacingRule { punctuation: PunctuationPattern::Ellipsis, side: PunctuationSpacingSide::After, preference: REGULAR_PUNCTUATION_SPACE },
];
const SWEDISH_SPACING_DELTA: &[PunctuationSpacingRule] = &[
    PunctuationSpacingRule { punctuation: PunctuationPattern::Ellipsis, side: PunctuationSpacingSide::After, preference: REGULAR_PUNCTUATION_SPACE },
];
const EUROPEAN_FRENCH_SPACING_RULES: &[PunctuationSpacingRule] = &[
    PunctuationSpacingRule { punctuation: PunctuationPattern::Character(':'), side: PunctuationSpacingSide::Before, preference: NON_BREAKING_PUNCTUATION_SPACE },
    PunctuationSpacingRule { punctuation: PunctuationPattern::Character(';'), side: PunctuationSpacingSide::Before, preference: NON_BREAKING_PUNCTUATION_SPACE },
    PunctuationSpacingRule { punctuation: PunctuationPattern::Character('?'), side: PunctuationSpacingSide::Before, preference: NON_BREAKING_PUNCTUATION_SPACE },
    PunctuationSpacingRule { punctuation: PunctuationPattern::Character('!'), side: PunctuationSpacingSide::Before, preference: NON_BREAKING_PUNCTUATION_SPACE },
    PunctuationSpacingRule { punctuation: PunctuationPattern::Character('«'), side: PunctuationSpacingSide::After, preference: NON_BREAKING_PUNCTUATION_SPACE },
    PunctuationSpacingRule { punctuation: PunctuationPattern::Character('»'), side: PunctuationSpacingSide::Before, preference: NON_BREAKING_PUNCTUATION_SPACE },
];
const DEFAULT_PUNCTUATION_POLICY: PunctuationPolicy = PunctuationPolicy { paired_marks: &[], spacing_rules: &[], spacing_rule_delta: &[], quote_interior_whitespace: None, bracket_interior_whitespace: None };
const ENGLISH_GERMAN_PUNCTUATION_POLICY: PunctuationPolicy = PunctuationPolicy { paired_marks: &[], spacing_rules: COMMON_SPACING_RULES, spacing_rule_delta: ENGLISH_GERMAN_SPACING_DELTA, quote_interior_whitespace: Some(NO_PUNCTUATION_SPACE), bracket_interior_whitespace: Some(NO_PUNCTUATION_SPACE) };
const SWEDISH_PUNCTUATION_POLICY: PunctuationPolicy = PunctuationPolicy { paired_marks: &[], spacing_rules: COMMON_SPACING_RULES, spacing_rule_delta: SWEDISH_SPACING_DELTA, quote_interior_whitespace: Some(NO_PUNCTUATION_SPACE), bracket_interior_whitespace: Some(NO_PUNCTUATION_SPACE) };
const SPANISH_PUNCTUATION_POLICY: PunctuationPolicy = PunctuationPolicy { paired_marks: SPANISH_PAIRED_MARKS, spacing_rules: &[], spacing_rule_delta: &[], quote_interior_whitespace: None, bracket_interior_whitespace: None };
const EUROPEAN_FRENCH_PUNCTUATION_POLICY: PunctuationPolicy = PunctuationPolicy { paired_marks: &[], spacing_rules: EUROPEAN_FRENCH_SPACING_RULES, spacing_rule_delta: &[], quote_interior_whitespace: None, bracket_interior_whitespace: None };

pub const NEUTRAL_PROFILE: LanguageProfile = LanguageProfile {
    parse_language: ParseLanguage::Neutral,
    function_words: &[],
    number_words: &[],
    unit_lexicon: &NEUTRAL_UNIT_LEXICON,
    primary_language: "en",
    aliases: &[],
    abbreviation: AbbreviationPolicy::new(&[], false),
    months: ENGLISH_MONTHS,
    currencies: ENGLISH_CURRENCY_LEXICON,
    home_currency: None,
    meridiems: ENGLISH_MERIDIEMS,
    range_separators: DEFAULT_RANGE_SEPARATORS,
    #[cfg(any(test, feature = "localization"))]
    number_policy: DOT_NUMBER_POLICY,
    join_apostrophe_words: false,
    quote_pairs: WESTERN_QUOTE_PAIRS,
    preferred_quotation_style: Some(QuotationStyle::CurlyOpenClose),
    preferred_ordinal_style: OrdinalStyle::EnglishSuffix,
};

pub(crate) fn language_text_eq(left: &str, right: &str) -> bool { crate::unicode_case_fold(left) == crate::unicode_case_fold(right) }

pub fn is_function_word_for_language(lang_tag: &str, word: &str) -> bool { LanguageTag::parse(lang_tag).supported_profile().is_some_and(|profile| profile.function_words.iter().any(|candidate| language_text_eq(candidate, word))) }

#[cfg(feature = "dictionary")]
pub use crate::dictionary_annotation::DictionaryLookup;

#[cfg(feature = "hunspell")]
pub struct HunspellDictionary {
    hunspell: Option<Hunspell>,
}

#[cfg(feature = "hunspell")]
impl Default for HunspellDictionary {
    fn default() -> Self { Self::for_language("en") }
}

#[cfg(feature = "hunspell")]
impl std::fmt::Debug for HunspellDictionary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HunspellDictionary")
            .field("hunspell", &self.hunspell.as_ref().map(|_| "Hunspell(...)"))
            .finish()
    }
}

#[cfg(feature = "hunspell")]
impl HunspellDictionary {
    fn new(hunspell: Option<Hunspell>) -> Self { Self { hunspell } }
    pub fn for_language(lang_tag: &str) -> Self { Self::new(load_system_hunspell(lang_tag)) }
    pub fn none() -> Self { Self::new(None) }
}

#[cfg(feature = "hunspell")]
impl DictionaryLookup for HunspellDictionary {
    fn is_dictionary_word(&self, word: &str) -> bool { is_dictionary_word_with_hunspell(self.hunspell.as_ref(), word) }
    fn dictionary_baseform(&self, word: &str) -> Option<String> { dictionary_baseform_with_hunspell(self.hunspell.as_ref(), word) }
    fn has_dictionary(&self) -> bool { self.hunspell.is_some() }
    fn dictionary_analysis(&self, word: &str) -> Option<structure::DictionaryAnalysis> { dictionary_analysis_with_hunspell(self.hunspell.as_ref(), word) }
}

#[cfg(feature = "hunspell")]
fn load_system_hunspell(lang_tag: &str) -> Option<Hunspell> {
    load_bundled_dictionary(lang_tag)
}

#[cfg(feature = "localization")]
pub(crate) fn supported_language_profile(lang_tag: &str) -> Option<&'static LanguageProfile> { LanguageTag::parse(lang_tag).supported_profile() }

fn primary_language(lang_tag: &str) -> &str { lang_tag.split(['-', '_']).next().unwrap_or(lang_tag) }

fn locale_region(lang_tag: &str) -> Option<&str> { lang_tag.split(['-', '_']).skip(1).find(|part| part.len() == 2 && part.bytes().all(|byte| byte.is_ascii_alphabetic())) }

pub(crate) fn detect_abbreviation_for_profile(stream: &SegmentTree, start_idx: usize, profile: &LanguageProfile) -> Option<Range<usize>> {
    let first = structure::parser_api::units(stream).get(start_idx)?;
    if first.kind() == UnitKind::Whitespace || first.is_tag() {
        return None;
    }

    let end_idx = contiguous_stream_end(stream, start_idx);
    let contiguous = detect_abbreviation_for_profile_in_unit_range(stream, start_idx, end_idx, profile);
    let text = unit_range_text(stream, start_idx, end_idx)?;
    let normalized = text.to_lowercase();
    let extended = extend_abbreviation_across_space(stream, start_idx, end_idx, &normalized, profile).map(|ext_end_idx| structure::parser_api::unit_start(stream, start_idx).expect("unit start")..structure::parser_api::unit_end(stream, ext_end_idx - 1).expect("unit end"));
    extended.or(contiguous)
}

/// Whether `text` is an abbreviation listed in `language`'s recognition catalogs.
pub fn is_listed_abbreviation(language: ParseLanguage, text: &str) -> bool { abbreviation_policy_contains(language.profile().abbreviation, text, &text.to_lowercase()) }

pub fn trailing_abbreviation_span(stream: &SegmentTree, language: ParseLanguage) -> Option<Range<usize>> {
    let profile = language.profile();
    let end = stream.text().char_indices().rev().find(|(_, character)| !character.is_whitespace() && !is_closing_enclosure(*character)).map(|(start, character)| start + character.len_utf8())?;
    stream.text().get(..end)?.char_indices().filter_map(|(start, _)| {
        let before = stream.text().get(..start).and_then(|prefix| prefix.chars().next_back());
        if before.is_some_and(char::is_alphanumeric) {
            return None;
        }
        let suffix = stream.text().get(start..end)?;
        let exact = suffix.chars().filter(|character| !character.is_whitespace()).collect::<String>();
        let normalized = exact.to_lowercase();
        let span = start..end;
        (profile.abbreviation.allows_dotted_initialism() && is_dotted_initialism(&normalized) || abbreviation_has_required_evidence(stream, profile, &exact, &normalized, &span)).then_some(span)
    }).max_by_key(|span| span.start)
}

pub(crate) fn detect_abbreviation_for_profile_in_unit_range(stream: &SegmentTree, start_idx: usize, end_idx: usize, profile: &LanguageProfile) -> Option<Range<usize>> {
    let text = unit_range_text(stream, start_idx, end_idx)?;
    if text.chars().all(char::is_alphanumeric) {
        return None;
    }
    let normalized = text.to_lowercase();
    let is_initialism = profile.abbreviation.allows_dotted_initialism() && is_dotted_initialism(&normalized);
    let span = structure::parser_api::unit_start(stream, start_idx).expect("unit start")..structure::parser_api::unit_end(stream, end_idx - 1).expect("unit end");
    (is_initialism || abbreviation_has_required_evidence(stream, profile, text, &normalized, &span)).then_some(span)
}

pub(crate) fn detect_apostrophe_word_at(stream: &SegmentTree, start_idx: usize) -> Option<Range<usize>> {
    let first = structure::parser_api::units(stream).get(start_idx)?;
    if first.kind() != UnitKind::Word || first.is_tag() {
        return None;
    }
    let end_idx = apostrophe_word_end(stream, start_idx)?;
    Some(structure::parser_api::unit_start(stream, start_idx).expect("unit start")..structure::parser_api::unit_end(stream, end_idx - 1).expect("unit end"))
}

fn apostrophe_word_end(stream: &SegmentTree, start_idx: usize) -> Option<usize> {
    let mut idx = start_idx;
    let mut saw_apostrophe = false;
    while is_word_unit(stream, idx) && is_apostrophe_unit(stream, idx + 1) && is_word_unit(stream, idx + 2) {
        saw_apostrophe = true;
        idx += 2;
    }
    saw_apostrophe.then_some(idx + 1)
}

fn is_word_unit(stream: &SegmentTree, idx: usize) -> bool { structure::parser_api::units(stream).get(idx).is_some_and(|unit| unit.kind() == UnitKind::Word && !unit.is_tag()) }

fn is_apostrophe_unit(stream: &SegmentTree, idx: usize) -> bool { structure::parser_api::units(stream).get(idx).is_some_and(|unit| unit.kind() == UnitKind::Punct && matches!(structure::parser_api::unit_text(stream, idx), Some("'" | "’"))) }

fn abbreviation_has_required_evidence(stream: &SegmentTree, profile: &LanguageProfile, exact: &str, normalized: &str, span: &Range<usize>) -> bool {
    abbreviation_evidence(profile, exact, normalized).is_some_and(|evidence| evidence.is_satisfied_by(stream.text(), span.clone()))
}

fn abbreviation_evidence(profile: &LanguageProfile, exact: &str, normalized: &str) -> Option<AbbreviationEvidence> { abbreviation_policy_evidence(profile.abbreviation, exact, normalized) }

/// Spans of final periods that end a terminal-compatible abbreviation with no continuation after it.
pub fn terminal_compatible_abbreviation_periods(stream: &SegmentTree, language: ParseLanguage) -> Vec<Range<usize>> {
    stream.preorder_iter()
        .filter(|node| node.kind() == structure::NodeKind::Abbreviation && node.text().ends_with('.'))
        .filter(|node| {
            let exact = node.text();
            let normalized = exact.to_lowercase();
            abbreviation_evidence(language.profile(), exact, &normalized) == Some(AbbreviationEvidence::TerminalCompatible) && !has_continuation_evidence(stream.text(), node.span().end)
        })
        .map(|node| node.span().end - 1..node.span().end)
        .collect()
}

pub(crate) fn month_number_for_profile(profile: &LanguageProfile, text: &str) -> Option<u32> {
    profile
        .months
        .iter()
        .find_map(|(month, number)| language_text_eq(month, text).then_some(*number))
}

fn extend_abbreviation_across_space(stream: &SegmentTree, start_idx: usize, end_idx: usize, first_normalized: &str, profile: &LanguageProfile) -> Option<usize> {
    if !first_normalized.ends_with('.') {
        return None;
    }
    let mut current_end = end_idx;
    let mut best = None;
    for _ in 0..4 {
        let whitespace = structure::parser_api::units(stream).get(current_end)?;
        if whitespace.kind() != UnitKind::Whitespace {
            break;
        }
        let next_start = current_end + 1;
        let next_end = contiguous_stream_end(stream, next_start);
        if next_end <= next_start {
            break;
        }
        let combined = unit_range_text(stream, start_idx, next_end)?;
        let exact = combined.chars().filter(|character| !character.is_whitespace()).collect::<String>();
        let normalized = exact.to_lowercase();
        if !abbreviation_policy_has_prefix(profile.abbreviation, &exact, &normalized) {
            break;
        }
        if abbreviation_policy_contains(profile.abbreviation, &exact, &normalized) {
            best = Some(next_end);
        }
        current_end = next_end;
    }
    best
}

#[cfg(feature = "hunspell")]
pub fn dictionary_for_language(lang_tag: &str) -> Box<dyn DictionaryLookup> {
    Box::new(HunspellDictionary::for_language(lang_tag))
}

#[cfg(all(feature = "dictionary", not(feature = "hunspell")))]
pub fn dictionary_for_language(_lang_tag: &str) -> Box<dyn DictionaryLookup> { Box::new(UnavailableDictionary) }

#[cfg(all(feature = "dictionary", not(feature = "hunspell")))]
struct UnavailableDictionary;

#[cfg(all(feature = "dictionary", not(feature = "hunspell")))]
impl DictionaryLookup for UnavailableDictionary {
    fn is_dictionary_word(&self, _word: &str) -> bool { false }
    fn dictionary_baseform(&self, _word: &str) -> Option<String> { None }
    fn has_dictionary(&self) -> bool { false }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_language_from_bcp47_falls_back_to_english() {
        assert_eq!(ParseLanguage::from_bcp47("en-US"), ParseLanguage::English);
        assert_eq!(ParseLanguage::from_bcp47("sv-SE"), ParseLanguage::Swedish);
        assert_eq!(ParseLanguage::from_bcp47("de-DE"), ParseLanguage::German);
        assert_eq!(ParseLanguage::from_bcp47("xx"), ParseLanguage::English);
    }

    #[test]
    fn neutral_profile_uses_noop_defaults() {
        let profile = NEUTRAL_PROFILE;
        assert_eq!(profile.primary_language, "en");
        assert!(!profile.join_apostrophe_words);
        assert_eq!(profile.number_policy.decimal_separators(), DOT_FIRST_DECIMAL_SEPARATORS);
        assert_eq!(profile.number_policy.grouping_separators(), NUMBER_GROUPING_SEPARATORS);
        assert_eq!(ParseLocale::from_language(ParseLanguage::Neutral).date_field_order(), DateFieldOrder::MonthDayYear);
        assert!(!profile.abbreviation.allows_dotted_initialism());
        assert!(profile.abbreviation.catalogs().is_empty());
    }

    #[test]
    fn number_policy_uses_language_and_region() {
        let decimal_separator_for_language = |lang_tag| LanguageTag::parse(lang_tag).number_policy().decimal_separators()[0];
        assert_eq!(decimal_separator_for_language("en-GB"), '.');
        assert_eq!(decimal_separator_for_language("de-DE"), ',');
        assert_eq!(decimal_separator_for_language("fr-FR"), ',');
        assert_eq!(decimal_separator_for_language("de-CH"), '.');
        assert_eq!(decimal_separator_for_language("swe"), ',');
    }

    #[test]
    fn date_field_order_uses_language_and_region() {
        ["en", "en-US", "en-PH", "en-GU"].into_iter().for_each(|language| assert_eq!(ParseLocale::from_bcp47(language).date_field_order(), DateFieldOrder::MonthDayYear, "{language}"));
        ["en-GB", "en-AU", "en-CA", "en-IE", "en-NZ", "en-ZA", "de-DE", "de-CH", "sv-SE", "sv-FI"].into_iter().for_each(|language| assert_eq!(ParseLocale::from_bcp47(language).date_field_order(), DateFieldOrder::DayMonthYear, "{language}"));
        assert_eq!(ParseLocale::from_bcp47("xx-DE").date_field_order(), DateFieldOrder::MonthDayYear);
    }

    #[test]
    fn ambiguous_numeric_dates_follow_the_regional_order() {
        let parsed = |language| {
            let mut tree = crate::tokenized_text_tree("03/07/2026");
            crate::parse(&mut tree, language);
            crate::date_facts(&tree).next().expect("date").date_fact()
        };
        let us = parsed("en-US");
        let gb = parsed("en-GB");
        assert_eq!((us.month(), us.day()), (3, 7));
        assert_eq!((gb.month(), gb.day()), (7, 3));
    }

    #[test]
    fn measurement_system_uses_region_without_guessing_from_language() {
        assert_eq!(ParseLocale::from_bcp47("en-GB").measurement_system(), Some(MeasurementSystem::ImperialUk));
        assert_eq!(ParseLocale::from_bcp47("cy_GB").measurement_system(), Some(MeasurementSystem::ImperialUk));
        assert_eq!(ParseLocale::from_bcp47("en-US").measurement_system(), Some(MeasurementSystem::UsCustomary));
        assert_eq!(ParseLocale::from_bcp47("es_US").measurement_system(), Some(MeasurementSystem::UsCustomary));
        assert_eq!(ParseLocale::from_bcp47("en").measurement_system(), None);
        assert_eq!(ParseLocale::from_bcp47("en-CA").measurement_system(), None);
        assert_eq!(ParseLocale::from_bcp47("sv-SE").measurement_system(), None);
        assert_eq!(ParseLocale::from_language(ParseLanguage::English).measurement_system(), None);
    }

    #[test]
    fn time_format_is_region_sensitive_for_german() {
        let default = ParseLocale::from_language(ParseLanguage::German);
        assert_eq!(default.time_format().separators(), TIME_SEPARATORS_WITH_DOT);
        assert_eq!(default.time_format().suffixes(), &["Uhr"]);

        let swiss = ParseLocale::from_bcp47("de-CH");
        assert_eq!(swiss.time_format().separators(), TIME_SEPARATORS_WITH_DOT);
        assert_eq!(swiss.time_format().suffixes(), &["Uhr", "h"]);

        let swedish = ParseLocale::from_language(ParseLanguage::Swedish);
        assert_eq!(swedish.time_format().separators(), TIME_SEPARATORS);
        assert!(swedish.time_format().suffixes().is_empty());
    }

    #[test]
    fn language_lexicons_use_unicode_case_folding_without_stripping_diacritics() {
        assert_eq!(month_number_for_profile(ParseLanguage::German.profile(), "MÄRZ"), Some(3));
        assert!(is_function_word_for_language("de-DE", "ÜBER"));
        assert!(is_function_word_for_language("sv-SE", "FÖR"));
        assert!(language_text_eq("Straße", "STRASSE"));
        assert!(language_text_eq("ς", "σ"));
        assert!(!language_text_eq("für", "fur"));
        assert!(!language_text_eq("schön", "schon"));
    }

    #[test]
    fn language_profile_falls_back_to_english() {
        assert_eq!(LanguageTag::parse("en-US").profile().primary_language, "en");
        assert_eq!(LanguageTag::parse("xx").profile().primary_language, "en");
    }

    #[test]
    fn function_words_are_language_specific_without_unknown_language_fallback() {
        assert!(is_function_word_for_language("en-US", "The"));
        assert!(is_function_word_for_language("sv-SE", "och"));
        assert!(is_function_word_for_language("de-DE", "und"));
        assert!(!is_function_word_for_language("sv-SE", "the"));
        assert!(!is_function_word_for_language("xx", "the"));
    }

    #[test]
    fn english_profile_declares_common_abbreviation_catalogs() {
        let profile = LanguageTag::parse("en-GB").profile();
        assert!(profile.abbreviation.allows_dotted_initialism());
        assert_eq!(profile.abbreviation.catalogs().len(), 4);
    }

    #[test]
    fn swedish_profile_disables_dotted_initialism() {
        let profile = LanguageTag::parse("sv-SE").profile();
        assert_eq!(profile.primary_language, "sv");
        assert!(!profile.abbreviation.allows_dotted_initialism());
    }
}
