//! Canonical range syntax shared by runtime loading and bundled revision generation.

pub fn canonical_lcc_selector(selector: &str) -> String {
    if !selector.contains("..") {
        return selector.to_owned();
    }
    let (start, end) = selector.split_once("..").expect("range separator exists");
    let (start, end) = (start.trim(), end.trim());
    if end.starts_with(|c: char| c.is_ascii_digit()) {
        let letters = start.chars().take_while(|c| c.is_ascii_alphabetic()).collect::<String>();
        format!("{start}..{letters}{end}")
    } else {
        format!("{start}..{end}")
    }
}
