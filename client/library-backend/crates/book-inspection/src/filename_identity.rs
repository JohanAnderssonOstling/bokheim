//! Filename evidence for unusable identity fields; never infers identifiers.

pub fn unusable_identity(value: &str) -> bool {
    let value = value.trim();
    let compact = value.replace('-', "");
    matches!(value.to_ascii_lowercase().as_str(), "" | "unknown" | "untitled") || (compact.len() >= 20 && compact.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Common production labels, document filenames and standalone web addresses.
pub fn standard_title_placeholder(value: &str) -> bool {
    let lower = value.trim().to_ascii_lowercase();
    let label = lower.split_whitespace().collect::<Vec<_>>().join(" ");
    let label = label.replace(['-', '_'], " ");
    let document_file = [".fb2.zip", ".fb2", ".fbz", ".cbz", ".pdf", ".doc", ".docx", ".epub", ".mobi", ".azw", ".azw3", ".m4b", ".rtf", ".odt", ".indd", ".qxd", ".p65"]
        .iter()
        .any(|extension| lower.ends_with(extension) || lower.split_once(':').is_some_and(|(name, _)| name.ends_with(extension)));
    let address = lower.trim_matches(['<', '>', '(', ')', '[', ']']);
    let host = address.split('/').next().unwrap_or(address);
    let domain = !address.chars().any(char::is_whitespace)
        && [".org", ".com", ".net", ".edu", ".gov", ".info", ".biz", ".io", ".co", ".uk", ".se"]
            .iter()
            .any(|suffix| host.strip_suffix(suffix).is_some_and(|name| !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.'))));
    matches!(label.as_str(), "ebook" | "cover" | "front cover" | "back cover" | "cover page" | "front matter" | "frontmatter" | "back matter" | "backmatter" | "title page" | "titlepage" | "table of contents" | "contents" | "copyright page")
        || ["_ebook", "-ebook", ".ebook"].iter().any(|suffix| lower.ends_with(suffix))
        || document_file
        || domain
        || address.starts_with("www.")
        || address.starts_with("http://")
        || address.starts_with("https://")
}

/// Standalone Kindle-style identifiers, optionally followed by the EBOK marker.
/// Require the whole field to match so books discussing identifiers keep their title.
pub fn kindle_identifier_title(value: &str) -> bool {
    let upper = value.trim().to_ascii_uppercase();
    let identifier =
        upper.strip_suffix("EBOK").and_then(|prefix| prefix.strip_suffix(|c: char| c.is_whitespace() || matches!(c, '_' | '-'))).map(|prefix| prefix.trim_end_matches(|c: char| c.is_whitespace() || matches!(c, '_' | '-'))).unwrap_or(&upper);
    identifier.len() == 10 && identifier.starts_with("B0") && identifier.bytes().all(|c| c.is_ascii_alphanumeric())
}

/// Ambiguous product-code shape; only replace it when a usable filename exists.
pub fn identifier_shaped_title(value: &str) -> bool {
    let value = value.trim();
    let token = value
        .get(value.len().saturating_sub(4)..)
        .filter(|suffix| suffix.eq_ignore_ascii_case("EBOK"))
        .and_then(|_| value.get(..value.len().saturating_sub(4)))
        .and_then(|prefix| prefix.strip_suffix(|c: char| c.is_whitespace() || matches!(c, '_' | '-')))
        .map(|prefix| prefix.trim_end_matches(|c: char| c.is_whitespace() || matches!(c, '_' | '-')))
        .unwrap_or(value);
    token.len() >= 10 && token.bytes().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()) && token.bytes().filter(u8::is_ascii_digit).count() >= 2 && token.bytes().filter(u8::is_ascii_uppercase).count() >= 2
}

/// Long identifier tokens remain invalid when surrounded by ordinary title text.
pub(crate) fn contains_long_identifier(value: &str) -> bool {
    value
        .split(|c: char| !c.is_alphanumeric())
        .any(|token| token.len() >= 20 && token.is_ascii() && (token.bytes().all(|c| c.is_ascii_hexdigit()) || (token.bytes().filter(u8::is_ascii_digit).count() >= 2 && token.bytes().filter(u8::is_ascii_alphabetic).count() >= 2)))
}

/// Reject recognizable technical placeholders, not merely unusual book names.
pub fn unusable_title(value: &str) -> bool {
    let value = value.trim();
    let lower = value.to_ascii_lowercase();
    let stem = [".fb2.zip", ".fb2", ".fbz", ".cbz", ".pdf", ".epub", ".mobi", ".azw3", ".m4b"].iter().find_map(|extension| lower.strip_suffix(extension)).unwrap_or(&lower);
    let number = ["urn:isbn:", "isbn-13:", "isbn-10:", "isbn"].iter().find_map(|prefix| stem.trim().strip_prefix(prefix)).unwrap_or(stem).trim_start_matches([':', ' ', '-']);
    let compact = number.replace([' ', '-'], "");
    let numeric_identifier = compact.len() >= 8 && compact.bytes().all(|c| c.is_ascii_digit() || c == b'x');
    let placeholder = lower.strip_prefix("microsoft word - ").unwrap_or(&lower);
    let numbered_placeholder = ["document", "untitled"].iter().any(|prefix| placeholder.strip_prefix(prefix).is_some_and(|tail| tail.is_empty() || tail.bytes().all(|byte| byte.is_ascii_digit())));
    unusable_identity(value)
        || contains_long_identifier(value)
        || kindle_identifier_title(value)
        || standard_title_placeholder(value)
        || lower.strip_prefix("oup_").is_some_and(|suffix| !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit()))
        || numeric_identifier
        || !super::filename_isbn::from_filename(value).is_empty()
        || unusable_identity(stem)
        || value.len() > 4096
        || value.chars().any(|c| c == '\u{fffd}' || (c.is_control() && !c.is_whitespace()))
        || !value.chars().any(char::is_alphanumeric)
        || numbered_placeholder
        || matches!(lower.as_str(), "true" | "none" | "null" | "n/a" | "microsoft word" | "adobe acrobat" | "acrobat distiller")
        || lower.starts_with("file://")
        || value.starts_with("\\\\")
        || (value.as_bytes().get(1) == Some(&b':') && value.as_bytes().get(2).is_some_and(|c| matches!(c, b'/' | b'\\')))
        || ["/home/", "/users/", "/tmp/", "/private/"].iter().any(|prefix| lower.starts_with(prefix))
}

