pub use structure::{NodeKind, PartOfSpeech, SegmentTree, TermHit};
use structure::{Run, SegmentPiece, TokenizedSegment, UnitKind};
#[cfg(test)]
use structure::TagData;

/// Fixed-point scale of the numeric values exposed by parsed facts.
pub const NUMBER_VALUE_SCALE: i128 = parser::NORMALIZED_VALUE_SCALE as i128;

/// Existing localized vocabulary for a canonical measurement unit. Unknown
/// units and units without vocabulary in this language return an empty list.
pub fn measurement_unit_word_forms(unit: &str, language: ParseLanguage) -> Vec<String> { parser::measurement_unit_word_forms(unit, language) }

pub mod char_class;
mod compact_span;
pub use compact_span::{compact_offset, CompactSpan};
mod tag_scopes;
pub use tag_scopes::{common_tag_scope, tag_scope_facts, tag_scopes_covering_span, SegmentTagScopeFactRef, TagScopeKind};
#[cfg(feature = "text-analysis")]
mod part_of_speech;
#[cfg(feature = "text-analysis")]
pub use part_of_speech::{part_of_speech_from_folkets_class, part_of_speech_from_penn, unambiguous_folkets_part_of_speech};
#[cfg(feature = "text-analysis")]
mod part_of_speech_annotation;
#[cfg(feature = "text-analysis")]
pub use part_of_speech_annotation::{annotate_part_of_speech, PosTaggedToken};
#[cfg(feature = "dictionary")]
mod dictionary_annotation;
#[cfg(feature = "dictionary")]
pub use dictionary_annotation::{apply_dictionary, dictionary_matches, is_listed_dictionary_word, MemoizedDictionary};
#[cfg(feature = "terminology")]
mod terminology;
#[cfg(feature = "terminology")]
pub use terminology::{compound_term_aliases, discouraged_target_terminology_occurrences, exact_source_terminology_occurrences, grounded_target_terminology_occurrences, normalize_terminology_text, tag_scopes_equivalent, terminology_match_forms, terminology_occurrence};
#[cfg(feature = "terminology")]
mod term_matcher;
#[cfg(feature = "terminology")]
pub use term_matcher::{first_case_insensitive_match_span, first_text_match_span, merged_term_hits, primary_term_hits, term_match_coverage_allowed, TermMatcher};
#[cfg(feature = "stemming")]
mod lexical_normalization;
#[cfg(feature = "stemming")]
pub use lexical_normalization::{is_stopword, stem, stem_for_language};
mod tokenization;
pub use tokenization::semantic_unit_ranges;
#[cfg(feature = "terminology")]
pub use tokenization::build_run_vec_with_semantic_unit_ranges;
mod text_normalization;
pub use text_normalization::{canonical_search_text, unicode_case_fold};
#[cfg(feature = "text-analysis")]
mod content_words;
#[cfg(feature = "text-analysis")]
pub use content_words::{adjacent_repeated_words, caret_inside_completion_token, clamp_text_offset, completion_token_span, contains_same_normalized_with_at_least_alphabetic, normalized_content_words, normalized_content_words_with_spans, normalized_text, normalized_text_source_map, normalized_text_source_span, segment_content_words, segment_content_words_with_spans, segment_lexical_words_around, unicode_case_and_diacritic_fold};
#[cfg(feature = "text-analysis")]
mod consistency_projection;
#[cfg(feature = "text-analysis")]
pub use consistency_projection::consistency_projection;
mod placeholder;
pub use placeholder::{detect_placeholders, DetectedPlaceholder};
mod measurement_unit;
mod ordinal;
pub use ordinal::format_ordinal;
#[cfg(feature = "text-analysis")]
mod semantic_comparison;
mod node_facts;
pub use node_facts::{NodeFactKind, NodeFactRef, NumberFormattedFactKind};
mod semantic_facts;
pub use semantic_facts::{version_facts, SegmentVersionFactRef};
#[cfg(feature = "text-analysis")]
pub use semantic_comparison::{currency_presence_equivalent, measurement_numbers_equal, measurement_units_equivalent, number_range_values_equal, numeric_facts_value_equivalent};
mod web_facts;
pub use web_facts::{detect_emails, detect_urls, email_facts, email_facts_value_equivalent, url_facts, url_facts_value_equivalent, SegmentEmailFactRef, SegmentUrlFactRef};
mod language_parser;
#[cfg(feature = "localization")]
mod numeric_localization;
mod numeric_facts;
pub use numeric_facts::{
    all_numeric_facts, currency_facts, date_facts, mathematical_operator_spans, measurement_facts, number_facts,
    number_range_facts, numeric_facts, ordinal_facts, quantity_facts, time_facts,
    SegmentCurrencyFactRef, SegmentDateFactRef,
    SegmentMeasurementFactRef, SegmentNumberFactRef, SegmentNumberRangeFactRef,
    SegmentNumericFactRef, SegmentNumericKind, SegmentOrdinalFactRef, SegmentQuantityFactRef,
    SegmentTimeFactRef,
};
#[cfg(feature = "localization")]
mod date_time_localization;
mod parser;
mod stream_util;
pub use stream_util::{adjacent_whitespace_span, is_trim_punctuation};
pub use language_parser::{is_listed_abbreviation, terminal_compatible_abbreviation_periods, trailing_abbreviation_span};
use std::ops::Range;

pub use language_parser::{
    is_function_word_for_language,
    parenthetical_dash_whitespace_preference,
    MeasurementSystem,
    ParseLanguage,
    ParseLocale,
};
#[cfg(feature = "dictionary")]
pub use language_parser::{dictionary_for_language, DictionaryLookup};
#[cfg(feature = "hunspell")]
pub use language_parser::HunspellDictionary;
#[cfg(feature = "localization")]
pub use numeric_localization::{canonical_numeric_text, localize_measurements, localize_numbers};
#[cfg(feature = "localization")]
pub use date_time_localization::{localized_date_text, localized_time_text};

pub fn rebuild(tree: &mut SegmentTree, locale: impl Into<ParseLocale>) {
    tree.rebuild();
    parse(tree, locale);
}

