//! Colon ratios require a nearby literal ratio, odds, or proportion label.
use segment_parser::SegmentTree;
use segment_rules::{LexicalKind, lexemes};

use super::decimals::{number_token_end, source_number_phrase};
use super::numbers::{cardinal, numeric_token_boundary};
use super::semantics::Replacement;
use super::words;

fn skip_space(text: &str) -> &str {
    text.trim_start_matches(|character: char| character.is_whitespace() && !matches!(character, '\n' | '\r'))
}

fn ratio_phrase(text: &str, start: usize) -> Option<(usize, String)> {
    let mut cursor = start;
    let mut parts = Vec::new();
    loop {
        if parts.len() >= 16 || !text[cursor..].chars().next()?.is_ascii_digit() {
            return None;
        }
        let length = number_token_end(&text[cursor..])?;
        let source = &text[cursor..cursor + length];
        let phrase = source.parse::<u32>().map(cardinal).ok().or_else(|| source_number_phrase(source, source.contains('.').then_some('.'), None))?;
        parts.push(phrase);
        cursor += length;
        let tail = skip_space(&text[cursor..]);
        if let Some(after_colon) = tail.strip_prefix(':') {
            cursor = text.len() - skip_space(after_colon).len();
        } else {
            break;
        }
    }
    (parts.len() > 1 && numeric_token_boundary(text, start, cursor)).then(|| (cursor, parts.join(" to ")))
}

pub(super) fn ratio_replacements(tree: &SegmentTree) -> impl Iterator<Item = Replacement> + '_ {
    let text = tree.text();
    words(text).filter_map(move |cue| {
        if !matches!(cue.text.to_ascii_lowercase().as_str(), "ratio" | "ratios" | "odds" | "proportion" | "proportions") {
            return None;
        }
        let tail = &text[cue.end..];
        let mut body = skip_space(tail);
        if body.len() == tail.len() && !body.starts_with([':', '=']) {
            return None;
        }
        if let Some(connector) = lexemes(body).next().filter(|lexeme| lexeme.kind() == LexicalKind::Word && matches!(lexeme.text().to_ascii_lowercase().as_str(), "of" | "is" | "was")) {
            let tail = &body[connector.span().end..];
            body = skip_space(tail);
            if body.len() == tail.len() {
                return None;
            }
        }
        if body.starts_with([':', '=']) {
            body = skip_space(&body[1..]);
        }
        let start = text.len() - body.len();
        let (end, phrase) = ratio_phrase(text, start)?;
        Some(Replacement::new(start..end, phrase))
    })
}
