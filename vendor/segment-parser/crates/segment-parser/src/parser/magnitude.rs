use super::*;

pub(super) fn canonical_numeric_magnitude_suffix(text: &str, language: ParseLanguage) -> Option<&'static str> {
    match (text, language) {
        ("M", ParseLanguage::English | ParseLanguage::Swedish | ParseLanguage::German) => Some("M"),
        ("bn", ParseLanguage::English) => Some("bn"),
        ("mn", ParseLanguage::Swedish) => Some("mn"),
        ("mdr", ParseLanguage::Swedish) => Some("mdr"),
        ("Mio", ParseLanguage::German) => Some("Mio"),
        ("Mrd", ParseLanguage::German) => Some("Mrd"),
        _ => None,
    }
}