pub fn parse(tree: &mut SegmentTree, locale: impl Into<ParseLocale>) { parse_with_known_units(tree, locale, &[]) }

pub fn parse_tokenized(segment: TokenizedSegment, locale: impl Into<ParseLocale>) -> SegmentTree { let mut tree = segment.into_tree_for_parser(); parse(&mut tree, locale); tree }

/// Tokenizes raw text and completes a locale-neutral parser pass.
pub fn parse_text(text: impl Into<String>) -> SegmentTree { parse_tokenized(TokenizedSegment::from_text(text), ParseLanguage::Neutral) }

pub fn parse_text_with_language(text: impl Into<String>, locale: impl Into<ParseLocale>) -> SegmentTree { parse_tokenized(TokenizedSegment::from_text(text), locale) }

/// Tokenizes lossless runs and completes a locale-neutral parser pass.
pub fn parse_runs(runs: impl IntoIterator<Item = Run>) -> SegmentTree { parse_tokenized(TokenizedSegment::from_runs(runs), ParseLanguage::Neutral) }

#[cfg(feature = "terminology")]
pub fn parse_tokenized_with_term_matcher(segment: TokenizedSegment, matcher: &TermMatcher, locale: impl Into<ParseLocale>) -> SegmentTree { rebuilt_with_term_matcher(&segment.into_tree_for_parser(), matcher, locale) }

#[cfg(any(test, feature = "localization"))]
pub(crate) fn tokenized_text_tree(text: impl Into<String>) -> SegmentTree { TokenizedSegment::from_text(text).into_tree_for_parser() }

#[cfg(test)]
pub(crate) fn tokenized_runs_tree(runs: impl IntoIterator<Item = Run>) -> SegmentTree { TokenizedSegment::from_runs(runs).into_tree_for_parser() }

#[cfg(feature = "terminology")]
pub(crate) fn tokenized_runs_with_resolved_spans_tree(runs: impl IntoIterator<Item = Run>, spans: &[(UnitKind, Range<usize>)]) -> SegmentTree { TokenizedSegment::from_runs_with_resolved_spans(runs, spans).into_tree_for_parser() }

/// Like `rebuild`, but grounded in `known_units` — canonical measurement
/// unit ids already found on the *other* side of the segment pair (read off
/// its typed `Measurement` node facts). Lets target
/// parsing recognize otherwise ambiguous unit spellings when the source
/// already used the same semantic unit. Localized full unit names are
/// recognized directly after numbers. Parsing source itself should use
/// `rebuild`/`parse` with no known units.
pub fn parse_with_known_units(tree: &mut SegmentTree, locale: impl Into<ParseLocale>, known_units: &[&str]) { let locale = locale.into(); apply_parse_with_semantic_units(tree, |tree| parser::parse_with_entities(tree, locale, known_units)); }

/// Parses a target tree using semantic evidence from its already parsed source.
/// Measurement vocabulary is grounded by the source's canonical units, and
/// spelled-out integers are grounded by numeric values present in the source.
/// Explicit currency expressions remain independently recognizable so QA can
/// report target-only currencies.
pub fn parse_target(tree: &mut SegmentTree, locale: impl Into<ParseLocale>, source: &SegmentTree) {
    let locale = locale.into();
    let known_units = known_measurement_units(source);
    let known_unit_refs = known_units.iter().map(String::as_str).collect::<Vec<_>>();
    let known_number_values = known_number_values(source);
    let known_currency_units = known_currency_units(source);
    let known_currency_refs = known_currency_units.iter().map(String::as_str).collect::<Vec<_>>();
    let known_non_translatables = source.protected_non_translatable_nodes().filter(|node| node.protected_kind() == Some(structure::ProtectedKind::Opaque)).map(|node| node.text()).collect::<Vec<_>>();
    clear_semantic_classification(tree, |kind, text| match kind {
        UnitKind::Currency => true,
        UnitKind::Number => text.chars().all(char::is_alphabetic),
        UnitKind::NonTranslatable => !known_non_translatables.contains(&text),
        _ => false,
    });
    apply_parse_with_semantic_units(tree, |tree| parser::parse_target_with_entities(tree, locale, &known_unit_refs, &known_number_values, &known_currency_refs, &known_non_translatables));
}

fn known_number_values(tree: &SegmentTree) -> Vec<i128> { sorted_unique(tree.iter_nodes().flat_map(|node| node.number_values().iter().copied())) }

fn known_currency_units(tree: &SegmentTree) -> Vec<String> { sorted_unique(tree.iter_nodes().filter_map(|node| node.currency_fact().and_then(|currency| currency.currency()).map(str::to_string))) }

fn sorted_unique<T: Ord>(items: impl Iterator<Item = T>) -> Vec<T> {
    let mut items = items.collect::<Vec<_>>();
    items.sort_unstable();
    items.dedup();
    items
}

fn clear_semantic_classification(tree: &mut SegmentTree, should_clear: impl Fn(UnitKind, &str) -> bool) {
    let unit_count = tree.unit_count();
    let mut changed = false;
    let pieces = tree.pieces_from_unit_range(0..unit_count).into_iter().map(|piece| {
        let clear = piece.semantic_text_content().is_some_and(|(kind, text)| should_clear(kind, text));
        if clear {
            changed = true;
            SegmentPiece::text(piece.text_content().unwrap_or(""))
        } else {
            piece
        }
    }).collect::<Vec<_>>();
    if changed {
        tree.splice_unit_pieces(0..unit_count, pieces);
    }
}

fn apply_parse_with_semantic_units(tree: &mut SegmentTree, parse: impl Fn(&SegmentTree) -> parser::ParseOutput) {
    // Parsing first discovers semantic spans against the lexical tree. If a
    // span crosses lexical unit boundaries, one semantic-unit retokenization
    // makes every discovered span atomic. Reapplying the same parse output is
    // sufficient because text offsets do not change and semantic atomic units
    // cannot introduce a new semantic span that was absent from the first pass.
    let parsed = parse(tree);
    let patterns = tokenization::semantic_unit_ranges_from_nodes(&parsed.nodes);
    if !semantic_ranges_are_units(tree, &patterns) {
        *tree = tree.rebuilt_with_resolved_spans(&patterns);
    }
    structure::parser_api::apply_parse_output(tree, parsed.nodes);
}

