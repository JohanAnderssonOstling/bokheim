use crate::parser::*;
use crate::ParseLanguage;

#[test]
fn phrase_scoring_prefers_semantic_currency_over_number() {
    let text = "5 USD";
    let nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::English);
    let first = nodes.first().expect("node");
    assert_eq!(first.kind(), NodeKind::Currency);
    assert_eq!(&text[first.span()], text);
}


#[test]
fn parser_detects_currency_entities() {
    let cases: [(&str, ParseLanguage); 5] = [
        ("€ 12,50", ParseLanguage::Swedish),
        ("12,50 kr", ParseLanguage::Swedish),
        ("SEK 1 200", ParseLanguage::Swedish),
        ("$5", ParseLanguage::Neutral),
        ("5 USD", ParseLanguage::Neutral),
    ];
    for (text, parse_language) in cases {
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), parse_language);
        let currency = nodes.iter().find(|node| node.kind() == NodeKind::Currency).expect("currency");
        assert_eq!(&text[currency.span()], text);
        assert!(currency.currency_fact().is_some_and(|currency| currency.currency().is_some()));
        assert!(currency.currency_fact().is_some());
    }
}

#[test]
fn parser_detects_trados_currency_symbols_from_symbols_list() {
    let cases = [
        "¢ 10",
        "¤ 11",
        "₡ 12",
        "₫ 13",
        "₣ 14",
        "₤ 15",
        "₧ 16",
        "₪ 17",
    ];
    for text in cases {
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::Neutral);
        let currency = nodes.iter().find(|node| node.kind() == NodeKind::Currency).expect("currency");
        assert_eq!(&text[currency.span()], text);
        assert!(currency.currency_fact().is_some_and(|currency| currency.currency().is_some()));
    }
}

#[test]
fn currency_symbols_include_trados_symbols() {
    let symbols = ["¢", "¤", "₡", "₫", "₣", "₤", "₧", "₪"];
    for symbol in symbols {
        assert!(crate::language_parser::UNIVERSAL_CURRENCY_SYMBOLS.iter().any(|lexeme| lexeme.text == symbol), "symbol {symbol}");
    }
}

#[test]
fn currency_codes_require_canonical_uppercase() {
    for text in ["15 SEK", "USD 15", "15 JPY"] {
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::Swedish);
        assert!(nodes.iter().any(|node| node.kind() == NodeKind::Currency), "text {text}");
    }

    for text in ["15 sek", "15 Sek", "usd 15", "Usd 15", "15 jpy", "15 JpY"] {
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::Swedish);
        assert!(!nodes.iter().any(|node| node.kind() == NodeKind::Currency), "text {text}");
    }
}


#[test]
fn parser_does_not_treat_ambiguous_weight_words_as_currency() {
    let cases = [
        ("The baby weighed 8 pounds.", ParseLanguage::English),
        ("Barnet vägde 8 pund.", ParseLanguage::Swedish),
        ("Das Baby wog 8 Pfund.", ParseLanguage::German),
    ];
    for (text, language) in cases {
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), language);
        assert!(!nodes.iter().any(|node| node.kind() == NodeKind::Currency), "text {text}");
        assert!(nodes.iter().any(|node| node.kind() == NodeKind::Measurement), "text {text}");
    }
}
