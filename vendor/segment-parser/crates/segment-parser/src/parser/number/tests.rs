use crate::parser::*;
use crate::parser::test_support::*;
use crate::ParseLanguage;
use structure::{Run, TagData, TagKind};

#[test]
fn fixed_point_values_do_not_saturate() {
    let number = |text| parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::English).into_iter().find(|node| node.kind() == NodeKind::Number).expect("number").number_values()[0];
    assert_ne!(number("10000000000"), number("99999999999"));
    assert_ne!(first_measurement("1 TB").measurement_fact().expect("measurement fact").values(), first_measurement("500 TB").measurement_fact().expect("measurement fact").values());
    assert_eq!(first_measurement("1 PB").measurement_fact().expect("measurement fact").values(), first_measurement("1000 TB").measurement_fact().expect("measurement fact").values());
    assert_ne!(first_measurement("1 nm").measurement_fact().expect("measurement fact").values()[0].amount(), 0);
    assert_eq!(to_fixed_point(f64::MAX), None);
}

#[test]
fn textual_dates_match_unicode_uppercase_months() {
    assert!(parse_nodes(&crate::tokenized_text_tree("15. MÄRZ 2024"), ParseLanguage::German).iter().any(|node| node.kind() == NodeKind::Date));
}

#[test]
fn numeric_dates_reject_mixed_and_repeated_separators() {
    ["01-02/2024", "1//2/2024"].into_iter().for_each(|text| assert!(!parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::English).iter().any(|node| node.kind() == NodeKind::Date), "{text}"));
}

#[test]
#[ignore = "parser profiling; run explicitly with --ignored --nocapture"]
fn profile_phrase_candidate_collection() {
    let samples = [
        "Dr. Alice paid € 12,50 on 2024-06-29, used https://example.com, and measured 10-20 mm.",
        "www.example.com mail@test.com ABC_DEF-123 600x400 px 5 USD 14:30",
        "The baby weighed 8 pounds, not 8 GBP, and the line ends with #fff.",
        "Plain text remains the fallback path with ordinary words and punctuation.",
    ];
    profile_parse_samples("phrase-candidates", &samples, |sample| Run::new(sample));
}

#[test]
fn numeric_phrase_parses_normalized_value_without_mutating_tree() {
    let tree = crate::tokenized_text_tree("1,5");
    let context = ParseContext::for_language(&tree, ParseLanguage::Neutral, &[]);
    let phrase = ParsedNumber::parse_range(&context, 0, structure::parser_api::units(&tree).len()).expect("numeric phrase");
    assert_eq!(phrase.value(), 15_000_000_000);
    assert_eq!(tree.text(), "1,5");
}

#[test]
fn standalone_tag_is_a_number_boundary_before_adjacent_word() {
    let mut run = Run::new("6.9");
    run.push_tag(TagData::new("tag".to_string(), "x".to_string(), TagKind::Standalone, "<x/>".to_string(), None, None));
    run.push_text("Not");
    let nodes = parse_nodes(&crate::tokenized_runs_tree([run]), ParseLanguage::English);
    let number = nodes.iter().find(|node| node.kind() == NodeKind::Number).expect("number before standalone tag");

    assert_eq!(number.span(), 0..3);
}

#[test]
fn capitalized_word_is_a_number_boundary_without_accepting_product_codes() {
    let prefixed = parse_nodes(&crate::tokenized_text_tree("6.9Not-Halt-Tester"), ParseLanguage::English);
    let product_code = parse_nodes(&crate::tokenized_text_tree("6.9ABC"), ParseLanguage::English);

    assert_eq!(prefixed.iter().find(|node| node.kind() == NodeKind::Number).map(Node::span), Some(0..3));
    assert!(!product_code.iter().any(|node| node.kind() == NodeKind::Number));
}

#[test]
fn grounded_currency_outranks_the_bare_number_candidate() {
    let mut tree = crate::tokenized_text_tree("5 USD");
    crate::parse(&mut tree, ParseLanguage::English);
    assert_eq!(crate::currency_facts(&tree).map(|fact| fact.text()).collect::<Vec<_>>(), vec!["5 USD"]);
    assert!(crate::number_facts(&tree).next().is_none());
}