fn semantic_ranges_are_units(tree: &SegmentTree, patterns: &[(UnitKind, Range<usize>)]) -> bool { patterns.iter().all(|(kind, range)| tree.iter_units().any(|unit| unit.kind() == *kind && unit.span() == *range)) }

fn mutate_and_reparse<R>(tree: &mut SegmentTree, locale: ParseLocale, mutate: impl FnOnce(&mut SegmentTree) -> (R, bool)) -> R {
    let (result, changed) = mutate(tree);
    if changed {
        clear_semantic_classification(tree, |kind, _| kind != UnitKind::Term);
        parse(tree, locale);
    }
    result
}

fn mutate_range_and_reparse(tree: &mut SegmentTree, range: Range<usize>, locale: ParseLocale, mutate: impl FnOnce(&mut SegmentTree, Range<usize>) -> usize) -> usize {
    let removes_units = !range.is_empty();
    mutate_and_reparse(tree, locale, |tree| {
        let inserted = mutate(tree, range);
        (inserted, removes_units || inserted > 0)
    })
}

/// Canonical measurement units already found in `tree` (from its typed
/// `Measurement` node facts) — pass this as
/// `known_units` when parsing the tree's source/target counterpart so it can
/// recognize the same unit's expanded spellings.
pub fn known_measurement_units(tree: &SegmentTree) -> Vec<String> { sorted_unique(tree.iter_nodes().filter(|node| node.kind() == NodeKind::Measurement).filter_map(|node| node.measurement_fact().map(|fact| fact.unit().to_string()))) }

#[cfg(feature = "terminology")]
pub fn rebuilt_with_term_matcher(tree: &SegmentTree, matcher: &TermMatcher, locale: impl Into<ParseLocale>) -> SegmentTree { rebuilt_with_term_hits(tree, &matcher.hits(tree.text()), locale) }

#[cfg(feature = "terminology")]
pub fn rebuilt_with_term_matcher_if_matched(tree: &SegmentTree, matcher: &TermMatcher, locale: impl Into<ParseLocale>) -> Option<SegmentTree> { rebuilt_with_term_hits_if_matched(tree, &matcher.hits(tree.text()), locale) }

#[cfg(feature = "terminology")]
pub fn rebuilt_with_term_hits(tree: &SegmentTree, hits: &[TermHit], locale: impl Into<ParseLocale>) -> SegmentTree { rebuilt_with_resolved_term_hits(tree, hits, |tree| parse(tree, locale)) }

#[cfg(feature = "terminology")]
pub fn rebuilt_with_term_hits_if_matched(tree: &SegmentTree, hits: &[TermHit], locale: impl Into<ParseLocale>) -> Option<SegmentTree> { (has_term_annotations(tree) || !hits.is_empty()).then(|| rebuilt_with_term_hits(tree, hits, locale)) }

#[cfg(feature = "terminology")]
pub fn rebuilt_with_term_matcher_or_known_units(tree: &SegmentTree, known_units_source: &SegmentTree, matcher: &TermMatcher, locale: impl Into<ParseLocale>) -> SegmentTree { rebuilt_with_resolved_term_hits(tree, &matcher.hits(tree.text()), |tree| parse_target(tree, locale, known_units_source)) }

#[cfg(feature = "terminology")]
fn rebuilt_with_resolved_term_hits(tree: &SegmentTree, hits: &[TermHit], parse_tree: impl FnOnce(&mut SegmentTree)) -> SegmentTree {
    let hits = merged_term_hits(hits.iter().cloned());
    let primary = primary_term_hits(&hits);
    let mut tree = tree.rebuilt_with_resolved_term_hits(&primary, hits);
    parse_tree(&mut tree);
    tree
}

#[cfg(feature = "terminology")]
fn has_term_annotations(tree: &SegmentTree) -> bool { tree.has_term_units() || tree.node_kind_count(NodeKind::Term) > 0 || !tree.term_hits().is_empty() }

pub fn splice_units(tree: &mut SegmentTree, range: Range<usize>, inserted: impl IntoIterator<Item = Run>, locale: impl Into<ParseLocale>) -> usize { mutate_range_and_reparse(tree, range, locale.into(), |tree, range| tree.splice_units(range, inserted)) }

pub fn replace_range_with_text(tree: &mut SegmentTree, range: Range<usize>, replacement: &str, locale: impl Into<ParseLocale>) -> usize { mutate_range_and_reparse(tree, range, locale.into(), |tree, range| tree.splice_unit_pieces(range, [SegmentPiece::text(replacement)])) }

pub fn replace_range_with_term(tree: &mut SegmentTree, range: Range<usize>, text: &str, locale: impl Into<ParseLocale>) -> usize { mutate_range_and_reparse(tree, range, locale.into(), |tree, range| tree.splice_unit_pieces(range, [SegmentPiece::semantic_text(UnitKind::Term, text)])) }

pub fn replace_unit_text_range(tree: &mut SegmentTree, unit_index: usize, range: Range<usize>, replacement: &str, locale: impl Into<ParseLocale>) -> bool { mutate_and_reparse(tree, locale.into(), |tree| { let changed = tree.replace_unit_text_range(unit_index, range, replacement); (changed, changed) }) }

pub fn replace_text(tree: &mut SegmentTree, query: &str, replacement: &str, locale: impl Into<ParseLocale>) -> usize { mutate_and_reparse(tree, locale.into(), |tree| { let replaced = tree.replace_text(query, replacement); (replaced, replaced > 0) }) }

pub fn splice_unit_pieces(tree: &mut SegmentTree, range: Range<usize>, pieces: impl IntoIterator<Item = SegmentPiece>, locale: impl Into<ParseLocale>) -> usize { mutate_range_and_reparse(tree, range, locale.into(), |tree, range| tree.splice_unit_pieces(range, pieces)) }

