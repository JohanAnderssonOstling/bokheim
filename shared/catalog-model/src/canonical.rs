/// Normalizes an ISBN-10 or ISBN-13 to a compact, checksum-verified form.
pub fn canonical_isbn(value: &str) -> Option<String> {
    let value = value.trim();
    let value = value.strip_prefix("urn:isbn:").or_else(|| value.strip_prefix("URN:ISBN:")).unwrap_or(value);
    let compact = value.chars().filter(|character| !matches!(character, '-' | ' ')).map(|character| character.to_ascii_uppercase()).collect::<String>();
    let bytes = compact.as_bytes();
    let valid = match bytes.len() {
        10 => {
            bytes[..9].iter().all(u8::is_ascii_digit)
                && (bytes[9].is_ascii_digit() || bytes[9] == b'X')
                && bytes.iter().enumerate().map(|(index, byte)| u32::from(if *byte == b'X' { 10 } else { *byte - b'0' }) * (10 - index as u32)).sum::<u32>() % 11 == 0
        }
        13 => bytes.iter().all(u8::is_ascii_digit) && bytes.iter().enumerate().map(|(index, byte)| u32::from(*byte - b'0') * if index % 2 == 0 { 1 } else { 3 }).sum::<u32>() % 10 == 0,
        _ => false,
    };
    valid.then_some(compact)
}

/// Normalizes a DOI, stripping resolver URL prefixes, to its bare value.
pub fn canonical_doi(value: &str) -> Option<String> {
    let lower = value.trim().to_ascii_lowercase();
    let value = lower
        .strip_prefix("https://doi.org/")
        .or_else(|| lower.strip_prefix("http://doi.org/"))
        .or_else(|| lower.strip_prefix("https://dx.doi.org/"))
        .or_else(|| lower.strip_prefix("http://dx.doi.org/"))
        .or_else(|| lower.strip_prefix("doi:"))
        .unwrap_or(&lower);
    let valid = value.starts_with("10.")
        && value.split_once('/').is_some_and(|(registrant, suffix)| (4..=9).contains(&registrant[3..].len()) && registrant[3..].bytes().all(|byte| byte.is_ascii_digit()) && !suffix.is_empty())
        && !value.bytes().any(|byte| byte.is_ascii_whitespace());
    valid.then_some(value.to_owned())
}

/// Normalizes a UUID, accepting an optional `urn:uuid:` prefix.
pub fn canonical_uuid(value: &str) -> Option<String> {
    let lower = value.trim().to_ascii_lowercase();
    let value = lower.strip_prefix("urn:uuid:").unwrap_or(&lower);
    uuid::Uuid::parse_str(value).ok().map(|value| value.hyphenated().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_identifiers_are_validated_and_normalized() {
        assert_eq!(canonical_isbn("urn:isbn:978-1-23456-789-7"), Some("9781234567897".to_owned()));
        assert_eq!(canonical_isbn("978-1-23456-789-8"), None);
        assert_eq!(canonical_doi("HTTPS://DOI.ORG/10.1000/Example"), Some("10.1000/example".to_owned()));
        assert_eq!(canonical_uuid("URN:UUID:00112233-4455-6677-8899-AABBCCDDEEFF"), Some("00112233-4455-6677-8899-aabbccddeeff".to_owned()));
    }
}
