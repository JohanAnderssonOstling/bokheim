use crate::parser::*;
use crate::parser::test_support::*;
use crate::ParseLanguage;

#[test]
fn dotted_version_is_not_reclassified_as_a_date() {
    let nodes = parse_nodes(&crate::tokenized_text_tree("7.2.1"), ParseLanguage::German);

    assert!(nodes.iter().any(|node| node.kind() == NodeKind::Version), "expected protected version");
    assert!(!nodes.iter().any(|node| node.kind() == NodeKind::Date));
}

#[test]
fn detects_camel_case_and_snake_case_as_non_translatable() {
    for text in ["SkyShowtime", "MålVärde", "my_value", "mått_värde"] {
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::Neutral);
        assert!(nodes.iter().any(|node| node.kind() == NodeKind::NonTranslatable), "{text}");
    }
}

#[test]
fn non_translatable_excludes_trailing_punctuation() {
    let units = vec![
        u(UnitKind::Word, "SkyShowtime", 11),
        u(UnitKind::Punct, "?", 1),
    ];
    let nodes = parse_nodes(&stream(units, "SkyShowtime?"), ParseLanguage::Neutral);
    let non_translatable = nodes.iter().find(|node| node.kind() == NodeKind::NonTranslatable).expect("non-translatable");
    assert_eq!(&"SkyShowtime?"[non_translatable.span()], "SkyShowtime");
    assert_eq!(non_translatable.span().len(), 11);
    assert!(nodes.iter().any(|node| node.kind() == NodeKind::Punct));
}

#[test]
fn non_translatable_stops_before_an_apostrophe_suffix() {
    let text = "SkyShowtime's priser";
    let nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::English);
    let non_translatable = nodes.iter().find(|node| node.kind() == NodeKind::NonTranslatable).expect("non-translatable");

    assert_eq!(&text[non_translatable.span()], "SkyShowtime");
}

#[test]
fn non_translatable_never_contains_parentheses() {
    for text in ["db(A)", "SkyShowtime(test)", "ERR_CONN_42(A)", "OD3.0(A)"] {
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::Neutral);
        assert!(nodes.iter().filter(|node| node.kind() == NodeKind::NonTranslatable).all(|node| {
            let detected = &text[node.span()];
            !detected.contains('(') && !detected.contains(')')
        }), "non-translatable node contains a parenthesis in {text}");
    }

    let text = "db(A)";
    let nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::Neutral);
    assert!(!nodes.iter().any(|node| node.kind() == NodeKind::NonTranslatable));
    assert_eq!(nodes.iter().map(|node| (node.kind(), &text[node.span()])).collect::<Vec<_>>(), vec![
        (NodeKind::Word, "db"),
        (NodeKind::Punct, "("),
        (NodeKind::Word, "A"),
        (NodeKind::Punct, ")"),
    ]);
}

#[test]
fn parser_treats_versions_as_structured_protected_facts() {
    for text in ["1.2.3", "1.2.3-beta", "1.2.3-rc.1", "v1.2", "V2.0-beta", "1.2.3+build.57"] {
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::Neutral);
        let version = nodes
            .iter()
            .find(|node| node.kind() == NodeKind::Version)
            .expect("structured version");
        assert_eq!(&text[version.span()], text);
    }
}

#[test]
fn parser_does_not_treat_ambiguous_short_values_as_versions() {
    for text in ["1.2", "2.0", "3.14"] {
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::Neutral);
        assert!(!nodes.iter().any(|node| node.kind() == NodeKind::Version), "text {text}");
    }
}

#[test]
fn version_facts_expose_components_suffixes_and_cue_grounding() {
    let mut tree = crate::tokenized_text_tree("Version 2.4 and v1.2.3-rc.1+build.57");
    crate::parse(&mut tree, ParseLanguage::English);
    let versions = crate::version_facts(&tree).collect::<Vec<_>>();

    assert_eq!(versions.iter().map(|version| version.canonical()).collect::<Vec<_>>(), vec!["2.4", "1.2.3-rc.1+build.57"]);
    assert_eq!(versions[0].components(), &["2".to_string(), "4".to_string()]);
    assert_eq!(versions[1].prerelease(), Some("rc.1"));
    assert_eq!(versions[1].build_metadata(), Some("build.57"));
    assert!(crate::number_facts(&tree).next().is_none());
}

#[test]
fn parser_treats_paths_as_non_translatable() {
    for text in [
        r"C:\Users\johan\AppData\file.txt",
        "C:/Users/johan/file.txt",
        r"\\server\share\file.txt",
        "/usr/local/bin/tool",
        "~/Documents/file.txt",
        "~/Dokument/åtgärd.txt",
        "./config/settings.json",
        "../assets/logo.svg",
    ] {
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::Neutral);
        let path = nodes
            .iter()
            .find(|node| node.kind() == NodeKind::NonTranslatable)
            .expect("path non-translatable");
        assert_eq!(&text[path.span()], text);
    }
}

#[test]
fn parser_does_not_treat_ambiguous_slashes_as_paths() {
    for text in ["a/b", "foo/bar", "2026/07/08", "/typnummer/serienummer/CE-markning"] {
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::Neutral);
        assert!(!nodes.iter().any(|node| node.kind() == NodeKind::NonTranslatable), "text {text}");
    }
}