#[cfg(test)]
mod integration_tests {
    use super::*;

    fn parsed_number(text: &str, language_tag: &str) -> SegmentTree {
        let mut tree = crate::tokenized_text_tree(text);
        parse(&mut tree, language_tag);
        tree
    }

    fn parsed_with_formatted_middle(before: &str, middle: &str, after: &str, language_tag: &str) -> SegmentTree {
        let mut run = Run::empty();
        run.push_text(before);
        run.push_tag(TagData::new("7".to_string(), "Start".to_string(), structure::TagKind::Open, "<Start>".to_string(), None, None));
        run.push_text(middle);
        run.push_tag(TagData::new("7".to_string(), "End".to_string(), structure::TagKind::Close, "</End>".to_string(), None, None));
        run.push_text(after);
        let mut tree = crate::tokenized_runs_tree([run]);
        parse(&mut tree, language_tag);
        tree
    }

    #[test]
    fn target_analysis_treats_tags_as_transparent_inside_a_measurement_phrase() {
        let source = parsed_number("Wait 5 days", "en-GB");
        let mut run = Run::empty();
        run.push_tag(TagData::new("7".to_string(), "Start".to_string(), structure::TagKind::Open, "<Start>".to_string(), None, None));
        run.push_text("5");
        run.push_tag(TagData::new("7".to_string(), "End".to_string(), structure::TagKind::Close, "</End>".to_string(), None, None));
        run.push_text(" dagar");
        let mut parsed = crate::tokenized_runs_tree([run]);
        parse_target(&mut parsed, "sv-SE", &source);

        assert!(measurement_facts(&parsed).next().is_some_and(|fact| fact.text() == "5 dagar"));
        assert_eq!(parsed.iter_tag_units().count(), 2);
    }

    #[test]
    fn atomic_numeric_facts_treat_internal_formatting_tags_as_transparent() {
        let number = parsed_with_formatted_middle("1", "234", ".5", "en-GB");
        let date = parsed_with_formatted_middle("2026-", "07", "-31", "en-GB");
        let time = parsed_with_formatted_middle("14:", "30", "", "en-GB");
        let version = parsed_with_formatted_middle("V2.", "3", ".1", "en-GB");
        let ordinal = parsed_with_formatted_middle("2", "n", "d", "en-GB");

        assert!(number_facts(&number).next().is_some_and(|fact| fact.text() == "1234.5"));
        assert!(date_facts(&date).next().is_some_and(|fact| fact.text() == "2026-07-31"));
        assert!(time_facts(&time).next().is_some_and(|fact| fact.text() == "14:30"));
        assert!(version_facts(&version).next().is_some_and(|fact| fact.text() == "V2.3.1"));
        assert!(ordinal_facts(&ordinal).next().is_some_and(|fact| fact.text() == "2nd"));
        [&number, &date, &time, &version, &ordinal].into_iter().for_each(|tree| assert_eq!(tree.iter_tag_units().count(), 2));
    }

    #[test]
    fn locale_resolves_ambiguous_decimal_and_grouping_separators() {
        let english = parsed_number("1.234", "en-GB");
        let german = parsed_number("1.234", "de-DE");
        let swiss = parsed_number("1’234", "de-CH");

        assert_eq!(english.iter_nodes().find(|node| node.kind() == NodeKind::Number).expect("English decimal").number_values(), &[12_340_000_000]);
        assert_eq!(german.iter_nodes().find(|node| node.kind() == NodeKind::Number).expect("German grouped integer").number_values(), &[12_340_000_000_000]);
        assert_eq!(swiss.iter_nodes().find(|node| node.kind() == NodeKind::Number).expect("Swiss grouped integer").number_values(), &[12_340_000_000_000]);
    }

    #[test]
    fn spaced_grouping_requires_one_to_three_leading_digits_and_three_digit_continuations() {
        for text in ["1 234", "12 345", "123 456", "1 234 567"] {
            let tree = parsed_number(text, "fr-FR");
            assert!(tree.iter_nodes().any(|node| node.kind() == NodeKind::Number && node.text() == text), "{text}");
        }
        for text in ["1234 567", "1 23", "12 345 67"] {
            let tree = parsed_number(text, "fr-FR");
            assert!(!tree.iter_nodes().any(|node| node.kind() == NodeKind::Number && node.text() == text), "{text}");
        }
    }

    #[test]
    fn parser_owns_number_range_structure_and_typography() {
        let compact = parsed_number("Use 10-20 items", "en");
        let canonical = parsed_number("Use 10–20 items", "en");
        let measurement = parsed_number("Use 10 - 20 mm", "en");

        let compact_range = number_range_facts(&compact).next().expect("compact number range");
        assert_eq!(compact_range.values(), Some((100_000_000_000, 200_000_000_000)));
        assert!(!compact_range.typography_is_canonical());
        assert!(number_range_facts(&canonical).next().is_some_and(|range| range.typography_is_canonical()));
        assert!(number_range_facts(&measurement).next().is_some_and(|range| !range.typography_is_canonical()));
    }

    #[test]
    fn mathematical_operators_require_numeric_operands() {
        let expression = parsed_number("5 × 4 = 20", "en");
        let subtraction = parsed_number("20 - 5 = 15", "en");
        let prose = parsed_number("state-of-the-art and/or", "en");
        let operators = |tree: &SegmentTree| mathematical_operator_spans(tree).into_iter().map(|span| tree.text()[span].to_string()).collect::<Vec<_>>();

        assert_eq!(operators(&expression), ["×", "="]);
        assert_eq!(number_range_facts(&subtraction).count(), 0);
        assert_eq!(operators(&subtraction), ["-", "="]);
        assert!(operators(&prose).is_empty());
    }

    #[test]
    fn mathematical_operators_need_numbers_on_both_sides() {
        ["value + 5", "5 + value", "x = 5", "5 = x", "+ 5", "5 +"].into_iter().for_each(|text| {
            let tree = parsed_number(text, "en");
            assert!(mathematical_operator_spans(&tree).is_empty(), "{text}");
        });
    }

