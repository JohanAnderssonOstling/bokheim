//! Conservative publication-identity evidence derived from filename-like titles.
//!
//! These helpers only prepare lookup evidence. They do not mutate embedded
//! publication metadata or claim that a filename identifies an edition.

use std::collections::BTreeSet;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EditionIdentityEvidence {
    pub title: String,
    pub author: Option<String>,
    pub publication_year: Option<i32>,
}

/// Extracts conservative title, author, and year evidence from a title that
/// may have originated from a local filename.
pub fn edition_identity_evidence(title: &str) -> EditionIdentityEvidence {
    let trimmed = title.trim();
    let segments = parenthetical_segments(trimmed);
    let mut inferred_author = None;
    let mut inferred_year = None;
    for segment in &segments {
        inferred_year = inferred_year.or_else(|| plausible_publication_year(segment));
        if inferred_author.is_some() {
            continue;
        }
        let lower = segment.to_lowercase();
        let (author, explicit) = if lower.starts_with("by-") || lower.starts_with("by_") || lower.starts_with("by ") { (&segment[3..], true) } else { (segment.as_str(), false) };
        let author = author.replace(['-', '_'], " ").split_whitespace().collect::<Vec<_>>().join(" ");
        if !author.is_empty() && (explicit || looks_like_person_name(&author)) {
            inferred_author = Some(author);
        }
    }
    let stripped = title_without_parenthetical_segments(trimmed);
    let clean_tokens = stripped
        .split_whitespace()
        .filter(|token| {
            let key = token.chars().filter(|character| character.is_alphanumeric()).flat_map(char::to_lowercase).collect::<String>();
            !key.is_empty() && !matches!(key.as_str(), "itebooks" | "ibooker" | "zliborg" | "zlibrary")
        })
        .collect::<Vec<_>>();
    let cleaned = clean_tokens.join(" ").trim_matches(|character: char| character.is_whitespace() || matches!(character, '-' | '_' | '–' | '—')).trim().to_owned();
    let mut title = if cleaned.is_empty() { stripped } else { cleaned };
    if let Some(by) = rfind_ascii_case_insensitive(&title, " by ") {
        let inline_author = title[by + 4..].trim();
        if looks_like_person_name(inline_author) {
            inferred_author.get_or_insert_with(|| inline_author.to_owned());
            title = title[..by].trim_matches(|character: char| character.is_whitespace() || matches!(character, '-' | '_' | '–' | '—')).to_owned();
        }
    }
    EditionIdentityEvidence { title, author: inferred_author, publication_year: inferred_year }
}

/// Returns one unambiguous, checksum-valid ISBN embedded in a filename-like
/// title. Multiple distinct ISBNs deliberately return `None`.
pub fn isbn_from_filename_like_title(title: &str) -> Option<String> {
    let mut candidate = String::new();
    let mut isbns = BTreeSet::new();
    let finish_candidate = |candidate: &mut String, isbns: &mut BTreeSet<String>| {
        if let Some(isbn) = library_model::canonical_isbn(candidate) {
            isbns.insert(isbn);
        }
        candidate.clear();
    };
    for character in title.chars() {
        if character.is_ascii_digit() || matches!(character, 'x' | 'X' | '-') {
            candidate.push(character);
        } else if !candidate.is_empty() {
            finish_candidate(&mut candidate, &mut isbns);
        }
    }
    if !candidate.is_empty() {
        finish_candidate(&mut candidate, &mut isbns);
    }
    (isbns.len() == 1).then(|| isbns.into_iter().next()).flatten()
}

