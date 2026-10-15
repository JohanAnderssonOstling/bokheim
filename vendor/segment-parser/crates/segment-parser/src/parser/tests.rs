use super::*;
use crate::ParseLanguage;
use structure::{Run, TagData, TagKind};

use crate::parser::test_support::*;

#[test]
#[ignore = "parser profiling; run explicitly with --ignored --nocapture"]
fn profile_parse_ascii_no_tags() {
    let samples = [
        "Plain sentence text with repeated words, punctuation, and no obvious entities.",
        "Another regular source segment that should resemble translation memory content.",
        "Short UI label text with words and spaces.",
        "A longer paragraph-like segment has commas, periods, and repeated common words for scanning.",
    ];
    profile_parse_samples("ascii-no-tags", &samples, |sample| Run::new(sample));
}

#[test]
#[ignore = "parser profiling; run explicitly with --ignored --nocapture"]
fn profile_parse_ascii_with_tags() {
    let samples = [
        "Hello world from tagged content.",
        "Keep bold words and placeholders in order.",
        "Segment with two inline tags around text.",
        "A longer tagged sentence with punctuation, numbers 123, and words.",
    ];
    profile_parse_samples("ascii-with-tags", &samples, tagged_run);
}

#[test]
#[ignore = "parser profiling; run explicitly with --ignored --nocapture"]
fn profile_parse_mixed_unicode() {
    let samples = [
        "Översätt följande mening med åäö och några svenska ord.",
        "Résumé envoyé à François avec naïve façade and coöperate.",
        "Mått: 10,5 kg, längd 42 cm, temperatur −5 °C.",
        "日本語 mixed with svenska åäö and English words.",
    ];
    profile_parse_samples("mixed-unicode", &samples, |sample| Run::new(sample));
}

#[test]
#[ignore = "parser profiling; run explicitly with --ignored --nocapture"]
fn profile_parse_mixed_existing() {
    let samples = [
        "Dr. Smith bought 1.234,56 kg of apples on 2024-06-29 and sent mail@test.com.",
        "Visit https://example.com/path?q=1 and compare 10,5 kg with 10% before 2026-07-01.",
        "The C-Med Alpha IFU_UserManualV23 contains ABC_123 and nontranslatable identifiers.",
        "Plain sentence text with repeated words, punctuation, and no obvious entities.",
    ];
    profile_parse_samples("mixed-existing", &samples, |sample| Run::new(sample));
}

#[test]
fn detects_basic_conservative_entities() {
    let units = vec![
        u(UnitKind::Word, "Visit", 5),
        u(UnitKind::Whitespace, " ", 1),
        u(UnitKind::Word, "https", 5),
        u(UnitKind::Punct, ":", 1),
        u(UnitKind::Punct, "/", 1),
        u(UnitKind::Punct, "/", 1),
        u(UnitKind::Word, "example", 7),
        u(UnitKind::Punct, ".", 1),
        u(UnitKind::Word, "com", 3),
    ];
    let nodes = parse_nodes(&stream(units, "Visit https://example.com"), ParseLanguage::Neutral);
    assert!(nodes.iter().any(|node| node.kind() == NodeKind::Url));
}