    #[test]
    fn parser_emits_normalized_european_quote_pair_facts() {
        [
            ("“Save”", "en", structure::DelimiterFamily::DoubleQuote),
            ("„Speichern“", "de", structure::DelimiterFamily::DoubleQuote),
            ("”Spara”", "sv", structure::DelimiterFamily::DoubleQuote),
            ("«Enregistrer»", "fr", structure::DelimiterFamily::DoubleQuote),
            ("‘Save’", "en", structure::DelimiterFamily::SingleQuote),
        ].into_iter().for_each(|(text, language, family)| {
            let tree = parsed_number(text, language);
            let facts = tree.delimiter_facts().collect::<Vec<_>>();
            assert_eq!(facts.iter().filter(|fact| fact.role() == structure::DelimiterRole::Open && fact.family() == family).count(), 1, "{text}");
            assert_eq!(facts.iter().filter(|fact| fact.role() == structure::DelimiterRole::Close && fact.family() == family).count(), 1, "{text}");
            assert!(facts.iter().all(|fact| fact.role() != structure::DelimiterRole::Unmatched), "{text}");
        });
    }

    #[test]
    fn parser_emits_bracket_pairs_and_unmatched_delimiters() {
        let paired = parsed_number("(A [B] {C D})", "en");
        let unmatched = parsed_number("(A] \"B", "en");

        assert_eq!(paired.delimiter_facts().filter(|fact| fact.role() == structure::DelimiterRole::Open && fact.family().is_bracket()).count(), 3);
        assert_eq!(unmatched.delimiter_facts().filter(|fact| fact.role() == structure::DelimiterRole::Unmatched && fact.family().is_bracket()).count(), 2);
        assert_eq!(unmatched.delimiter_facts().filter(|fact| fact.role() == structure::DelimiterRole::Unmatched && fact.family().is_quote()).count(), 1);
    }

    #[test]
    fn parser_does_not_treat_lexical_or_numeric_apostrophes_as_quotes() {
        let contraction = parsed_number("don't", "en");
        let archaic_contraction = parsed_number("'Twas", "en");
        let swiss_number = parsed_number("1’234", "de-CH");

        assert_eq!(contraction.delimiter_facts().count(), 0);
        assert!(archaic_contraction.delimiter_facts().all(|fact| fact.role() == structure::DelimiterRole::Ignored));
        assert_eq!(swiss_number.delimiter_facts().count(), 0);
    }

    #[test]
    fn parser_emits_spanish_paired_sentence_punctuation() {
        let paired = parsed_number("¿Qué? ¡Bien!", "es-ES");
        let unpaired = parsed_number("Qué? Bien!", "es-ES");
        let english = parsed_number("What?", "en");

        assert_eq!(paired.delimiter_facts().filter(|fact| fact.family().is_language_punctuation() && fact.role() == structure::DelimiterRole::Open).count(), 2);
        assert_eq!(paired.delimiter_facts().filter(|fact| fact.family().is_language_punctuation() && fact.role() == structure::DelimiterRole::Unmatched).count(), 0);
        assert_eq!(unpaired.delimiter_facts().filter(|fact| fact.family().is_language_punctuation() && fact.role() == structure::DelimiterRole::Unmatched).count(), 2);
        assert_eq!(english.delimiter_facts().filter(|fact| fact.family().is_language_punctuation()).count(), 0);
    }

    #[test]
    fn parser_emits_french_non_breaking_spacing_facts() {
        let correct = parsed_number("«\u{00A0}Bonjour\u{202F}»\u{00A0}!", "fr-FR");
        let breaking = parsed_number("« Bonjour » !", "fr-FR");

        assert!(correct.punctuation_whitespace_facts().all(|fact| fact.assessment() != structure::WhitespaceTypographyAssessment::Foreign));
        assert_eq!(breaking.punctuation_whitespace_facts().filter(|fact| fact.assessment() == structure::WhitespaceTypographyAssessment::Foreign).count(), 3);
    }

    #[test]
    fn parser_emits_generic_latin_spacing_facts_without_reparsing_placeables() {
        let correct = parsed_number("Version 1.5, ready.", "en");
        let broken = parsed_number("Version 1.5 ,ready.", "en");

        assert!(correct.punctuation_whitespace_facts().all(|fact| fact.assessment() != structure::WhitespaceTypographyAssessment::Foreign));
        assert_eq!(broken.punctuation_whitespace_facts().filter(|fact| fact.assessment() == structure::WhitespaceTypographyAssessment::Foreign).count(), 2);
        assert!(correct.iter_nodes().find(|node| node.text() == "1.5").is_some_and(|node| node.kind() == NodeKind::Version));
    }

    #[test]
    fn parser_recognizes_both_ellipsis_spellings_as_atomic_punctuation() {
        ["Jag vet inte ...", "Jag vet inte \u{2026}"].into_iter().for_each(|text| {
            let tree = parsed_number(text, "sv-SE");
            let ellipses = tree.iter_nodes().filter(|node| node.kind() == NodeKind::Ellipsis).collect::<Vec<_>>();
            assert_eq!(ellipses.len(), 1, "{text}");
            assert!(matches!(ellipses[0].text(), "..." | "\u{2026}"), "{text}");
        });
    }

    #[test]
    fn swedish_ellipsis_allows_space_or_no_space_before_it() {
        ["Jag vet inte ...", "Vad i helv...", "Jag vet inte \u{2026}", "Vad i helv\u{2026}"].into_iter().for_each(|text| {
            assert!(parsed_number(text, "sv-SE").punctuation_whitespace_facts().all(|fact| fact.assessment() != structure::WhitespaceTypographyAssessment::Foreign), "{text}");
        });
        assert_eq!(parsed_number("Detta är fel .", "sv-SE").punctuation_whitespace_facts().filter(|fact| fact.assessment() == structure::WhitespaceTypographyAssessment::Foreign).count(), 1);
    }

