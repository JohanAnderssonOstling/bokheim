//! Shared bibliographic normalization for ebook and audiobook discovery.
use std::collections::BTreeSet;
mod normalized;
pub use normalized::{author_suffix, matching_author_count, NormalizedAuthor, NormalizedTitle};
use unicode_normalization::UnicodeNormalization;

pub fn bibliographic_match_key(value: &str) -> String {
    use unicode_normalization::char::is_combining_mark;
    let mut key = String::new();
    let mut separator = false;
    for character in value.nfkd().flat_map(char::to_lowercase) {
        if is_combining_mark(character) {
            continue;
        }
        if character.is_alphanumeric() {
            if separator && !key.is_empty() {
                key.push(' ');
            }
            key.push(character);
            separator = false;
        } else {
            separator = true;
        }
    }
    key
}

/// Match punctuation variants such as Owner's/Owners without merging words
/// separated by other punctuation. Keep the legacy key for stored-index queries.
pub fn title_apostrophe_variant(value: &str) -> String {
    let chars = value.chars().collect::<Vec<_>>();
    chars
        .iter()
        .enumerate()
        .filter_map(|(i, &c)| {
            let internal = matches!(c, '\'' | '’' | 'ʼ') && i > 0 && i + 1 < chars.len() && chars[i - 1].is_alphanumeric() && chars[i + 1].is_alphanumeric();
            (!internal).then_some(c)
        })
        .collect()
}

pub fn bibliographic_title_match_key(value: &str) -> String {
    bibliographic_match_key(&title_apostrophe_variant(value))
}