#[test]
fn detects_email_and_url_only() {
    let units = vec![
        u(UnitKind::Word, "Dr", 2),
        u(UnitKind::Punct, ".", 1),
        u(UnitKind::Whitespace, " ", 1),
        u(UnitKind::Numeric, "2024", 4),
        u(UnitKind::Punct, "-", 1),
        u(UnitKind::Numeric, "06", 2),
        u(UnitKind::Punct, "-", 1),
        u(UnitKind::Numeric, "29", 2),
        u(UnitKind::Whitespace, " ", 1),
        u(UnitKind::Word, "visit", 5),
        u(UnitKind::Whitespace, " ", 1),
        u(UnitKind::Word, "https", 5),
        u(UnitKind::Punct, ":", 1),
        u(UnitKind::Punct, "/", 1),
        u(UnitKind::Punct, "/", 1),
        u(UnitKind::Word, "example", 7),
        u(UnitKind::Punct, ".", 1),
        u(UnitKind::Word, "com", 3),
        u(UnitKind::Whitespace, " ", 1),
        u(UnitKind::Word, "and", 3),
        u(UnitKind::Whitespace, " ", 1),
        u(UnitKind::Word, "mail", 4),
        u(UnitKind::Punct, "@", 1),
        u(UnitKind::Word, "test", 4),
        u(UnitKind::Punct, ".", 1),
        u(UnitKind::Word, "com", 3),
    ];
    let nodes = parse_nodes(&stream(units, "Dr. 2024-06-29 visit https://example.com and mail@test.com"), ParseLanguage::English);
    assert!(nodes.iter().any(|node| node.kind() == NodeKind::Url));
    assert!(nodes.iter().any(|node| node.kind() == NodeKind::Email));
    assert!(nodes.iter().any(|node| node.kind() == NodeKind::Abbreviation));
    assert!(nodes.iter().any(|node| node.kind() == NodeKind::Date));
    for text in ["a..b@example.com", "a.@example.com"] {
        assert!(!parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::English).iter().any(|node| node.kind() == NodeKind::Email && &text[node.span()] == text));
    }
}

#[test]
fn email_and_url_stop_after_a_recognized_top_level_domain() {
    for (text, kind, expected) in [
        ("hello@skyshowtimeinsiders.com.Thank", NodeKind::Email, "hello@skyshowtimeinsiders.com"),
        ("https://example.com.Next", NodeKind::Url, "https://example.com"),
        ("https://example.technology.Next", NodeKind::Url, "https://example.technology"),
        ("team@example.international.Tack", NodeKind::Email, "team@example.international"),
        ("https://example.com/path.to/page?part=1", NodeKind::Url, "https://example.com/path.to/page?part=1"),
    ] {
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::English);
        let node = nodes.iter().find(|node| node.kind() == kind).unwrap_or_else(|| panic!("web entity for {text}; nodes={nodes:?}"));
        assert_eq!(&text[node.span()], expected);
    }
}

#[test]
fn web_phrase_resolves_through_phrase_boundary() {
    let tree = crate::tokenized_text_tree("https://example.com");
    let context = ParseContext::for_language(&tree, ParseLanguage::English, &[]);
    let phrase = parse_url_phrase_at(&context, 0).expect("url phrase");
    let detected = phrase.resolve();
    assert_eq!(detected.units_end(), structure::parser_api::units(&tree).len());
}

#[test]
fn phrase_scoring_prefers_protected_url_over_word() {
    for text in ["www.example.com", "HTTPS://example.com", "https://example.com/path_(one)"] {
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::English);
        let first = nodes.first().expect("node");
        assert_eq!(first.kind(), NodeKind::Url);
        assert_eq!(&text[first.span()], text);
    }
}

#[test]
fn german_detects_space_separated_abbreviations() {
    for text in ["z. B. das hier", "d. h. genau so"] {
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::German);
        assert!(
            nodes.iter().any(|node| node.kind() == NodeKind::Abbreviation),
            "expected an abbreviation node for {text}"
        );
    }
}

#[test]
fn detects_multi_space_multi_part_abbreviations() {
    let nodes = parse_nodes(&crate::tokenized_text_tree("fr. o. m. idag"), ParseLanguage::Swedish);
    let abbreviation = nodes.iter().find(|node| node.kind() == NodeKind::Abbreviation).expect("swedish abbreviation");
    assert_eq!(&"fr. o. m. idag"[abbreviation.span()], "fr. o. m.");

    let nodes = parse_nodes(&crate::tokenized_text_tree("z. B. hier"), ParseLanguage::German);
    let abbreviation = nodes.iter().find(|node| node.kind() == NodeKind::Abbreviation).expect("german abbreviation");
    assert_eq!(&"z. B. hier"[abbreviation.span()], "z. B.");

    let text = "t. ex. x";
    let nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::Swedish);
    assert!(!nodes.iter().any(|node| node.kind() == NodeKind::Abbreviation && &text[node.span()] == text));
}