    #[test]
    fn generic_spacing_policy_is_limited_to_english_german_and_swedish() {
        ["en-GB", "de-DE", "sv-SE"].into_iter().for_each(|language| assert!(parsed_number("Text ,text", language).punctuation_whitespace_facts().next().is_some(), "{language}"));
        assert_eq!(parsed_number("Testo ,testo", "it-IT").punctuation_whitespace_facts().count(), 0);
    }

    #[cfg(feature = "terminology")]
    #[test]
    fn target_rebuild_prefers_terms_and_falls_back_to_known_units() {
        let term_matcher = TermMatcher::new(&["hårddisk".to_string()]);
        let term_target = rebuilt_with_term_matcher_or_known_units(&crate::tokenized_text_tree("Använd hårddisk"), &crate::tokenized_text_tree("Use hard disk"), &term_matcher, ParseLanguage::Swedish);
        let mut measurement_source = crate::tokenized_text_tree("Wait 20 min");
        rebuild(&mut measurement_source, ParseLanguage::English);
        let measurement_target = rebuilt_with_term_matcher_or_known_units(&crate::tokenized_text_tree("Vänta 20 minuter"), &measurement_source, &TermMatcher::new(&[]), ParseLanguage::Swedish);

        assert!(term_target.has_term_units());
        assert!(measurement_target.preorder_iter().any(|node| node.kind() == NodeKind::Measurement));
    }

    #[cfg(feature = "terminology")]
    #[test]
    fn target_rebuild_applies_terms_and_known_units_together() {
        let mut source = crate::tokenized_text_tree("Use hard disk for 20 min");
        rebuild(&mut source, ParseLanguage::English);
        let target = rebuilt_with_term_matcher_or_known_units(&crate::tokenized_text_tree("Use hard disk for 20 minutes"), &source, &TermMatcher::new(&["hard disk".to_string()]), ParseLanguage::English);

        assert!(target.has_term_units());
        assert!(target.iter_nodes().find(|node| node.kind() == NodeKind::Measurement).is_some_and(|node| node.measurement_fact().is_some_and(|fact| fact.unit() == "min")));
    }

    #[test]
    fn target_parsing_resolves_source_grounded_expanded_compound_units() {
        for (source_text, source_language, target_text, target_language, canonical) in [
            ("5 km/h", ParseLanguage::English, "5 kilometer per timme", ParseLanguage::Swedish, "km/h"),
            ("5 l/min", ParseLanguage::English, "5 Liter pro Minute", ParseLanguage::German, "l/min"),
            ("5 W/m²", ParseLanguage::Swedish, "5 watts per square metre", ParseLanguage::English, "W/m²"),
            ("5 kWh", ParseLanguage::German, "5 kilowattimmar", ParseLanguage::Swedish, "kWh"),
        ] {
            let mut source = crate::tokenized_text_tree(source_text);
            parse(&mut source, source_language);
            let mut target = crate::tokenized_text_tree(target_text);
            parse_target(&mut target, target_language, &source);

            assert_eq!(target.iter_nodes().find_map(|node| node.measurement_fact().map(|fact| fact.unit())), Some(canonical), "{target_text}");
        }
    }

    #[test]
    fn target_only_currency_remains_classified_for_qa() {
        let mut source = crate::tokenized_text_tree("The duration is 15 seconds");
        parse(&mut source, ParseLanguage::English);
        let mut target = crate::tokenized_text_tree("Tiden är 15 SEK");
        parse_target(&mut target, ParseLanguage::Swedish, &source);

        assert_eq!(source.node_kind_count(NodeKind::Currency), 0);
        assert_eq!(target.node_kind_count(NodeKind::Currency), 1);
    }

    #[test]
    fn target_currency_is_allowed_when_source_contains_currency() {
        let mut source = crate::tokenized_text_tree("The price is 15 USD");
        parse(&mut source, ParseLanguage::English);
        let mut target = crate::tokenized_text_tree("Priset är 15 SEK");
        parse_target(&mut target, ParseLanguage::Swedish, &source);

        assert_eq!(source.node_kind_count(NodeKind::Currency), 1);
        assert_eq!(target.node_kind_count(NodeKind::Currency), 1);
        assert_eq!(target.iter_nodes().find_map(|node| node.currency_fact().and_then(|currency| currency.currency())), Some("SEK"));
    }

    #[test]
    fn ambiguous_currency_words_use_source_grounding_then_home_currency() {
        let mut source = crate::tokenized_text_tree("The price is 15 NOK");
        parse(&mut source, ParseLanguage::English);
        let mut german = crate::tokenized_text_tree("Der Preis beträgt 15 Kronen");
        parse_target(&mut german, ParseLanguage::German, &source);
        assert_eq!(german.iter_nodes().find_map(|node| node.currency_fact().and_then(|currency| currency.currency())), Some("NOK"));

        let mut swedish = crate::tokenized_text_tree("Priset är 15 kronor");
        parse(&mut swedish, ParseLanguage::Swedish);
        assert_eq!(swedish.iter_nodes().find_map(|node| node.currency_fact().and_then(|currency| currency.currency())), Some("SEK"));
    }

    #[test]
    fn unresolved_bare_currency_word_is_marked_ambiguous() {
        let mut tree = crate::tokenized_text_tree("Der Preis beträgt 15 Kronen");
        parse(&mut tree, ParseLanguage::German);
        let currency = tree.iter_nodes().find(|node| node.kind() == NodeKind::Currency).expect("ambiguous currency node");

        assert!(currency.currency_fact().is_some_and(|currency| currency.currency().is_none()));
    }

    #[test]
    fn qualified_currency_words_prefer_the_longest_match() {
        [("15 norwegian kroner", ParseLanguage::English, "NOK"), ("15 norska kronor", ParseLanguage::Swedish, "NOK"), ("15 dänische Kronen", ParseLanguage::German, "DKK")].into_iter().for_each(|(text, language, expected)| {
            let mut tree = crate::tokenized_text_tree(text);
            parse(&mut tree, language);
            assert_eq!(tree.iter_nodes().find_map(|node| node.currency_fact().and_then(|currency| currency.currency())), Some(expected), "{text}");
        });
    }

