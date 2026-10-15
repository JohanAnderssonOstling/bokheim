//! Plain-text context recognition extracted from the segment parser.
use crate::char_class::{is_dash, is_opening_bracket, is_opening_enclosure, is_roman_numeral_letter, is_word_joiner};

pub fn has_continuation_evidence(text: &str, abbreviation_end: usize) -> bool {
    let suffix = text.get(abbreviation_end..).unwrap_or("").trim_start_matches(char::is_whitespace);
    let continuation = suffix.trim_start_matches(is_opening_bracket);
    let Some(first) = continuation.chars().next() else { return false; };
    if first.is_lowercase() || first.is_numeric() || is_dash(first) || matches!(first, ',' | ';' | ':' | '/' | '+' | '±' | '=' | '<' | '>' | '§' | '$' | '€' | '£' | '¥') {
        return true;
    }
    let label = continuation.chars().take_while(|character| character.is_alphanumeric()).collect::<String>();
    let label_length = label.chars().count();
    label_length == 1 && first.is_uppercase() || label_length > 1 && label.chars().all(is_roman_numeral_letter)
}

pub fn next_lexical_token(text: &str, offset: usize) -> Option<&str> {
    let suffix = text.get(offset..)?.trim_start_matches(|character: char| character.is_whitespace() || is_opening_enclosure(character));
    let length = suffix.char_indices().take_while(|(_, character)| character.is_alphabetic() || is_word_joiner(*character)).last().map_or(0, |(start, character)| start + character.len_utf8());
    (length > 0).then(|| &suffix[..length])
}

pub fn previous_lexical_token(text: &str, offset: usize) -> Option<&str> {
    text.get(..offset)?
        .split_whitespace()
        .next_back()
        .map(|token| token.trim_matches(|character: char| !character.is_alphanumeric() && !is_word_joiner(character)))
        .filter(|token| !token.is_empty())
}

pub fn is_proper_name_token(token: &str) -> bool {
    token.chars().next().is_some_and(char::is_uppercase)
        && token.chars().filter(|character| character.is_alphabetic()).count() > 1
        && token.chars().all(|character| character.is_alphabetic() || is_word_joiner(character))
}

pub fn is_reference_token(token: &str) -> bool {
    let normalized = token.trim_matches(|character: char| matches!(character, '.' | ',' | ':' | ';' | '(' | ')' | '[' | ']' | '{' | '}'));
    !normalized.is_empty()
        && (normalized.chars().all(char::is_numeric)
            || normalized.chars().count() > 1 && normalized.chars().all(is_roman_numeral_letter))
}

pub fn is_dotted_initialism(text: &str) -> bool {
    let mut chars = text.chars().peekable();
    let mut saw_segment = false;

    while let Some(c) = chars.next() {
        if !c.is_ascii_alphabetic() || c.len_utf8() != 1 {
            return false;
        }
        if chars.next() != Some('.') {
            return false;
        }
        saw_segment = true;
        match chars.peek() {
            Some(next) if next.is_ascii_alphabetic() => continue,
            None => break,
            _ => return false,
        }
    }

    saw_segment && text.ends_with('.') && text.chars().filter(|c| *c == '.').count() >= 2
}