#[test]
fn detects_hyphenated_compound_words_with_children() {
    let units = vec![
        u(UnitKind::Word, "award", 5),
        u(UnitKind::Punct, "-", 1),
        u(UnitKind::Word, "winning", 7),
        u(UnitKind::Whitespace, " ", 1),
        u(UnitKind::Word, "show", 4),
    ];
    let nodes = parse_nodes(&stream(units, "award-winning show"), ParseLanguage::Neutral);
    let compound = nodes.iter().find(|node| node.kind() == NodeKind::CompoundWord).expect("compound word");
    assert_eq!(&"award-winning show"[compound.span()], "award-winning");
    assert_eq!(compound.iter_children().map(Node::kind).collect::<Vec<_>>(), vec![NodeKind::Word, NodeKind::Punct, NodeKind::Word]);
}

#[test]
fn detects_abbreviation_components_inside_hyphenated_compounds() {
    let text = "Typen-Nr.";
    let nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::German);
    let compound = nodes.iter().find(|node| node.kind() == NodeKind::CompoundWord).expect("compound word");
    assert_eq!(&text[compound.span()], "Typen-Nr.");
    assert_eq!(compound.iter_children().map(Node::kind).collect::<Vec<_>>(), vec![NodeKind::Word, NodeKind::Punct, NodeKind::Abbreviation]);
    assert!(!nodes.iter().any(|node| node.kind() == NodeKind::Punct));
}

#[test]
fn detects_multiple_abbreviation_components_inside_compound() {
    let text = "Art.-Nr.";
    let nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::German);
    let compound = nodes.iter().find(|node| node.kind() == NodeKind::CompoundWord).expect("compound word");
    assert_eq!(&text[compound.span()], text);
    assert_eq!(compound.iter_children().map(Node::kind).collect::<Vec<_>>(), vec![NodeKind::Abbreviation, NodeKind::Punct, NodeKind::Abbreviation]);
}

#[test]
fn detects_english_abbreviation() {
    let units = vec![
        u(UnitKind::Word, "Dr", 2),
        u(UnitKind::Punct, ".", 1),
        u(UnitKind::Whitespace, " ", 1),
        u(UnitKind::Word, "Smith", 5),
    ];
    let nodes = parse_nodes(&stream(units, "Dr. Smith"), ParseLanguage::English);
    assert!(nodes.iter().any(|node| node.kind() == NodeKind::Abbreviation));
}

#[test]
fn classified_case_insensitive_abbreviations_accept_case_variants() {
    for (text, parse_language) in [
        ("ABBR. notes", ParseLanguage::English),
        ("ADM. office", ParseLanguage::English),
        ("CERT. issued", ParseLanguage::English),
        ("BSPW. hier", ParseLanguage::German),
        ("ANGEST. Person", ParseLanguage::German),
        ("ÖFFENTL. Stelle", ParseLanguage::German),
        ("WAHRSCH. richtig", ParseLanguage::German),
        ("P.G.A. detta", ParseLanguage::Swedish),
    ] {
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), parse_language);
        assert!(nodes.iter().any(|node| node.kind() == NodeKind::Abbreviation), "expected classified abbreviation for {text}");
    }
}

#[test]
fn dictionary_word_collisions_require_continuation_evidence() {
    for (text, parse_language) in [
        ("This is art. Next sentence.", ParseLanguage::English),
        ("Das ist eine Art. Danach geht es weiter.", ParseLanguage::German),
        ("Det är en bil. Sedan går vi.", ParseLanguage::Swedish),
    ] {
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), parse_language);
        assert!(!nodes.iter().any(|node| node.kind() == NodeKind::Abbreviation), "ordinary word was classified as an abbreviation in {text}");
    }

    for (text, parse_language) in [
        ("See art. 3", ParseLanguage::English),
        ("die sog. technische Lösung", ParseLanguage::German),
        ("se bil. A", ParseLanguage::Swedish),
    ] {
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), parse_language);
        assert!(nodes.iter().any(|node| node.kind() == NodeKind::Abbreviation), "continuation evidence was ignored in {text}");
    }
}

