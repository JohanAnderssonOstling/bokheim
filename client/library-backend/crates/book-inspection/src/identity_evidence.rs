//! Conservative title, author and year evidence read from filename-like titles.
//!
//! Import cleans a book's own title with it. Catalogue titles never take these
//! shapes, so the metadata server compares plain titles and never needs it.

use metadata_contract::matching::{bibliographic_match_key, delimited_segments, title_without_parenthetical_segments};
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EditionIdentityEvidence {
    pub title: String,
    pub author: Option<String>,
    pub book_year: Option<i32>,
}

/// Extracts conservative title, author, and year evidence from a title that
/// may have originated from a local filename.
pub fn edition_identity_evidence(title: &str) -> EditionIdentityEvidence {
    parse_identity_evidence(title, false, None)
}

/// Filenames use an explicit ` by ` delimiter for trailing author credits.
pub fn filename_identity_evidence(title: &str) -> EditionIdentityEvidence {
    parse_identity_evidence(title, true, None)
}

/// Recover multiple subtitle segments only when the first segment matches a known title.
pub fn filename_identity_evidence_with_title(title: &str, anchor: &str) -> EditionIdentityEvidence {
    parse_identity_evidence(title, true, Some(anchor))
}

/// Recognize a second subtitle that repeats the first subtitle's whole-word prefix.
fn overlapping_subtitles(value: &str) -> Option<String> {
    let parts = value.split('_').filter(|part| !part.is_empty()).map(str::trim).collect::<Vec<_>>();
    if parts.len() != 3 || parts.iter().any(|part| part.is_empty()) {
        return None;
    }
    let first = bibliographic_match_key(parts[1]);
    let later = title_without_parenthetical_segments(parts[2], true);
    // A repeated subtitle prefix can have a trailing publisher suffix.
    let later = bibliographic_match_key(later.split(['-', '–', '—']).next()?.trim());
    if later.split_whitespace().count() < 2 {
        return None;
    }
    if first != later && !first.strip_prefix(&later).is_some_and(|rest| rest.starts_with(' ')) {
        return None;
    }
    Some(format!("{}: {}", parts[0], parts[1]))
}

fn parse_identity_evidence(title: &str, filename: bool, anchor: Option<&str>) -> EditionIdentityEvidence {
    // Only underscore groups followed by whitespace can mark a subtitle.
    // Other groups are filename punctuation, not additional subtitle candidates.
    let title = title.trim_end();
    let title = title.rsplit_once(" -- ").filter(|(_, tag)| tag.replace('’', "'").eq_ignore_ascii_case("Anna's Archive")).map_or(title, |(title, _)| title);
    let production = clean_production_title(title).replace(';', ": ");
    let trimmed = production.trim();
    let mut stripped = title_without_parenthetical_segments(trimmed, true);
    // Remove only separators attached to metadata groups that cleanup consumed.
    let groups = delimited_segments(trimmed);
    if groups.first().is_some_and(|(start, end)| *start == 0 && title_without_parenthetical_segments(&trimmed[..*end], true).is_empty()) {
        stripped = stripped.trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '-' | '–' | '—')).to_owned();
    }
    if groups.last().is_some_and(|(start, end)| *end == trimmed.len() && title_without_parenthetical_segments(&trimmed[*start..], true).is_empty()) {
        stripped = stripped.trim_end_matches(|c: char| c.is_whitespace() || matches!(c, '-' | '–' | '—')).to_owned();
    }
    let cleaned = stripped;

    let groups = cleaned
        .match_indices('_')
        .filter(|(index, _)| *index == 0 || cleaned.as_bytes()[index - 1] != b'_')
        .map(|(start, _)| {
            let end = start + cleaned[start..].bytes().take_while(|byte| *byte == b'_').count();
            (start, end)
        })
        .collect::<Vec<_>>();
    let candidates = groups.iter().copied().filter(|(_, end)| cleaned[*end..].starts_with(char::is_whitespace)).collect::<Vec<_>>();
    let subtitle_normalized = if let Some(title) = overlapping_subtitles(&cleaned) {
        title
    } else if let [(start, end)] = candidates.as_slice() {
        format!("{}: {}", cleaned[..*start].replace('_', " "), cleaned[*end..].trim_start().replace('_', " "))
    } else if candidates.len() > 1 && anchor.is_some_and(|anchor| !anchor.trim().is_empty() && bibliographic_match_key(&cleaned[..candidates[0].0]) == bibliographic_match_key(anchor)) {
        let mut parts = Vec::new();
        let mut previous = 0;
        for (start, end) in &candidates {
            parts.push(cleaned[previous..*start].replace('_', " ").trim().to_owned());
            previous = *end;
        }
        parts.push(cleaned[previous..].replace('_', " ").trim().to_owned());
        if parts.iter().all(|part| !part.is_empty()) {
            parts.join(": ")
        } else {
            cleaned.replace('_', " ")
        }
    } else if groups.len() > 1 {
        cleaned.replace('_', " ")
    } else {
        cleaned
    };
    let trimmed = subtitle_normalized.trim();
    // Structured archive filenames explicitly delimit title, credits and year.
    let fields = trimmed.split(" -- ").map(str::trim).collect::<Vec<_>>();
    if fields.len() >= 3 && !fields[0].is_empty() && !fields[1].is_empty() {
        if let Some(year) = fields[2].parse::<i32>().ok().filter(|year| (1450..=2100).contains(year)) {
            return EditionIdentityEvidence { title: fields[0].to_owned(), author: Some(fields[1].to_owned()), book_year: Some(year) };
        }
    }
    let segments = parenthetical_segments(title);
    let mut inferred_author = None;
    let mut inferred_year = None;
    for segment in &segments {
        inferred_year = inferred_year.or_else(|| plausible_book_year(segment));
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
    let stripped = trimmed;
    let tokens = stripped.split_whitespace().collect::<Vec<_>>();
    let distribution_tag = |token: &str| {
        let key = token.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect::<String>();
        matches!(key.as_str(), "itebooks" | "ibooker" | "zliborg" | "zlibrary" | "libgenli")
    };
    let clean_tokens = tokens
        .iter()
        .enumerate()
        .filter_map(|(index, token)| {
            let metadata_separator = matches!(*token, "-" | "–" | "—") && (tokens.get(index + 1).is_some_and(|next| distribution_tag(next)) || index.checked_sub(1).is_some_and(|previous| distribution_tag(tokens[previous])));
            (!distribution_tag(token) && !metadata_separator && !token.chars().all(|c| c == '_')).then_some(*token)
        })
        .collect::<Vec<_>>();
    let cleaned = clean_tokens.join(" ").trim().to_owned();
    let mut title = if cleaned.is_empty() { stripped.to_owned() } else { cleaned };
    if let Some(by) = rfind_ascii_case_insensitive(&title, " by ") {
        let inline_author = title[by + 4..].trim();
        if by > 0 && !inline_author.is_empty() && (filename || looks_like_person_name(inline_author)) {
            inferred_author.get_or_insert_with(|| inline_author.to_owned());
            title = title[..by].trim_matches(|character: char| character.is_whitespace() || matches!(character, '-' | '_' | '–' | '—')).to_owned();
        }
    }
    if let Some((base, suffix)) = title.rsplit_once('-') {
        let suffix = suffix.trim();
        if suffix.len() >= 6 && suffix.bytes().all(|b| b.is_ascii_digit()) && base.matches('-').count() >= 2 && !base.split('-').any(|word| matches!(word.to_ascii_lowercase().as_str(), "volume" | "vol" | "part" | "book" | "edition")) {
            title = base.to_owned();
        }
    }
    // A whole title written as hyphen-delimited words is filename formatting.
    // Require several words; preserve ordinary compounds and numeric ranges.
    let parts = title.split('-').map(str::trim).collect::<Vec<_>>();
    if parts.len() >= 4 && parts.iter().all(|part| !part.is_empty() && part.chars().all(|c| c.is_alphabetic() || matches!(c, '\'' | '’'))) {
        title = parts.join(" ");
    }
    EditionIdentityEvidence { title, author: inferred_author, book_year: inferred_year }
}