#[test]
fn specific_email_candidate_outranks_generic_non_translatable() {
    let mut tree = crate::tokenized_text_tree("UserName@example.com");
    crate::parse(&mut tree, ParseLanguage::English);
    assert_eq!(crate::email_facts(&tree).map(|fact| fact.text()).collect::<Vec<_>>(), vec!["UserName@example.com"]);
    assert_eq!(tree.node_kind_count(NodeKind::NonTranslatable), 0);
}

#[test]
fn swedish_and_german_number_parsing_accepts_comma_and_dot_decimals() {
    for text in ["1.234,56", "1,234.56"] {
        let german_nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::German);
        let swedish_nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::Swedish);
        assert!(german_nodes.iter().any(|node| node.kind() == NodeKind::Number), "german did not parse {}", text);
        assert!(swedish_nodes.iter().any(|node| node.kind() == NodeKind::Number), "swedish did not parse {}", text);
    }
}

#[test]
fn number_semantics_record_observed_and_expected_locale_format() {
    let swedish_nodes = parse_nodes(&crate::tokenized_text_tree("1,234.50"), ParseLanguage::Swedish);
    let swedish = swedish_nodes.iter().find(|node| node.kind() == NodeKind::Number).and_then(Node::first_number_format).expect("swedish number format");
    assert_eq!(swedish.decimal_separator(), Some('.'));
    assert_eq!(swedish.grouping_separator(), Some(','));
    assert_eq!(swedish.locale_decimal_separator(), Some(','));
    assert_eq!(swedish.locale_grouping_separator(), Some('\u{00A0}'));
    assert_eq!(swedish.fraction_digits(), Some(2));
    assert_eq!(swedish.assessment(), NumberFormatAssessment::Foreign);

    let german_nodes = parse_nodes(&crate::tokenized_text_tree("1.234,50"), ParseLanguage::German);
    let german = german_nodes.iter().find(|node| node.kind() == NodeKind::Number).and_then(Node::first_number_format).expect("german number format");
    assert_eq!(german.grouping_separator(), Some('.'));
    assert_eq!(german.assessment(), NumberFormatAssessment::Canonical);
}

#[test]
fn swedish_narrow_non_breaking_grouping_is_accepted() {
    let nodes = parse_nodes(&crate::tokenized_text_tree("1\u{202F}234,50"), ParseLanguage::Swedish);
    let format = nodes.iter().find(|node| node.kind() == NodeKind::Number).and_then(Node::first_number_format).expect("number format");

    assert_eq!(format.grouping_separator(), Some('\u{202F}'));
    assert_eq!(format.assessment(), NumberFormatAssessment::Accepted);
}

#[test]
fn locale_grouping_separator_produces_integer_semantic_value() {
    for (text, language, locale_decimal) in [("1,234 USD", ParseLanguage::English, '.'), ("1.234 EUR", ParseLanguage::German, ',')] {
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), language);
        let currency = nodes.iter().find(|node| node.kind() == NodeKind::Currency).expect("currency");

        assert_eq!(currency.currency_fact().expect("currency fact").amount(), 12_340_000_000_000, "{text}");
        let number_text = text.split_whitespace().next().expect("number text");
        let number_nodes = parse_nodes(&crate::tokenized_text_tree(number_text), language);
        let number = number_nodes.iter().find(|node| node.kind() == NodeKind::Number).expect("number");
        assert_eq!(number.first_number_format().and_then(|format| format.decimal_separator()), None, "{text}");
        assert_eq!(number.first_number_format().and_then(|format| format.locale_decimal_separator()), Some(locale_decimal), "{text}");
    }
}

#[test]
fn grouped_number_space_variants_preserve_semantic_facts() {
    let baseline = semantic_signature(&parse_nodes(&crate::tokenized_text_tree("1\u{00a0}234,5 kg"), ParseLanguage::Swedish));
    for space in ['\u{00a0}', '\u{202f}'] {
        let text = format!("1{space}234,5 kg");
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::Swedish);
        assert_eq!(semantic_signature(&nodes), baseline);
    }
}