#[test]
fn recommended_ambiguous_abbreviations_use_general_continuation_evidence() {
    for (abbreviation, parse_language) in [
        ("ans.", ParseLanguage::English),
        ("arr.", ParseLanguage::English),
        ("def.", ParseLanguage::English),
        ("dep.", ParseLanguage::English),
        ("diff.", ParseLanguage::English),
        ("dist.", ParseLanguage::English),
        ("doc.", ParseLanguage::English),
        ("ed.", ParseLanguage::English),
        ("ext.", ParseLanguage::English),
        ("par.", ParseLanguage::English),
        ("pp.", ParseLanguage::English),
        ("prev.", ParseLanguage::English),
        ("quot.", ParseLanguage::English),
        ("rel.", ParseLanguage::English),
        ("seq.", ParseLanguage::English),
        ("sub.", ParseLanguage::English),
        ("div.", ParseLanguage::German),
        ("geh.", ParseLanguage::German),
        ("log.", ParseLanguage::German),
        ("reg.", ParseLanguage::German),
        ("bl.", ParseLanguage::Swedish),
        ("lat.", ParseLanguage::Swedish),
        ("long.", ParseLanguage::Swedish),
        ("sp.", ParseLanguage::Swedish),
    ] {
        let with_reference = format!("{abbreviation} 2");
        let possible_sentence = format!("{abbreviation} Example");
        assert!(parse_nodes(&crate::tokenized_text_tree(&with_reference), parse_language).iter().any(|node| node.kind() == NodeKind::Abbreviation), "reference evidence was ignored for {with_reference}");
        assert!(!parse_nodes(&crate::tokenized_text_tree(&possible_sentence), parse_language).iter().any(|node| node.kind() == NodeKind::Abbreviation), "uppercase sentence was hidden after {abbreviation}");
    }

    assert!(parse_nodes(&crate::tokenized_text_tree("So. 10:00"), ParseLanguage::German).iter().any(|node| node.kind() == NodeKind::Abbreviation));
    assert!(!parse_nodes(&crate::tokenized_text_tree("so. 10:00"), ParseLanguage::German).iter().any(|node| node.kind() == NodeKind::Abbreviation));
}

#[test]
fn narrow_abbreviation_evidence_supports_names_addresses_references_and_terminals() {
    for (text, parse_language) in [
        ("Gen. Smith", ParseLanguage::English),
        ("Madison Ave.", ParseLanguage::English),
        ("Soc. America", ParseLanguage::English),
        ("respectively resp.", ParseLanguage::English),
        ("19. Jh.", ParseLanguage::German),
    ] {
        assert!(parse_nodes(&crate::tokenized_text_tree(text), parse_language).iter().any(|node| node.kind() == NodeKind::Abbreviation), "narrow evidence was ignored in {text}");
    }

    for (text, parse_language) in [
        ("Gen. details", ParseLanguage::English),
        ("an Ave.", ParseLanguage::English),
        ("soc. America", ParseLanguage::English),
        ("Jh. 19", ParseLanguage::German),
    ] {
        assert!(!parse_nodes(&crate::tokenized_text_tree(text), parse_language).iter().any(|node| node.kind() == NodeKind::Abbreviation), "insufficient evidence was accepted in {text}");
    }
}

#[test]
fn classified_exact_case_abbreviations_preserve_ambiguous_lowercase_words() {
    for (recognized, rejected, parse_language) in [
        ("Mx. Smith", "mx. Smith", ParseLanguage::English),
        ("Miss. Smith", "miss. Smith", ParseLanguage::English),
        ("Anm. A", "anm. A", ParseLanguage::German),
        ("Abf. A", "abf. A", ParseLanguage::German),
        ("Ausbild. A", "ausbild. A", ParseLanguage::German),
        ("Apr. 2026", "apr. 2026", ParseLanguage::German),
        ("Reg. 1", "reg. 1", ParseLanguage::Swedish),
    ] {
        let recognized = parse_nodes(&crate::tokenized_text_tree(recognized), parse_language);
        let rejected = parse_nodes(&crate::tokenized_text_tree(rejected), parse_language);
        assert!(recognized.iter().any(|node| node.kind() == NodeKind::Abbreviation));
        assert!(!rejected.iter().any(|node| node.kind() == NodeKind::Abbreviation));
    }
}

