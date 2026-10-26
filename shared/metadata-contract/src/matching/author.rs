//! Author lookup keys omit titles and biographical dates, never display text.
use super::{bibliographic_match_key, delimited_segments};
use std::borrow::Cow;

struct Word {
    text: String,
    date_label: bool,
    approximate: bool,
    era: bool,
    dotted_honorific: bool,
}

fn honorific(word: &str) -> bool {
    // Keep ambiguous abbreviations such as St. (also a surname component)
    // and Sr. (also a generational suffix). Reviewed aliases handle St. for
    // known saints without changing other authors' names.
    matches!(word, "dr" | "doctor" | "prof" | "professor" | "mr" | "mrs" | "ms" | "miss" | "sir" | "dame" | "rev" | "reverend" | "baron" | "baroness" | "graf" | "capt" | "captain" | "col" | "colonel" | "saint" | "father" | "sister" | "brother" | "rabbi" | "swami" | "mahatma" | "general" | "baronet")
}

fn without_contributor_roles(value: &str) -> Cow<'_, str> {
    let mut result = String::new();
    let mut start = 0;
    for (close, _) in value.match_indices(')') {
        let Some(open) = value[start..close].rfind('(').map(|offset| start + offset) else { continue };
        let role = bibliographic_match_key(&value[open + 1..close]);
        if matches!(role.as_str(), "author" | "translator" | "editor" | "illustrator" | "compiler" | "contributor") {
            result.push_str(&value[start..open]);
            result.push(' ');
            start = close + 1;
        }
    }
    if start == 0 {
        Cow::Borrowed(value)
    } else {
        result.push_str(&value[start..]);
        Cow::Owned(result)
    }
}

/// Lookup variants only for expansions that agree with adjacent printed names
/// and initials. Nicknames, roles and contradictory expansions stay unchanged.
pub(super) fn initial_expansion_variants(value: &str) -> Vec<String> {
    let mut current = value.to_owned();
    for _ in 0..16 {
        let mut replacement = None;
        for (start, end) in delimited_segments(&current) {
            let expanded = &current[start + 1..end - 1];
            let expanded_key = author_name_match_key(expanded);
            let words = expanded_key.split_whitespace().collect::<Vec<_>>();
            if words.is_empty() || words.iter().any(|word| !word.chars().all(char::is_alphabetic)) {
                continue;
            }
            let prefix = &current[..start];
            let mut preceding = Vec::new();
            let mut offset = 0;
            for segment in prefix.split_inclusive(char::is_whitespace) {
                let at = offset + segment.len() - segment.trim_start().len();
                offset += segment.len();
                let raw = segment.trim();
                if raw.is_empty() { continue; }
                let key = author_name_match_key(raw);
                if !key.is_empty() {
                    preceding.push((at, key));
                }
            }
            if preceding.len() < words.len() {
                continue;
            }
            let preceding = &preceding[preceding.len() - words.len()..];
            let agrees = preceding.iter().zip(&words).all(|((_, old), new)| old == new || (old.chars().count() == 1 && new.starts_with(old.as_str())));
            let expands_initial = preceding.iter().zip(&words).any(|((_, old), new)| old.chars().count() == 1 && new.chars().count() > 1);
            if !agrees || !expands_initial {
                continue;
            }
            replacement = Some(format!("{} {} {}", &current[..preceding[0].0], expanded, &current[end..]));
            break;
        }
        let Some(expanded) = replacement else { break };
        current = expanded;
    }
    // Retain the stated full name as the constraint. Keeping an additional
    // initial-only variant would let Henry also match a conflicting Holmes.
    vec![current]
}

fn number(word: &str, max_digits: usize) -> bool {
    !word.is_empty() && word.len() <= max_digits && word.bytes().all(|byte| byte.is_ascii_digit())
}

