use crate::parser::*;
use crate::parser::test_support::*;
use crate::ParseLanguage;
use structure::Run;

#[test]
#[ignore = "parser profiling; run explicitly with --ignored --nocapture"]
fn profile_parse_punctuation_heavy() {
    let samples = [
        "A/B-C_D.E:F;G,H(I)[J]{K}<L>|M\\N+O=P*Q&R%S#T@U!",
        "123,456.789-00/11:22;33%44+55=66 and ABC_DEF-123.",
        "https://example.com/path?q=1&x=2 mail@test.example.com",
        "((alpha)) -- [[beta]] :: {{gamma}} && delta != epsilon;",
    ];
    profile_parse_samples("punctuation-heavy", &samples, |sample| Run::new(sample));
}

#[test]
fn alternating_current_voltage_requires_voltage_context() {
    let normalized = |text: &str| {
        parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::English)
            .into_iter()
            .find_map(|node| node.measurement_fact().and_then(|fact| fact.values().first().copied()))
            .unwrap_or_else(|| panic!("normalized measurement for {text:?}"))
    };

    assert_eq!(normalized("230 VAC"), normalized("230 V AC"));
    assert_eq!(normalized("230 V\u{00A0}AC"), normalized("230 VAC"));
    assert_eq!(normalized("230 VAC").quantity_kind(), MeasurementQuantityKind::AlternatingCurrentVoltage);
    assert_ne!(normalized("230 VAC"), normalized("230V"));
    assert!(!parse_nodes(&crate::tokenized_text_tree("230 AC"), ParseLanguage::English).iter().any(|node| node.kind() == NodeKind::Measurement));
}

#[test]
fn quote_interior_whitespace_is_locale_assessed_for_english_swedish_and_german() {
    [ParseLanguage::English, ParseLanguage::Swedish, ParseLanguage::German].into_iter().for_each(|language| {
        let mut spaced = crate::tokenized_text_tree("\" text \"");
        crate::parse(&mut spaced, language);
        let spaced_facts = spaced.quote_interior_whitespace_facts().collect::<Vec<_>>();
        assert_eq!(spaced_facts.len(), 2);
        assert!(spaced_facts.iter().all(|fact| fact.preferred() == Some(WhitespaceTypographyKind::Absent) && fact.assessment() == structure::WhitespaceTypographyAssessment::Foreign));

        let mut compact = crate::tokenized_text_tree("\"text\"");
        crate::parse(&mut compact, language);
        let compact_facts = compact.quote_interior_whitespace_facts().collect::<Vec<_>>();
        assert_eq!(compact_facts.len(), 2);
        assert!(compact_facts.iter().all(|fact| fact.observed() == WhitespaceTypographyKind::Absent && fact.assessment() == structure::WhitespaceTypographyAssessment::Canonical));
    });
}

#[test]
fn bracket_interior_whitespace_is_distinct_and_locale_assessed() {
    [ParseLanguage::English, ParseLanguage::Swedish, ParseLanguage::German].into_iter().for_each(|language| {
        let mut spaced = crate::tokenized_text_tree("( text ) [ text ] { text }");
        crate::parse(&mut spaced, language);
        let bracket_facts = spaced.bracket_interior_whitespace_facts().collect::<Vec<_>>();
        assert_eq!(bracket_facts.len(), 4);
        assert!(bracket_facts.iter().all(|fact| fact.preferred() == Some(WhitespaceTypographyKind::Absent) && fact.assessment() == structure::WhitespaceTypographyAssessment::Foreign));
        assert_eq!(spaced.quote_interior_whitespace_facts().count(), 0);

        let mut compact = crate::tokenized_text_tree("(text) [text]");
        crate::parse(&mut compact, language);
        let compact_facts = compact.bracket_interior_whitespace_facts().collect::<Vec<_>>();
        assert_eq!(compact_facts.len(), 4);
        assert!(compact_facts.iter().all(|fact| fact.observed() == WhitespaceTypographyKind::Absent && fact.assessment() == structure::WhitespaceTypographyAssessment::Canonical));
    });
}