#[test]
fn swedish_and_german_numbers_with_hyphen_between_decimals() {
    let texts = ["2,0-2,5"];
    for text in texts {
        let german_nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::German);
        let swedish_nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::Swedish);
        let expected = [NodeKind::Number, NodeKind::Punct, NodeKind::Number];
        assert_eq!(german_nodes.iter().map(Node::kind).collect::<Vec<_>>(), expected);
        assert_eq!(swedish_nodes.iter().map(Node::kind).collect::<Vec<_>>(), expected);
    }
}

#[test]
fn dates_validate_real_calendar_days() {
    for (text, language) in [("2024-02-29", ParseLanguage::Swedish), ("31/12/2024", ParseLanguage::Swedish), ("15.01.2024", ParseLanguage::German)] {
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), language);
        assert!(nodes.iter().any(|node| node.kind() == NodeKind::Date), "expected date for {text}");
    }

    for text in ["2024-02-30", "2024/31/12"] {
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::Swedish);
        assert!(!nodes.iter().any(|node| node.kind() == NodeKind::Date), "unexpected date for {text}");
    }
}

#[test]
fn detects_textual_dates_by_language() {
    let cases: [(&str, ParseLanguage); 4] = [
        ("July 8, 2026", ParseLanguage::English),
        ("Jul. 8, 2026", ParseLanguage::English),
        ("8 juli 2026", ParseLanguage::Swedish),
        ("8. Juli 2026", ParseLanguage::German),
    ];
    for (text, parse_language) in cases {
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), parse_language);
        let date = nodes.iter().find(|node| node.kind() == NodeKind::Date).expect("date");
        assert_eq!(&text[date.span()], text);
    }
}

#[test]
fn rejects_invalid_textual_dates() {
    let cases: [(&str, ParseLanguage); 3] = [
        ("February 30, 2024", ParseLanguage::English),
        ("31 februari 2024", ParseLanguage::Swedish),
        ("31. April 2024", ParseLanguage::German),
    ];
    for (text, parse_language) in cases {
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), parse_language);
        assert!(!nodes.iter().any(|node| node.kind() == NodeKind::Date), "unexpected date for {text}");
    }
}

#[test]
fn swedish_parser_detects_numbers_without_english_abbreviations() {
    // "Mr." (unlike "Dr.", which Swedish also genuinely recognizes) has no Swedish usage —
    // titles like this largely dropped out of Swedish address since the 1960s "du-reform".
    let units = vec![
        u(UnitKind::Word, "Mr", 2),
        u(UnitKind::Punct, ".", 1),
        u(UnitKind::Whitespace, " ", 1),
        u(UnitKind::Numeric, "1", 1),
        u(UnitKind::Punct, ",", 1),
        u(UnitKind::Numeric, "5", 1),
    ];
    let nodes = parse_nodes(&stream(units, "Mr. 1,5"), ParseLanguage::Swedish);
    assert!(nodes.iter().any(|node| node.kind() == NodeKind::Number));
    assert!(!nodes.iter().any(|node| node.kind() == NodeKind::Abbreviation));
}

#[test]
fn swedish_parser_treats_leading_number_as_number() {
    let nodes = parse_nodes(&crate::tokenized_text_tree("1,5 grader"), ParseLanguage::Swedish);
    assert!(nodes.iter().any(|node| node.kind() == NodeKind::Number));
}

#[test]
fn swedish_parser_treats_narrow_and_no_break_space_numbers_as_number() {
    let parse_language = ParseLanguage::Swedish;
    for text in ["1\u{00A0}234", "1\u{202F}234", "1\u{00A0}234,56"] {
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), parse_language);
        let number = nodes
            .iter()
            .find(|node| node.kind() == NodeKind::Number)
            .expect("number");
        assert_eq!(&text[number.span()], text);
    }
}