#[test]
fn trados_parity_groups_cover_low_collision_english_and_german_entries() {
    for (text, parse_language) in [
        ("Fahr. 32", ParseLanguage::English),
        ("pcs. 10", ParseLanguage::English),
        ("Geogr. Daten", ParseLanguage::German),
        ("veröff. Text", ParseLanguage::German),
        ("Stromkreisnr. 7", ParseLanguage::German),
    ] {
        assert!(parse_nodes(&crate::tokenized_text_tree(text), parse_language).iter().any(|node| node.kind() == NodeKind::Abbreviation), "Trados parity abbreviation was not recognized in {text}");
    }

    for text in ["A. Andersson", "X. Exempel"] {
        assert!(!parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::Swedish).iter().any(|node| node.kind() == NodeKind::Abbreviation), "ambiguous Swedish initial was accepted in {text}");
    }

    for (text, parse_language) in [("FAHR. 32", ParseLanguage::English), ("URL. Danach", ParseLanguage::German)] {
        assert!(!parse_nodes(&crate::tokenized_text_tree(text), parse_language).iter().any(|node| node.kind() == NodeKind::Abbreviation), "all-caps token was hidden by abbreviation parity in {text}");
    }
}

#[test]
fn trados_lexical_collisions_remain_complete_words() {
    for (text, parse_language) in [
        ("Am.", ParseLanguage::English),
        ("Aug.", ParseLanguage::English),
        ("Bart.", ParseLanguage::English),
        ("Brit.", ParseLanguage::English),
        ("Cal.", ParseLanguage::English),
        ("Del.", ParseLanguage::English),
        ("Gal.", ParseLanguage::English),
        ("Ida.", ParseLanguage::English),
        ("Imp.", ParseLanguage::English),
        ("Int.", ParseLanguage::English),
        ("Jan.", ParseLanguage::English),
        ("Jun.", ParseLanguage::English),
        ("Lat.", ParseLanguage::English),
        ("Lon.", ParseLanguage::English),
        ("Mar.", ParseLanguage::English),
        ("Nat.", ParseLanguage::English),
        ("Neg.", ParseLanguage::English),
        ("Perm.", ParseLanguage::English),
        ("Sept.", ParseLanguage::English),
        ("Aug.", ParseLanguage::German),
        ("Beil.", ParseLanguage::German),
        ("Erz.", ParseLanguage::German),
        ("Graph.", ParseLanguage::German),
        ("Haupts.", ParseLanguage::German),
        ("Intern.", ParseLanguage::German),
        ("Jan.", ParseLanguage::German),
        ("Meteor.", ParseLanguage::German),
        ("Mohammed.", ParseLanguage::German),
        ("Mus.", ParseLanguage::German),
        ("Part.", ParseLanguage::German),
        ("Physik.", ParseLanguage::German),
        ("Pol.", ParseLanguage::German),
        ("Port.", ParseLanguage::German),
        ("Pos.", ParseLanguage::German),
        ("Qual.", ParseLanguage::German),
        ("Quant.", ParseLanguage::German),
        ("Reform.", ParseLanguage::German),
        ("Sowjet.", ParseLanguage::German),
        ("Span.", ParseLanguage::German),
        ("Typ.", ParseLanguage::German),
        ("Ungar.", ParseLanguage::German),
        ("Verb.", ParseLanguage::German),
    ] {
        assert!(!parse_nodes(&crate::tokenized_text_tree(text), parse_language).iter().any(|node| node.kind() == NodeKind::Abbreviation), "documented lexical collision was classified as an abbreviation: {text} ({parse_language:?})");
    }
}