#[test]
fn parser_treats_canonical_uuids_as_non_translatable() {
    for text in [
        "550e8400-e29b-41d4-a716-446655440000",
        "550E8400-E29B-41D4-A716-446655440000",
    ] {
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::Neutral);
        let uuid = nodes
            .iter()
            .find(|node| node.kind() == NodeKind::NonTranslatable)
            .expect("uuid non-translatable");
        assert_eq!(&text[uuid.span()], text);
    }
}

#[test]
fn parser_treats_ipv4_addresses_as_ip_address() {
    for text in ["127.0.0.1", "192.168.1.10:8080", "(192.168.0.1)", "192.168.0.1,"] {
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::Neutral);
        let ip = nodes
            .iter()
            .find(|node| node.kind() == NodeKind::IpAddress)
            .expect("ip address");
        assert!(looks_like_ip_address(&text[ip.span()]));
    }
}

#[test]
fn parser_does_not_treat_invalid_ipv4_addresses_as_ip_address() {
    for text in ["999.0.0.1", "127.0.0", "127.0.0.1:0", "127.0.0.1:99999", "999.1.2.3.4", "x192.168.0.1", "192.168.0.1a"] {
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::Neutral);
        assert!(!nodes.iter().any(|node| node.kind() == NodeKind::IpAddress), "text {text}");
    }
}

#[test]
fn parser_does_not_treat_non_canonical_uuids_as_non_translatable() {
    for text in [
        "550e8400e29b41d4a716446655440000",
        "{550e8400-e29b-41d4-a716-446655440000}",
        "not-a-uuid",
    ] {
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::Neutral);
        assert!(!nodes.iter().any(|node| node.kind() == NodeKind::NonTranslatable), "text {text}");
    }
}

#[test]
fn parser_treats_hex_colors_as_non_translatable() {
    for text in ["#fff", "#ffff", "#FF00AA", "#ff00aa80"] {
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::Neutral);
        let color = nodes
            .iter()
            .find(|node| node.kind() == NodeKind::NonTranslatable)
            .expect("hex color non-translatable");
        assert_eq!(&text[color.span()], text);
    }

    let text = "#fff.";
    let nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::Neutral);
    let color = nodes
        .iter()
        .find(|node| node.kind() == NodeKind::NonTranslatable)
        .expect("hex color non-translatable");
    assert_eq!(&text[color.span()], "#fff");
}

#[test]
fn parser_does_not_treat_invalid_hex_colors_as_non_translatable() {
    for text in ["#12", "#fffff", "#tag", "#12345g"] {
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::Neutral);
        assert!(!nodes.iter().any(|node| node.kind() == NodeKind::NonTranslatable), "text {text}");
    }
}

#[test]
fn parser_treats_code_identifiers_as_non_translatable() {
    for text in ["GA-S006", "ABC-123", "TRADOS-492", "ERR_CONN_42", "FEL_ÅÄÖ2"] {
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::Neutral);
        let code = nodes
            .iter()
            .find(|node| node.kind() == NodeKind::NonTranslatable)
            .expect("code identifier non-translatable");
        assert_eq!(&text[code.span()], text);
    }
}

#[test]
fn parser_does_not_treat_ambiguous_codes_as_non_translatable() {
    for text in ["a-123", "A-1", "state-of-the-art", "AB-CD", "2026-07-08"] {
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::Neutral);
        assert!(!nodes.iter().any(|node| node.kind() == NodeKind::NonTranslatable), "text {text}");
    }
}

#[test]
fn parser_treats_compact_alnum_codes_as_non_translatable() {
    for text in ["a15", "A15", "x86", "i18n", "S006", "RTX4090", "ISO9001", "UTF8", "MUNHÄLSA1", "B2B", "B2C", "C2C", "D2C", "OD3.0", "RTX4090.2", "ISO9001.1", "i18n.2", "H2O.2"] {
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::Neutral);
        let code = nodes
            .iter()
            .find(|node| node.kind() == NodeKind::NonTranslatable)
            .unwrap_or_else(|| panic!("compact alnum code non-translatable: {text}; nodes={nodes:?}"));
        assert_eq!(&text[code.span()], text);
    }
}

#[test]
fn parser_does_not_treat_tiny_or_plain_alnum_as_compact_codes() {
    for text in ["A1", "x1", "123", "abc", "abc.2", "3.0", "OD3.foo", "OD3..0"] {
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::Neutral);
        assert!(!nodes.iter().any(|node| node.kind() == NodeKind::NonTranslatable), "text {text}");
    }
}

#[test]
fn parser_treats_long_lowercase_words_with_trailing_reference_digits_as_lexical_content() {
    for (text, language) in [("system2", ParseLanguage::English), ("tandregleringsskena2", ParseLanguage::Swedish)] {
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), language);
        assert!(!nodes.iter().any(|node| node.kind() == NodeKind::NonTranslatable), "text {text}");
        assert!(nodes.iter().any(|node| node.kind() == NodeKind::Word), "text {text}");
        assert!(nodes.iter().any(|node| node.kind() == NodeKind::Number), "text {text}");
    }
}
