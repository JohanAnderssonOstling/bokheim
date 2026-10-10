use unicode_normalization::char::is_combining_mark;
use unicode_normalization::UnicodeNormalization;

/// Normalizes user-visible metadata for strict, case-insensitive contiguous
/// substring search. Diacritics are removed and whitespace runs collapse to a
/// single space; punctuation remains significant.
pub fn normalize_search_text(value: &str) -> String {
    let mut normalized = String::new();
    let mut pending_space = false;
    for character in value.nfkd().filter(|character| !is_combining_mark(*character)).flat_map(char::to_lowercase) {
        if character.is_whitespace() {
            pending_space = !normalized.is_empty();
        } else {
            if pending_space {
                normalized.push(' ');
                pending_space = false;
            }
            normalized.push(character);
        }
    }
    normalized
}

pub fn normalized_substring_match(value: &str, normalized_query: &str) -> bool {
    !normalized_query.is_empty() && normalize_search_text(value).contains(normalized_query)
}
