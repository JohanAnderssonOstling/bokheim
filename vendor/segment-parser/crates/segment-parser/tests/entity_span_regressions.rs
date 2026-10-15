use segment_parser::{NodeKind, ParseLanguage};

fn nodes(text: &str, lang: ParseLanguage) -> Vec<(NodeKind, String)> {
    let tree = segment_parser::parse_text_with_language(text, lang);
    tree.iter_nodes().map(|n| (n.kind(), n.text().to_string())).collect()
}

fn measurement_values(text: &str, lang: ParseLanguage) -> Vec<i128> {
    let tree = segment_parser::parse_text_with_language(text, lang);
    tree.iter_nodes()
        .find(|node| node.kind() == NodeKind::Measurement)
        .expect("measurement")
        .measurement_fact()
        .expect("measurement fact")
        .values()
        .iter()
        .map(|value| value.amount())
        .collect()
}

#[test]
fn grouped_space_measurement_is_one_value() {
    assert_eq!(measurement_values("1\u{00A0}234,5 km", ParseLanguage::Swedish), vec![1_234_500 * 10_000_000_000]);
}

#[test]
fn measurement_range_still_two_values() {
    assert_eq!(measurement_values("10-20 mm", ParseLanguage::Neutral).len(), 2);
}

#[test]
fn signed_measurement_gets_semantic_values() {
    let values = measurement_values("-5 °C", ParseLanguage::English);
    assert_eq!(values, vec![((-5.0_f64 + 273.15) * 1e10).round() as i128]);
    let values = measurement_values("−5 °C", ParseLanguage::English);
    assert_eq!(values, vec![((-5.0_f64 + 273.15) * 1e10).round() as i128]);
}

#[test]
fn date_excludes_trailing_period() {
    let text = "Vi ses 2024-06-29.";
    let all = nodes(text, ParseLanguage::Swedish);
    let date = all.iter().find(|(kind, _)| *kind == NodeKind::Date).expect("date");
    assert_eq!(date.1, "2024-06-29");
    assert!(all.iter().any(|(kind, text)| *kind == NodeKind::Punct && text == "."));
}

#[test]
fn date_detected_before_comma() {
    let all = nodes("On 2024-06-29, we met", ParseLanguage::English);
    let date = all.iter().find(|(kind, _)| *kind == NodeKind::Date).expect("date");
    assert_eq!(date.1, "2024-06-29");
}

#[test]
fn nbsp_grouped_currency_spans_full_amount() {
    for lang in [ParseLanguage::English, ParseLanguage::Swedish] {
        for text in ["1\u{00A0}234 kr", "1 234 kr"] {
            let all = nodes(text, lang);
            let currency = all.iter().find(|(kind, _)| *kind == NodeKind::Currency).expect("currency");
            assert_eq!(currency.1, text, "{lang:?}");
        }
    }
}
