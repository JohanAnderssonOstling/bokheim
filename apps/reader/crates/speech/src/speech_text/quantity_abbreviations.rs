//! Quantity abbreviations need only a preceding parsed number, not prose inference.
use segment_parser::{NUMBER_VALUE_SCALE, SegmentTree, number_facts};
use segment_rules::abbreviations::ENGLISH_POLICY;
use segment_rules::{LexicalKind, lexemes};

use super::abbreviation_end;
use super::semantics::Replacement;

fn quantity_phrase(word: &str, plural: bool) -> Option<&'static str> {
    let forms = match word.to_ascii_lowercase().as_str() {
        "hr" | "hrs" => ["hour", "hours"],
        "yr" | "yrs" => ["year", "years"],
        "pcs" => ["piece", "pieces"],
        "lb" => ["pound", "pounds"],
        "qt" => ["quart", "quarts"],
        "yd" => ["yard", "yards"],
        "hhd" => ["hogshead", "hogsheads"],
        _ => return None,
    };
    Some(forms[usize::from(plural)])
}

pub(super) fn quantity_abbreviation_replacements(tree: &SegmentTree) -> impl Iterator<Item = Replacement> + '_ {
    number_facts(tree).filter_map(|fact| {
        if fact.number_format_requires_locale_change() || fact.number_values().len() != 1 {
            return None;
        }
        let tail = &tree.text()[fact.span().end..];
        let gap = tail.len() - tail.trim_start().len();
        // Whitespace distinguishes prose quantities from compact identifiers.
        if gap == 0 {
            return None;
        }
        let suffix = &tail[gap..];
        let word = lexemes(suffix).next().filter(|word| word.kind() == LexicalKind::Word)?;
        let following = suffix[word.span().end..].strip_prefix('.')?;
        if following.starts_with('.') || following.chars().next().is_some_and(char::is_alphanumeric) {
            return None;
        }
        let start = fact.span().end + gap;
        let period = start + word.span().end;
        if !ENGLISH_POLICY.recognizes(tree.text(), start..period + 1) {
            return None;
        }
        let value = fact.number_values()[0];
        let plural = value != NUMBER_VALUE_SCALE && value != -NUMBER_VALUE_SCALE;
        let phrase = quantity_phrase(word.text(), plural)?;
        Some(Replacement::new(start..abbreviation_end(tree.text(), period, false), phrase))
    })
}