fn century_number(word: &str) -> bool {
    number(word, 2)
        || matches!(word, "i" | "ii" | "iii" | "iv" | "v" | "vi" | "vii" | "viii" | "ix" | "x" | "xi" | "xii" | "xiii" | "xiv" | "xv" | "xvi" | "xvii" | "xviii" | "xix" | "xx" | "xxi")
        || ["st", "nd", "rd", "th"].iter().any(|suffix| word.strip_suffix(suffix).is_some_and(|value| number(value, 2)))
}

fn date_atom(words: &[Word], start: usize, short_year: bool) -> Option<usize> {
    let first = words.get(start)?;
    if century_number(&first.text) {
        let mut end = start + 1;
        // Hyphenated ranges become adjacent tokens; explicit connectors remain.
        if words.get(end).is_some_and(|word| matches!(word.text.as_str(), "and" | "to")) && words.get(end + 1).is_some_and(|word| century_number(&word.text)) {
            end += 2;
        } else if words.get(end).is_some_and(|word| century_number(&word.text)) {
            end += 1;
        }
        if words.get(end).is_some_and(|word| matches!(word.text.as_str(), "century" | "centuries")) {
            return Some(end + 1);
        }
    }
    (number(&first.text, 4) && (short_year || first.text.len() >= 3)).then_some(start + 1)
}

fn date_expression(words: &[Word], start: usize, labelled: bool) -> Option<usize> {
    let mut cursor = start;
    while words.get(cursor).is_some_and(|word| word.approximate || matches!(word.text.as_str(), "in" | "between" | "before" | "after" | "early" | "mid" | "middle" | "late" | "the")) {
        cursor += 1;
    }
    let mut end = date_atom(words, cursor, labelled)?;
    loop {
        while words.get(end).is_some_and(|word| word.era) {
            end += 1;
        }
        let next = if words.get(end).is_some_and(|word| matches!(word.text.as_str(), "and" | "to")) { end + 1 } else { end };
        if let Some(next_end) = date_atom(words, next, labelled) {
            end = next_end;
        } else {
            return Some(end);
        }
    }
}

/// Honorifics anywhere, dated biographical phrases and adjacent duplicated name
/// parts affect display, not identity. Preserve initials and nonadjacent names.
pub fn author_name_match_key(value: &str) -> String {
    let value = without_contributor_roles(value);
    let words = value
        .split(|character: char| character.is_whitespace() || character == ',')
        .flat_map(|raw| {
            let key = bibliographic_match_key(raw);
            let letters = raw.chars().filter(|character| character.is_alphabetic()).collect::<String>();
            // Uppercase B./D. are initials. Lowercase b./d./fl. introduce dates.
            let date_label = matches!(key.as_str(), "born" | "died" | "active" | "flourished" | "floruit") || matches!(letters.as_str(), "b" | "d" | "fl");
            let approximate = matches!(key.as_str(), "ca" | "circa" | "about" | "approx" | "approximately" | "around") || letters == "c";
            let era = matches!(key.as_str(), "bc" | "bce" | "ad" | "ce" | "b c" | "b c e" | "a d" | "c e");
            // Gen and Bart can be given names; require the printed period
            // when treating these additional abbreviations as titles.
            let dotted_honorific = raw.ends_with('.') && matches!(key.as_str(), "gen" | "lt" | "maj" | "sgt" | "adm" | "brig" | "bt" | "bart" | "fr");
            key.split_whitespace().map(|text| Word { text: text.to_owned(), date_label, approximate, era, dotted_honorific }).collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let mut names = Vec::new();
    let mut cursor = 0;
    while let Some(word) = words.get(cursor) {
        if honorific(&word.text) || word.dotted_honorific {
            cursor += 1;
            continue;
        }
        if let Some(end) = date_expression(&words, cursor + usize::from(word.date_label), word.date_label) {
            cursor = end;
            continue;
        }
        names.push(word.text.clone());
        cursor += 1;
    }
    names.dedup();
    names.join(" ")
}