#[test]
fn measurements_respect_generic_wrapping_contexts() {
    let wrappers = [("(", ")"), ("[", "]"), ("{", "}"), ("<", ">"), ("“", "”"), ("‘", "’"), ("«", "»"), ("「", "」"), ("‹", "›"), ("'", "'"), ("\"", "\"")];
    for (open, close) in wrappers {
        let wrapped_measurement = format!("{open}50 kg{close}");
        let nodes = parse_nodes(&crate::tokenized_text_tree(&wrapped_measurement), ParseLanguage::Neutral);
        assert!(nodes.iter().any(|node| node.kind() == NodeKind::Measurement), "failed measurement in wrapper {wrapped_measurement}");

        let wrapped_direct_measurement = format!("{open}20°C{close}");
        let nodes = parse_nodes(&crate::tokenized_text_tree(&wrapped_direct_measurement), ParseLanguage::Neutral);
        assert!(nodes.iter().any(|node| node.kind() == NodeKind::Measurement), "failed direct measurement in wrapper {wrapped_direct_measurement}");

        let wrapped_with_ambiguous_context = format!("{open}50 m{close}");
        let nodes = parse_nodes(&crate::tokenized_text_tree(&wrapped_with_ambiguous_context), ParseLanguage::Neutral);
        assert!(
            nodes.iter().all(|node| node.kind() != NodeKind::Measurement),
            "failed to avoid ambiguous wrapped space unit in {wrapped_with_ambiguous_context}"
        );
        assert_eq!(
            nodes.iter().filter(|node| node.kind() == NodeKind::Number).count(),
            1,
            "failed number-only parse in {wrapped_with_ambiguous_context}"
        );

        let wrapped_number = format!("{open}50{close}");
        let nodes = parse_nodes(&crate::tokenized_text_tree(&wrapped_number), ParseLanguage::Neutral);
        assert!(nodes.iter().all(|node| node.kind() != NodeKind::Measurement));
        if open == "{" {
            assert!(nodes.iter().any(|node| node.kind() == NodeKind::Placeholder));
        } else {
            assert!(nodes.iter().any(|node| node.kind() == NodeKind::Number));
        }

        let wrapped_number_trailing = format!("{open}4{close}.");
        let nodes = parse_nodes(&crate::tokenized_text_tree(&wrapped_number_trailing), ParseLanguage::Neutral);
        if open == "{" {
            assert!(nodes.iter().any(|node| node.kind() == NodeKind::Placeholder));
        } else {
            assert!(nodes.iter().any(|node| node.kind() == NodeKind::Number));
        }
    }
}

#[test]
fn parses_number_with_wrapped_trailing_close_punctuation() {
    let nodes = parse_nodes(&crate::tokenized_text_tree("(3.)"), ParseLanguage::Neutral);
    assert_eq!(nodes.iter().filter(|node| node.kind() == NodeKind::Number).count(), 1);
}

#[test]
fn only_matching_nested_contexts_are_entered() {
    let nodes = parse_nodes(&crate::tokenized_text_tree("(5 (50) ."), ParseLanguage::Neutral);
    assert_eq!(nodes.iter().filter(|node| node.kind() == NodeKind::Number).count(), 2);
}

#[test]
fn bracketed_phrases_do_not_claim_the_closing_context_unit() {
    for text in ["(see www.example.com)", "(ref ABC_123)x"] {
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::English);
        assert_non_overlapping_text_nodes(&nodes);
        assert_eq!(&text[nodes.last().expect("closing node").span()], if text.ends_with('x') { "x" } else { ")" });
    }
}

#[test]
fn parser_invariants_hold_for_representative_segments() {
    for (text, language) in [
        ("12 kg, 1,234 USD, and 2026-07-12", ParseLanguage::English),
        ("(see www.example.com) 1 234,5 kg", ParseLanguage::Swedish),
        ("50 x 20 mm at 13:45", ParseLanguage::German),
        ("ref ABC_123 and 192.168.1.1", ParseLanguage::Neutral),
    ] {
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), language);
        assert_non_overlapping_text_nodes(&nodes);
        for node in &nodes {
            if node.kind() == NodeKind::Measurement {
                assert!(node.measurement_fact().is_some_and(|fact| !fact.unit().is_empty()));
            }
            if node.kind() == NodeKind::Currency {
                assert!(node.currency_fact().is_some());
            }
        }
    }
}

#[test]
fn number_spans_exclude_trailing_sentence_punctuation() {
    for text in ["Costs 123.", "123. Next sentence"] {
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::Neutral);
        let number = nodes.iter().find(|node| node.kind() == NodeKind::Number).expect("number");
        assert_eq!(&text[number.span()], "123");
    }

    let text = "1.234,56.";
    let nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::Swedish);
    let number = nodes.iter().find(|node| node.kind() == NodeKind::Number).expect("number");
    assert_eq!(&text[number.span()], "1.234,56");

    let nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::German);
    let number = nodes.iter().find(|node| node.kind() == NodeKind::Number).expect("number");
    assert_eq!(&text[number.span()], "1.234,56");
}

#[test]
fn keeps_sentence_punctuation_outside_abbreviated_compound() {
    let text = "Typen-Nr.?";
    let nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::German);
    assert_eq!(nodes.iter().map(|node| (node.kind(), &text[node.span()])).collect::<Vec<_>>(), vec![(NodeKind::CompoundWord, "Typen-Nr."), (NodeKind::Punct, "?")]);
}

#[test]
fn unknown_compound_suffix_keeps_period_as_punctuation() {
    let text = "Typen-Xyz.";
    let nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::German);
    assert_eq!(nodes.iter().map(|node| (node.kind(), &text[node.span()])).collect::<Vec<_>>(), vec![(NodeKind::CompoundWord, "Typen-Xyz"), (NodeKind::Punct, ".")]);
}

#[test]
fn punctuation_as_number_boundary() {
    let nodes = parse_nodes(&crate::tokenized_text_tree("10:30 1+2-3 1,5"), ParseLanguage::Neutral);
    let kinds: Vec<_> = nodes.iter().map(Node::kind).collect();
    assert_eq!(kinds, vec![NodeKind::Time, NodeKind::Whitespace, NodeKind::Number, NodeKind::Punct, NodeKind::Number, NodeKind::Punct, NodeKind::Number, NodeKind::Whitespace, NodeKind::Number]);
}