fn title_without_parenthetical_segments(title: &str) -> String {
    let mut depth = 0_u32;
    title
        .chars()
        .filter(|character| match character {
            '(' => {
                depth = depth.saturating_add(1);
                false
            }
            ')' if depth > 0 => {
                depth -= 1;
                false
            }
            _ => depth == 0,
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn rfind_ascii_case_insensitive(value: &str, needle: &str) -> Option<usize> {
    value.as_bytes().windows(needle.len()).rposition(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

fn parenthetical_segments(value: &str) -> Vec<String> {
    let mut depth = 0_u32;
    let mut segment = String::new();
    let mut segments = Vec::new();
    for character in value.chars() {
        match character {
            '(' => {
                if depth > 0 {
                    segment.push(character);
                }
                depth = depth.saturating_add(1);
            }
            ')' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    let value = segment.trim();
                    if !value.is_empty() {
                        segments.push(value.to_owned());
                    }
                    segment.clear();
                } else {
                    segment.push(character);
                }
            }
            _ if depth > 0 => segment.push(character),
            _ => {}
        }
    }
    segments
}

fn plausible_publication_year(value: &str) -> Option<i32> {
    value.split(|character: char| !character.is_ascii_digit()).filter(|part| part.len() == 4).filter_map(|part| part.parse::<i32>().ok()).find(|year| (1450..=2100).contains(year))
}

fn looks_like_person_name(value: &str) -> bool {
    const PARTICLES: &[&str] = &["al", "bin", "da", "de", "del", "den", "der", "di", "du", "la", "le", "van", "von"];
    const NON_NAMES: &[&str] = &["abridged", "book", "edition", "guide", "illustrated", "introduction", "manual", "novel", "revised", "series", "textbook", "translation", "unabridged", "volume"];
    let words = value.split_whitespace().collect::<Vec<_>>();
    if !(2..=5).contains(&words.len()) || words.iter().any(|word| NON_NAMES.contains(&word.to_lowercase().as_str())) {
        return false;
    }
    words.iter().all(|word| {
        let clean = word.trim_matches(|character: char| matches!(character, '.' | ',' | '\'' | '’'));
        if clean.is_empty() || !clean.chars().all(|character| character.is_alphabetic() || matches!(character, '-' | '\'' | '’')) {
            return false;
        }
        PARTICLES.contains(&clean.to_lowercase().as_str()) || clean.chars().next().is_some_and(char::is_uppercase)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_explicit_filename_author() {
        assert_eq!(edition_identity_evidence("An Illustrated Guide (by-Kenneth-Ray-Stubbs)"), EditionIdentityEvidence { title: "An Illustrated Guide".to_owned(), author: Some("Kenneth Ray Stubbs".to_owned()), publication_year: None });
    }

    #[test]
    fn extracts_plain_person_parenthesis_conservatively() {
        assert_eq!(edition_identity_evidence("An Illustrated Guide (Kenneth Ray Stubbs)").author.as_deref(), Some("Kenneth Ray Stubbs"));
        assert_eq!(edition_identity_evidence("Physical Chemistry (Second Edition)").author, None);
    }

    #[test]
    fn extracts_noisy_filename_title_author_and_year() {
        assert_eq!(
            edition_identity_evidence("(it-ebooks-2019) it-ebooks - Algorithms (Jeff Erickson)-iBooker it-ebooks (2019)(1)"),
            EditionIdentityEvidence { title: "Algorithms".to_owned(), author: Some("Jeff Erickson".to_owned()), publication_year: Some(2019) }
        );
    }

    #[test]
    fn extracts_inline_by_author_before_source_tag() {
        assert_eq!(
            edition_identity_evidence("A Concise History of France by Roger Price (z-lib.org)"),
            EditionIdentityEvidence { title: "A Concise History of France".to_owned(), author: Some("Roger Price".to_owned()), publication_year: None }
        );
    }

    #[test]
    fn filename_isbn_requires_one_checksum_valid_value() {
        assert_eq!(isbn_from_filename_like_title("9780198571001.pdf").as_deref(), Some("9780198571001"));
        assert_eq!(isbn_from_filename_like_title("book-978-0-19-857100-1 scan.pdf").as_deref(), Some("9780198571001"));
        assert_eq!(isbn_from_filename_like_title("9780198571002.pdf"), None);
        assert_eq!(isbn_from_filename_like_title("9780198571001 and 9781501168697.pdf"), None);
    }
}