/// Honorifics affect display, not author identity. Handle surname-first credits too.
pub fn author_name_match_key(value: &str) -> String {
    value
        .split(',')
        .map(|part| {
            let key = bibliographic_match_key(part);
            key.split_whitespace().skip_while(|word| matches!(*word, "dr" | "doctor" | "prof" | "professor" | "mr" | "mrs" | "ms" | "miss" | "sir" | "dame" | "rev" | "reverend")).collect::<Vec<_>>().join(" ")
        })
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn author_match_keys(value: &str) -> BTreeSet<String> {
    let mut keys = BTreeSet::from([author_name_match_key(value)]);
    if let Some((family, given)) = value.split_once(',') {
        keys.insert(author_name_match_key(&format!("{given} {family}")));
    }
    keys.remove("");
    keys
}

/// Whether two credits name the same person: the names agree once accents,
/// honorifics and surname-first order are set aside.
pub fn same_author(left: &str, right: &str) -> bool {
    !author_match_keys(left).is_disjoint(&author_match_keys(right))
}

pub fn author_match_tokens(value: &str) -> BTreeSet<String> {
    NormalizedAuthor::new(value).candidate_tokens()
}

/// Compare author credits, allowing an omitted middle initial only. Full-name
/// tokens and their order must agree; conflicting stated initials stay distinct.
pub fn author_names_compatible(left: &str, right: &str) -> bool {
    let left = NormalizedAuthor::new(left);
    let right = NormalizedAuthor::new(right);
    if !left.keys.is_disjoint(&right.keys) {
        return true;
    }
    left.variants.iter().any(|a| {
        right.variants.iter().any(|b| {
            if a.suffix.is_some() && b.suffix.is_some() && a.suffix != b.suffix {
                return false;
            }
            let a = a.words.iter().map(String::as_str).collect::<Vec<_>>();
            let b = b.words.iter().map(String::as_str).collect::<Vec<_>>();
            if a.len() < 2 || b.len() < 2 || a[0] != b[0] || a.last() != b.last() || a[0].chars().count() < 2 || a.last().unwrap().chars().count() < 2 {
                return false;
            }
            if a.len() == b.len() && a.iter().zip(&b).all(|(a, b)| a == b || (a.chars().count() == 1 && b.starts_with(a)) || (b.chars().count() == 1 && a.starts_with(b))) {
                return true;
            }
            let parts = |tokens: &[&str]| {
                let mut names = Vec::new();
                let mut gaps = vec![Vec::new()];
                for token in tokens {
                    if token.chars().count() == 1 {
                        gaps.last_mut().unwrap().push((*token).to_owned());
                    } else {
                        names.push((*token).to_owned());
                        gaps.push(Vec::new());
                    }
                }
                (names, gaps)
            };
            let (a_names, a_gaps) = parts(&a);
            let (b_names, b_gaps) = parts(&b);
            a_names == b_names && a_gaps.iter().zip(&b_gaps).all(|(a, b)| a.is_empty() || b.is_empty() || a == b)
        })
    })
}


/// Bounded whole-word title fragments shared with edition candidate lookup.
pub fn title_substring_keys(value: &str, limit: usize) -> Vec<(String, usize)> {
    let normalized = bibliographic_title_match_key(value);
    let words = normalized.split_whitespace().take(limit).collect::<Vec<_>>();
    let mut substrings = std::collections::BTreeMap::<String, usize>::new();
    for start in 0..words.len() {
        for end in start + 1..=words.len() {
            let substring = words[start..end].join(" ");
            let specificity = substring.chars().count();
            substrings.entry(substring).or_insert(specificity);
        }
    }
    let mut substrings = substrings.into_iter().collect::<Vec<_>>();
    substrings.sort_by(|(left_title, left_specificity), (right_title, right_specificity)| right_specificity.cmp(left_specificity).then_with(|| left_title.cmp(right_title)));
    substrings.truncate(limit);
    substrings
}

/// Words whose parenthetical placement must never hide a structural distinction.
pub fn is_title_qualifier_word(word: &str) -> bool {
    matches!(word, "volume" | "vol" | "part" | "book" | "abridged" | "abridgment" | "abridgement" | "adapted" | "adaptation" | "retold" | "retelling" | "workbook" | "activity" | "selections" | "solutions" | "answers")
}

pub fn title_qualifiers(title: &str) -> BTreeSet<String> {
    let normalized = bibliographic_match_key(title);
    let words = normalized.split_whitespace().collect::<Vec<_>>();
    let mut qualifiers = BTreeSet::new();
    for (index, word) in words.iter().enumerate() {
        if matches!(*word, "volume" | "vol" | "part" | "book") {
            if let Some(number) = words.get(index + 1).and_then(|s| match *s {
                "i" | "one" => Some("1"),
                "ii" | "two" => Some("2"),
                "iii" | "three" => Some("3"),
                "iv" | "four" => Some("4"),
                "v" | "five" => Some("5"),
                "vi" | "six" => Some("6"),
                "vii" | "seven" => Some("7"),
                "viii" | "eight" => Some("8"),
                "ix" | "nine" => Some("9"),
                "x" | "ten" => Some("10"),
                s if s.bytes().all(|c| c.is_ascii_digit()) => Some(s),
                _ => None,
            }) {
                qualifiers.insert(format!("{}:{number}", if *word == "vol" { "volume" } else { word }));
            }
        }
        if matches!(*word, "abridged" | "abridgment" | "abridgement" | "adapted" | "adaptation" | "retold" | "retelling" | "workbook" | "activity" | "selections") {
            qualifiers.insert(word.to_string());
        }
    }
    qualifiers
}

/// Split explicit coauthor separators without confusing `Family, Given` names.
pub fn author_credits(values: &[String]) -> Vec<String> {
    let mut credits = Vec::new();
    for value in values {
        let mut parts = vec![value.trim().to_owned()];
        for separator in [" with ", " and ", " & ", ";"] {
            parts = parts
                .into_iter()
                .flat_map(|part| {
                    let mut result = Vec::new();
                    let mut rest = part.as_str();
                    while let Some(index) = rest.as_bytes().windows(separator.len()).position(|window| window.eq_ignore_ascii_case(separator.as_bytes())) {
                        result.push(rest[..index].trim().to_owned());
                        rest = &rest[index + separator.len()..];
                    }
                    result.push(rest.trim().to_owned());
                    result
                })
                .collect();
        }
        for part in parts {
            let chunks = part.split(',').map(str::trim).collect::<Vec<_>>();
            let is_suffix = |s: &str| matches!(bibliographic_match_key(s).as_str(), "jr" | "sr" | "ii" | "iii" | "iv");
            // A suffix disambiguates an inverted name followed by full coauthor names.
            let expanded = if chunks.len() >= 3 && is_suffix(chunks[2]) && chunks[3..].iter().all(|s| s.split_whitespace().count() >= 2) {
                let mut names = vec![format!("{} {} {}", chunks[1], chunks[0], chunks[2])];
                names.extend(chunks[3..].iter().map(|s| (*s).to_owned()));
                names
            } else {
                vec![part]
            };
            for name in expanded {
                if !name.is_empty() && !credits.contains(&name) {
                    credits.push(name);
                }
            }
        }
    }
    credits
}

/// Complete groups can mix parentheses and brackets. Unclosed groups remain
/// intact so cleanup cannot swallow the remaining title.
pub fn delimited_segments(value: &str) -> Vec<(usize, usize)> {
    let mut stack = Vec::new();
    let mut start = 0;
    let mut spans = Vec::new();
    for (index, character) in value.char_indices() {
        match character {
            '(' | '[' => {
                if stack.is_empty() {
                    start = index;
                }
                stack.push(character);
            }
            ')' | ']' if !stack.is_empty() => {
                stack.pop();
                if stack.is_empty() {
                    spans.push((start, index + 1));
                }
            }
            _ => {}
        }
    }
    spans
}

/// Drops parenthetical groups such as series names. Volume and adaptation
/// qualifiers always stay; edition notes stay when `keep_edition_notes`.
pub fn title_without_parenthetical_segments(title: &str, keep_edition_notes: bool) -> String {
    let mut outside = String::new();
    let mut previous = 0;
    for (start, end) in delimited_segments(title) {
        outside.push_str(&title[previous..start]);
        let inside = &title[start + 1..end - 1];
        let key = bibliographic_match_key(inside);
        if key.split_whitespace().any(|word| is_title_qualifier_word(word) || (keep_edition_notes && matches!(word, "edition" | "ed" | "revised" | "revision" | "unabridged" | "translation" | "translated"))) {
            outside.push(' ');
            outside.push_str(&title[start..end]);
        }
        previous = end;
    }
    outside.push_str(&title[previous..]);
    outside.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::same_author;

    #[test]
    fn same_author_ignores_order_accents_and_honorifics() {
        assert!(same_author("Yakobson, Alexander", " ALEXANDER  YAKOBSON "));
        assert!(same_author("García, José", "Jose Garcia"));
        assert!(same_author("Dr John Smith", "John Smith"));
        assert!(!same_author("Alexander Yakobson", "Alexander Smith"));
        assert!(!same_author("John Smith", "John A. Smith"));
        assert!(!same_author("...", "---"));
    }
}