#[test]
fn detects_additional_unambiguous_language_abbreviations() {
    let cases: [(&str, ParseLanguage); 57] = [
        ("fig. 1", ParseLanguage::English),
        ("dept. sales", ParseLanguage::English),
        ("tel. 123", ParseLanguage::English),
        ("app. A", ParseLanguage::English),
        ("sec. 2", ParseLanguage::English),
        ("ref. A", ParseLanguage::English),
        ("ver. 1", ParseLanguage::English),
        ("rev. B", ParseLanguage::English),
        ("addr. Main", ParseLanguage::English),
        ("fax. 123", ParseLanguage::English),
        ("art. 3", ParseLanguage::English),
        ("max. 10", ParseLanguage::English),
        ("suppl. 1", ParseLanguage::English),
        ("spec. A", ParseLanguage::English),
        ("diag. 1", ParseLanguage::English),
        ("orig. text", ParseLanguage::English),
        ("encl. A", ParseLanguage::English),
        ("qty. 5", ParseLanguage::English),
        ("misc. notes", ParseLanguage::English),
        ("mfg. date", ParseLanguage::English),
        ("mfr. name", ParseLanguage::English),
        ("acct. 123", ParseLanguage::English),
        ("amt. due", ParseLanguage::English),
        ("blvd. address", ParseLanguage::English),
        ("corp. name", ParseLanguage::English),
        ("Abb. 1", ParseLanguage::German),
        ("Abt. Vertrieb", ParseLanguage::German),
        ("Tel. 123", ParseLanguage::German),
        ("Anh. A", ParseLanguage::German),
        ("Abschn. 2", ParseLanguage::German),
        ("Ref. A", ParseLanguage::German),
        ("Vers. 1", ParseLanguage::German),
        ("Rev. B", ParseLanguage::German),
        ("Adr. Hauptstrasse", ParseLanguage::German),
        ("Fax. 123", ParseLanguage::German),
        ("Art. 3", ParseLanguage::German),
        ("Max. 10", ParseLanguage::German),
        ("Spez. A", ParseLanguage::German),
        ("Diag. 1", ParseLanguage::German),
        ("Orig. Text", ParseLanguage::German),
        ("Str. 10", ParseLanguage::German),
        ("fig. 1", ParseLanguage::Swedish),
        ("avd. sälj", ParseLanguage::Swedish),
        ("tel. 123", ParseLanguage::Swedish),
        ("bil. A", ParseLanguage::Swedish),
        ("avsn. 2", ParseLanguage::Swedish),
        ("ref. A", ParseLanguage::Swedish),
        ("ver. 1", ParseLanguage::Swedish),
        ("rev. B", ParseLanguage::Swedish),
        ("adr. Storgatan", ParseLanguage::Swedish),
        ("fax. 123", ParseLanguage::Swedish),
        ("art. 3", ParseLanguage::Swedish),
        ("max. 10", ParseLanguage::Swedish),
        ("suppl. 1", ParseLanguage::Swedish),
        ("spec. A", ParseLanguage::Swedish),
        ("diag. 1", ParseLanguage::Swedish),
        ("orig. text", ParseLanguage::Swedish),
    ];
    for (text, parse_language) in cases {
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), parse_language);
        assert!(
            nodes.iter().any(|node| node.kind() == NodeKind::Abbreviation),
            "expected an abbreviation node for {text}"
        );
    }
}

#[test]
fn parse_language_neutral_resembles_noop_parser_behavior() {
    let neutral_nodes = parse_nodes(&crate::tokenized_text_tree("Dr. 1,5"), ParseLanguage::Neutral);
    let english_nodes = parse_nodes(&crate::tokenized_text_tree("Dr. 1,5"), ParseLanguage::English);
    assert!(!neutral_nodes.iter().any(|node| node.kind() == NodeKind::Abbreviation));
    assert!(english_nodes.iter().any(|node| node.kind() == NodeKind::Abbreviation));
}

#[test]
fn placeholder_survives_internal_formatting_tags() {
    let mut run = Run::new("{user}");
    run.push_tag_at(1, TagData::new("format-1".to_string(), "g".to_string(), TagKind::Open, "<g>".to_string(), None, None));
    run.push_tag_at(5, TagData::new("format-1".to_string(), "g".to_string(), TagKind::Close, "</g>".to_string(), None, None));
    let mut tree = crate::tokenized_runs_tree([run]);
    crate::parse(&mut tree, ParseLanguage::English);

    assert_eq!(tree.preorder_iter().find_map(|node| node.placeholder_semantic()).map(|placeholder| placeholder.canonical()), Some("{user}"));
    assert_eq!(tree.preorder_iter().filter(|node| node.kind() == NodeKind::Tag).count(), 2);
}