    #[test]
    fn target_number_word_is_grounded_by_the_same_source_integer() {
        let mut source = crate::tokenized_text_tree("There are 3 items");
        parse(&mut source, ParseLanguage::English);
        let mut target = crate::tokenized_text_tree("Det finns tre objekt");
        parse_target(&mut target, ParseLanguage::Swedish, &source);

        let number = target.iter_nodes().find(|node| node.kind() == NodeKind::Number).expect("source-grounded target number");
        assert_eq!(number.text(), "tre");
        assert_eq!(number.number_values(), &[30_000_000_000]);
    }

    #[test]
    fn target_number_word_remains_a_word_without_matching_source_integer() {
        let mut source = crate::tokenized_text_tree("There are 4 items");
        parse(&mut source, ParseLanguage::English);
        let mut target = crate::tokenized_text_tree("Det finns tre objekt");
        parse_target(&mut target, ParseLanguage::Swedish, &source);

        assert_eq!(target.node_kind_count(NodeKind::Number), 0);
        assert!(target.iter_nodes().any(|node| node.kind() == NodeKind::Word && node.text() == "tre"));
    }

    #[test]
    fn target_non_translatable_requires_an_exact_source_non_translatable() {
        let mut source = crate::tokenized_text_tree("ORAL HEALTH1");
        parse(&mut source, ParseLanguage::English);
        let mut target = crate::tokenized_text_tree("MUNHÄLSA1");
        parse_target(&mut target, ParseLanguage::Swedish, &source);

        assert!(source.protected_non_translatable_nodes().any(|node| node.text() == "HEALTH1"));
        assert_eq!(target.node_kind_count(NodeKind::NonTranslatable), 0);
        assert!(target.iter_nodes().any(|node| node.kind() == NodeKind::Word && node.text() == "MUNHÄLSA"));
        assert!(target.iter_nodes().any(|node| node.kind() == NodeKind::Number && node.text() == "1"));
    }

    #[cfg(feature = "dictionary")]
    #[test]
    fn dictionary_declassifies_known_word_before_reference_digit() {
        struct HealthDictionary;
        impl DictionaryLookup for HealthDictionary {
            fn is_dictionary_word(&self, word: &str) -> bool { word.eq_ignore_ascii_case("health") }
            fn dictionary_baseform(&self, _word: &str) -> Option<String> { None }
            fn has_dictionary(&self) -> bool { true }
        }
        let mut source = crate::tokenized_text_tree("ORAL HEALTH1");
        parse(&mut source, ParseLanguage::English);

        apply_dictionary(&mut source, &HealthDictionary);

        assert_eq!(source.node_kind_count(NodeKind::NonTranslatable), 0);
        assert!(source.iter_nodes().any(|node| node.kind() == NodeKind::Word && node.text() == "HEALTH" && node.dictionary_known()));
        assert!(source.iter_nodes().any(|node| node.kind() == NodeKind::Number && node.text() == "1"));
    }

    #[cfg(feature = "dictionary")]
    #[test]
    fn dictionary_prefix_recovery_requires_a_curated_prefix() {
        struct TailDictionary;
        impl DictionaryLookup for TailDictionary {
            fn is_dictionary_word(&self, word: &str) -> bool { matches!(word, "align" | "bekannt") }
            fn dictionary_baseform(&self, _word: &str) -> Option<String> { None }
            fn has_dictionary(&self) -> bool { true }
        }
        let mut tree = crate::tokenized_text_tree("invisalign unbekannt");
        parse(&mut tree, ParseLanguage::German);
        apply_dictionary(&mut tree, &TailDictionary);

        assert!(tree.iter_nodes().any(|node| node.text() == "invisalign" && node.dictionary_unknown()));
        assert!(tree.iter_nodes().any(|node| node.text() == "unbekannt" && node.dictionary_known()));
    }

    #[test]
    fn exact_source_non_translatable_grounds_the_target_and_stale_grounding_is_removed() {
        let mut matching_source = crate::tokenized_text_tree("Use MUNHÄLSA1");
        parse(&mut matching_source, ParseLanguage::English);
        let mut target = crate::tokenized_text_tree("MUNHÄLSA1");
        parse_target(&mut target, ParseLanguage::Swedish, &matching_source);
        assert!(target.protected_non_translatable_nodes().any(|node| node.text() == "MUNHÄLSA1"));

        let mut different_source = crate::tokenized_text_tree("ORAL HEALTH1");
        parse(&mut different_source, ParseLanguage::English);
        parse_target(&mut target, ParseLanguage::Swedish, &different_source);
        assert_eq!(target.node_kind_count(NodeKind::NonTranslatable), 0);
    }

    #[test]
    fn explicit_source_non_translatable_grounds_plain_target_text() {
        let source = crate::tokenized_runs_with_resolved_spans_tree([Run::new("Align")], &[(UnitKind::NonTranslatable, 0..5)]);
        let mut target = crate::tokenized_text_tree("Align");
        parse_target(&mut target, ParseLanguage::Swedish, &source);

        assert!(target.protected_non_translatable_nodes().any(|node| node.text() == "Align"));
    }

    #[test]
    fn ambiguous_swedish_articles_remain_words_when_source_contains_one() {
        let mut source = crate::tokenized_text_tree("Set the level to 1");
        parse(&mut source, ParseLanguage::English);
        let mut target = crate::tokenized_text_tree("Ställ in nivån i en meny med ett val");
        parse_target(&mut target, ParseLanguage::Swedish, &source);

        assert_eq!(target.node_kind_count(NodeKind::Number), 0);
        assert_eq!(target.iter_nodes().filter(|node| node.kind() == NodeKind::Word && matches!(node.text(), "en" | "ett")).count(), 2);
    }

    #[test]
    fn ambiguous_german_articles_remain_words_when_source_contains_one() {
        let mut source = crate::tokenized_text_tree("Set the level to 1");
        parse(&mut source, ParseLanguage::English);
        let mut target = crate::tokenized_text_tree("Stellen Sie die Stufe auf ein Menü mit einer Auswahl");
        parse_target(&mut target, ParseLanguage::German, &source);

        assert_eq!(target.node_kind_count(NodeKind::Number), 0);
        assert_eq!(target.iter_nodes().filter(|node| node.kind() == NodeKind::Word && matches!(node.text(), "ein" | "einer")).count(), 2);
    }

