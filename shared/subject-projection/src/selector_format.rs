//! Canonical range syntax shared by runtime loading and bundled revision generation.

pub(crate) fn canonical_lcc_selector(selector: &str) -> String {
    if !selector.contains("..") {
        return selector.to_owned();
    }
    // Old overlays and historical migrations may still send @N. Accept the
    // syntax, but discard the number before indexing, hashing, or persistence.
    let bounds = match selector.rsplit_once('@') {
        Some((bounds, legacy)) if !legacy.is_empty() && legacy.bytes().all(|byte| byte.is_ascii_digit()) => bounds,
        _ => selector,
    };
    let (start, end) = bounds.split_once("..").expect("range separator exists");
    let (start, end) = (start.trim(), end.trim());
    if end.starts_with(|c: char| c.is_ascii_digit()) {
        let letters = start.chars().take_while(|c| c.is_ascii_alphabetic()).collect::<String>();
        format!("{start}..{letters}{end}")
    } else {
        format!("{start}..{end}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_ranges_are_canonicalized_without_changing_bounds() {
        assert_eq!(canonical_lcc_selector("GR140..153@3"), "GR140..GR153");
        assert_eq!(canonical_lcc_selector("DL701..DL702@7"), "DL701..DL702");
        assert_eq!(canonical_lcc_selector("DL701..DL702"), "DL701..DL702");
        assert_eq!(canonical_lcc_selector("DL703"), "DL703");
    }
}