/// Normalize unmistakable release naming while preserving initials and decimals.
fn clean_production_title(title: &str) -> String {
    let title = title.trim();
    let lower = title.to_ascii_lowercase();
    let title = [".ebook-een", " ebook-een", ".een", " een"].iter().find_map(|suffix| lower.strip_suffix(suffix).map(|base| &title[..base.len()])).unwrap_or(title);
    let mut words = Vec::new();
    for word in title.split_whitespace() {
        let parts = word.split('.').collect::<Vec<_>>();
        let long_words = parts.iter().filter(|part| part.chars().count() > 2 && part.chars().all(char::is_alphabetic)).count();
        if parts.len() >= 4 && long_words >= 3 {
            let mut normalized = String::new();
            for (i, part) in parts.iter().enumerate() {
                if i > 0 {
                    let previous = parts[i - 1];
                    let letters = previous.chars().all(char::is_alphabetic) && part.chars().all(char::is_alphabetic);
                    let short = previous.chars().count() <= 2 && part.chars().count() <= 2;
                    normalized.push(if !previous.is_empty() && !part.is_empty() && letters && !short { ' ' } else { '.' });
                }
                normalized.push_str(part);
            }
            words.push(normalized);
        } else {
            words.push(word.to_owned());
        }
    }
    let mut cleaned = Vec::new();
    let mut i = 0;
    while i < words.len() {
        if words[i].bytes().all(|b| b.is_ascii_digit()) && words.get(i + 1).is_some_and(|word| word.eq_ignore_ascii_case("dpi")) {
            i += 2;
        } else {
            cleaned.push(words[i].as_str());
            i += 1;
        }
    }
    cleaned.join(" ")
}

/// Remove complete leading bracket/parenthesis groups before filename credits.
pub fn without_leading_bracket_segments(title: &str) -> &str {
    let mut rest = title.trim_start();
    while let Some((0, end)) = delimited_segments(rest).first().copied() {
        rest = rest[end..].trim_start();
    }
    rest
}

fn rfind_ascii_case_insensitive(value: &str, needle: &str) -> Option<usize> {
    value.as_bytes().windows(needle.len()).rposition(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

fn parenthetical_segments(value: &str) -> Vec<String> {
    delimited_segments(value).into_iter().map(|(start, end)| value[start + 1..end - 1].trim()).filter(|value| !value.is_empty()).map(str::to_owned).collect()
}

fn plausible_book_year(value: &str) -> Option<i32> {
    value.split(|character: char| !character.is_ascii_digit()).filter(|part| part.len() == 4).filter_map(|part| part.parse::<i32>().ok()).find(|year| (1450..=2100).contains(year))
}

fn looks_like_person_name(value: &str) -> bool {
    const PARTICLES: &[&str] = &["al", "bin", "da", "de", "del", "den", "der", "di", "du", "la", "le", "van", "von"];
    const NON_NAMES: &[&str] =
        &["abridged", "answers", "solutions", "book", "edition", "guide", "histories", "illustrated", "introduction", "introductions", "manual", "novel", "revised", "series", "textbook", "translation", "unabridged", "volume"];
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