    #[test]
    fn reparsing_target_removes_stale_source_grounded_number_word() {
        let mut matching_source = crate::tokenized_text_tree("There are 3 items");
        parse(&mut matching_source, ParseLanguage::English);
        let mut target = crate::tokenized_text_tree("Det finns tre objekt");
        parse_target(&mut target, ParseLanguage::Swedish, &matching_source);
        assert_eq!(target.node_kind_count(NodeKind::Number), 1);

        let mut different_source = crate::tokenized_text_tree("There are 4 items");
        parse(&mut different_source, ParseLanguage::English);
        parse_target(&mut target, ParseLanguage::Swedish, &different_source);

        assert_eq!(target.node_kind_count(NodeKind::Number), 0);
        assert!(target.iter_nodes().any(|node| node.kind() == NodeKind::Word && node.text() == "tre"));
    }

    #[cfg(feature = "terminology")]
    #[test]
    fn target_rebuild_preserves_explicit_currency_without_source_currency() {
        let mut source = crate::tokenized_text_tree("The duration is 15 seconds");
        parse(&mut source, ParseLanguage::English);
        let mut target = crate::tokenized_text_tree("Tiden är 15 SEK");
        parse(&mut target, ParseLanguage::Swedish);
        assert_eq!(target.node_kind_count(NodeKind::Currency), 1);

        let target = rebuilt_with_term_matcher_or_known_units(&target, &source, &TermMatcher::new(&[]), ParseLanguage::Swedish);
        assert_eq!(target.node_kind_count(NodeKind::Currency), 1);
    }

    #[test]
    fn standalone_currency_parsing_remains_unrestricted() {
        let mut tree = crate::tokenized_text_tree("15 SEK");
        parse(&mut tree, ParseLanguage::Swedish);

        assert_eq!(tree.node_kind_count(NodeKind::Currency), 1);
    }

    #[cfg(feature = "terminology")]
    #[test]
    fn term_rebuild_clears_stale_overlay_annotations() {
        let annotated = rebuilt_with_term_matcher(&crate::tokenized_text_tree("Visit Yellowstone"), &TermMatcher::new(&["Yellowstone".to_string()]), ParseLanguage::English);
        let cleared = rebuilt_with_term_matcher_if_matched(&annotated, &TermMatcher::new(&[]), ParseLanguage::English).expect("stale annotation requires a rebuild");

        assert_eq!(cleared.node_kind_count(NodeKind::Term), 0);
        assert!(!cleared.has_term_units());
    }

    #[test]
    fn empty_splice_preserves_parse_output() {
        let mut tree = crate::tokenized_text_tree("5 USD");
        parse(&mut tree, ParseLanguage::English);
        let parsed = tree.clone();

        assert_eq!(replace_range_with_text(&mut tree, 0..0, "", ParseLanguage::English), 0);
        assert_eq!(tree, parsed);
    }

    #[test]
    fn deletion_reparses_even_when_zero_units_are_inserted() {
        let mut tree = crate::tokenized_text_tree("x 5 USD");
        parse(&mut tree, ParseLanguage::English);

        assert_eq!(replace_range_with_text(&mut tree, 0..2, "", ParseLanguage::English), 0);
        assert_eq!(tree.text(), "5 USD");
        assert_eq!(tree.node_kind_count(NodeKind::Currency), 1);
    }

    #[test]
    fn reparse_and_unrelated_edit_preserve_semantic_facts() {
        let mut tree = crate::tokenized_text_tree("x 5 USD and 20 min");
        parse(&mut tree, ParseLanguage::English);
        parse(&mut tree, ParseLanguage::English);

        assert!(tree.iter_nodes().find(|node| node.kind() == NodeKind::Currency).is_some_and(|node| node.currency_fact().is_some()));
        assert!(tree.iter_nodes().find(|node| node.kind() == NodeKind::Measurement).is_some_and(|node| node.measurement_fact().is_some()));

        assert_eq!(replace_range_with_text(&mut tree, 0..2, "", ParseLanguage::English), 0);
        assert!(tree.iter_nodes().find(|node| node.kind() == NodeKind::Currency).is_some_and(|node| node.currency_fact().is_some()));
        assert!(tree.iter_nodes().find(|node| node.kind() == NodeKind::Measurement).is_some_and(|node| node.measurement_fact().is_some()));
    }

    #[test]
    fn edit_reparse_preserves_only_explicit_term_occurrence() {
        let mut tree = crate::tokenized_text_tree("x foo foo");

        assert_eq!(replace_range_with_term(&mut tree, 2..3, "foo", ParseLanguage::English), 1);
        assert_eq!(tree.unit_kind_count(UnitKind::Term), 1);
        assert_eq!(replace_range_with_text(&mut tree, 0..2, "", ParseLanguage::English), 0);
        assert_eq!(tree.unit_kind_count(UnitKind::Term), 1);
    }

    #[cfg(feature = "terminology")]
    #[test]
    fn rebuilding_matcher_terms_preserves_an_explicit_term_inserted_into_empty_text() {
        let mut tree = crate::tokenized_text_tree("");

        assert_eq!(replace_range_with_term(&mut tree, 0..0, "approvedterm", ParseLanguage::English), 1);
        let rebuilt = rebuilt_with_term_hits(&tree, &[], ParseLanguage::English);

        assert_eq!(rebuilt.unit_kind_count(UnitKind::Term), 1);
        assert_eq!(rebuilt.text(), "approvedterm");
    }

    #[test]
    fn url_entity_does_not_consume_plain_word_after_whitespace() {
        let mut tree = crate::tokenized_text_tree("https://example.com/ next");
        parse(&mut tree, ParseLanguage::English);

        assert_eq!(url_facts(&tree).count(), 1);
        assert_eq!(url_facts(&tree).next().map(|fact| fact.text()), Some("https://example.com/"));
    }

}