#[test]
fn parser_detects_time_entities() {
    for text in ["14:30", "14:30:15", "2 p.m.", "9 AM", "5:30 PM", "5:30:15 p.m."] {
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::Neutral);
        let time = nodes.iter().find(|node| node.kind() == NodeKind::Time).expect("time");
        assert_eq!(&text[time.span()], text);
    }

    let text = "5 p.m.,";
    let nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::Neutral);
    let time = nodes.iter().find(|node| node.kind() == NodeKind::Time).expect("time");
    assert_eq!(&text[time.span()], "5 p.m.");
    assert_eq!(time.time_fact(), Some(TimeFact::new(17, 0, None)));

    let nodes = parse_nodes(&crate::tokenized_text_tree("14:30:15"), ParseLanguage::Neutral);
    let time = nodes.iter().find(|node| node.kind() == NodeKind::Time).expect("time");
    assert_eq!(time.time_fact(), Some(TimeFact::new(14, 30, Some(15))));

    let nodes = parse_nodes(&crate::tokenized_text_tree("5:30:15 p.m."), ParseLanguage::Neutral);
    let time = nodes.iter().find(|node| node.kind() == NodeKind::Time).expect("meridiem time");
    assert_eq!(time.time_fact(), Some(TimeFact::new(17, 30, Some(15))));
}

#[test]
fn parser_detects_time_entities_with_locale_specific_separators_and_suffixes() {
    let text = "14.30 Uhr";
    let mut tree = crate::tokenized_text_tree(text);
    crate::parse(&mut tree, "de-DE");
    let time = tree.iter_nodes().find(|node| node.kind() == NodeKind::Time).expect("de time");
    assert_eq!(&text[time.span()], text);
    assert_eq!(time.time_fact(), Some(TimeFact::new(14, 30, None)));

    let text = "14.30h";
    let mut tree = crate::tokenized_text_tree(text);
    crate::parse(&mut tree, "de-CH");
    let time = tree.iter_nodes().find(|node| node.kind() == NodeKind::Time).expect("ch time");
    assert_eq!(&text[time.span()], text);
    assert_eq!(time.time_fact(), Some(TimeFact::new(14, 30, None)));
}

#[test]
fn meridiem_detection_uses_language_profile() {
    let english_nodes = parse_nodes(&crate::tokenized_text_tree("9 AM"), ParseLanguage::English);
    assert!(english_nodes.iter().any(|node| node.kind() == NodeKind::Time));

    let german_nodes = parse_nodes(&crate::tokenized_text_tree("9 AM"), ParseLanguage::German);
    assert!(!german_nodes.iter().any(|node| node.kind() == NodeKind::Time));
}

#[test]
fn parser_rejects_invalid_time_entities() {
    for text in ["24:30", "14:99", "14:30:99", "13 p.m.", "14:30abc", "14:30:15:99", "x14:30", "14.30", "14.30h"] {
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::Neutral);
        assert!(!nodes.iter().any(|node| node.kind() == NodeKind::Time), "text {text}");
    }

    let mut tree = crate::tokenized_text_tree("14.30 Uhr");
    crate::parse(&mut tree, "sv-SE");
    assert!(!tree.iter_nodes().any(|node| node.kind() == NodeKind::Time));

    let mut tree = crate::tokenized_text_tree("14.30h");
    crate::parse(&mut tree, "de-DE");
    assert!(!tree.iter_nodes().any(|node| node.kind() == NodeKind::Time), "de h suffix");
}

#[test]
fn language_parser_factory_uses_swedish_for_swedish_tags() {
    let units = vec![
        u(UnitKind::Word, "Mr", 2),
        u(UnitKind::Punct, ".", 1),
        u(UnitKind::Whitespace, " ", 1),
        u(UnitKind::Numeric, "1", 1),
        u(UnitKind::Punct, ",", 1),
        u(UnitKind::Numeric, "5", 1),
    ];
    let nodes = parse_nodes(&stream(units, "Mr. 1,5"), ParseLanguage::Swedish);
    assert!(nodes.iter().any(|node| node.kind() == NodeKind::Number));
    assert!(!nodes.iter().any(|node| node.kind() == NodeKind::Abbreviation));
}

#[test]
fn placeholders_are_structural_facts_without_internal_number_facts() {
    let mut tree = crate::tokenized_text_tree("Use {count}, ${user.name}, %02d and %1.");
    crate::parse(&mut tree, ParseLanguage::English);
    let placeholders = tree.preorder_iter().filter_map(|node| node.placeholder_semantic()).collect::<Vec<_>>();
    assert_eq!(placeholders.iter().map(|placeholder| placeholder.canonical()).collect::<Vec<_>>(), vec!["{count}", "${user.name}", "%02d", "%1"]);
    assert!(crate::number_facts(&tree).next().is_none());
}
