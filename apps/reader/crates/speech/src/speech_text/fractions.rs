//! Unicode fractions and narrowly recognized typed fractions with units.
use segment_parser::{ParseLanguage, SegmentTree, measurement_facts, parse_text_with_language};

use super::numbers::{cardinal, numeric_token_boundary, numeric_token_start};
use super::semantics::{Replacement, unit_phrase};

fn unit_candidate(text: &str, end: usize) -> Option<(&str, usize)> {
    let body = text[end..].trim_start_matches(|character: char| character.is_whitespace() && !matches!(character, '\n' | '\r'));
    let length =
        body.char_indices().find(|(_, character)| character.is_whitespace() || matches!(character, '.' | ',' | ';' | ':' | '!' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '"' | '\'' | '“' | '”')).map_or(body.len(), |(index, _)| index);
    if length == 0 || length > 64 {
        return None;
    }
    let candidate = &body[..length];
    // These spellings are also common articles or prepositions in prose.
    if matches!(candidate, "a" | "d" | "in") {
        return None;
    }
    Some((candidate, text.len() - body.len() + length))
}

fn unit_after(text: &str, end: usize, plural: bool) -> Option<(String, usize)> {
    let (candidate, end) = unit_candidate(text, end)?;
    // Probe only recognition with an ordinary number. The actual fractional
    // value is never converted or rounded, and its original unit is retained.
    let tree = parse_text_with_language(format!("2 {candidate}"), ParseLanguage::English);
    let fact = measurement_facts(&tree).find(|fact| fact.measurement_unit_span() == (2..tree.text().len()) && fact.measurement_fact().number_values().len() == 1)?;
    let phrase = unit_phrase(fact.measurement_fact().unit(), plural)?;
    Some((phrase, end))
}

fn typed_unit_after(text: &str, end: usize, plural: bool) -> Option<(String, usize)> {
    if !text[end..].starts_with([' ', '\u{00a0}', '\u{202f}']) {
        return None;
    }
    unit_after(text, end, plural).or_else(|| {
        let (candidate, end) = unit_candidate(text, end)?;
        let forms = match candidate {
            "cup" | "cups" => ["cup", "cups"],
            "teaspoon" | "teaspoons" | "tsp" => ["teaspoon", "teaspoons"],
            "tablespoon" | "tablespoons" | "tbsp" => ["tablespoon", "tablespoons"],
            _ => return None,
        };
        Some((forms[usize::from(plural)].to_string(), end))
    })
}

fn unit_article(unit: &str) -> &'static str {
    if unit.starts_with("hour") || unit.starts_with(['a', 'e', 'i', 'o']) { "an" } else { "a" }
}

fn fraction_words(character: char) -> Option<(&'static str, &'static str)> {
    Some(match character {
        '¼' => ("one quarter", "a quarter"),
        '½' => ("one half", "a half"),
        '¾' => ("three quarters", "three quarters"),
        '⅐' => ("one seventh", "one seventh"),
        '⅑' => ("one ninth", "one ninth"),
        '⅒' => ("one tenth", "one tenth"),
        '⅓' => ("one third", "one third"),
        '⅔' => ("two thirds", "two thirds"),
        '⅕' => ("one fifth", "one fifth"),
        '⅖' => ("two fifths", "two fifths"),
        '⅗' => ("three fifths", "three fifths"),
        '⅘' => ("four fifths", "four fifths"),
        '⅙' => ("one sixth", "one sixth"),
        '⅚' => ("five sixths", "five sixths"),
        '⅛' => ("one eighth", "one eighth"),
        '⅜' => ("three eighths", "three eighths"),
        '⅝' => ("five eighths", "five eighths"),
        '⅞' => ("seven eighths", "seven eighths"),
        '↉' => ("zero thirds", "zero thirds"),
        _ => return None,
    })
}

pub(super) fn fraction_replacements(tree: &SegmentTree) -> impl Iterator<Item = Replacement> + '_ {
    let text = tree.text();
    let unicode = text.char_indices().filter_map(move |(index, character)| fraction_replacement(text, index, index + character.len_utf8(), character, false));
    let typed = text.char_indices().filter_map(move |(index, character)| {
        if !character.is_ascii_digit() || text[..index].chars().next_back().is_some_and(|previous| previous.is_ascii_digit()) {
            return None;
        }
        let numerator_end = index + text[index..].bytes().take_while(u8::is_ascii_digit).count();
        let denominator_start = numerator_end + 1;
        if !text[numerator_end..].starts_with('/') {
            return None;
        }
        let end = denominator_start + text[denominator_start..].bytes().take_while(u8::is_ascii_digit).count();
        // Common proper fractions only: no zero padding, dates, arbitrary
        // ratios, or division with spaces around the slash.
        let glyph = match &text[index..end] {
            "1/2" => '½',
            "1/3" => '⅓',
            "2/3" => '⅔',
            "1/4" => '¼',
            "3/4" => '¾',
            "1/5" => '⅕',
            "2/5" => '⅖',
            "3/5" => '⅗',
            "4/5" => '⅘',
            "1/6" => '⅙',
            "5/6" => '⅚',
            "1/7" => '⅐',
            "1/8" => '⅛',
            "3/8" => '⅜',
            "5/8" => '⅝',
            "7/8" => '⅞',
            "1/9" => '⅑',
            "1/10" => '⅒',
            _ => return None,
        };
        fraction_replacement(text, index, end, glyph, true)
    });
    unicode.chain(typed)
}

fn fraction_replacement(text: &str, index: usize, end: usize, character: char, typed: bool) -> Option<Replacement> {
    let (fraction, mixed_fraction) = fraction_words(character)?;
    let mut number_end = index;
    // A mixed number can be joined, or separated by one ordinary or
    // nonbreaking space. Do not merge across paragraphs or table tabs.
    if let Some(space @ (' ' | '\u{00a0}' | '\u{202f}')) = text[..index].chars().next_back() {
        number_end -= space.len_utf8();
    }
    let mut start = number_end;
    while start > 0 && text.as_bytes()[start - 1].is_ascii_digit() {
        start -= 1;
    }
    let whole = (start < number_end).then(|| &text[start..number_end]);
    if whole.is_none() {
        start = index;
    }
    let mut sign = "";
    if let Some(character @ ('-' | '−' | '+')) = text[..start].chars().next_back() {
        let signed_start = start - character.len_utf8();
        if numeric_token_start(text, signed_start) {
            start = signed_start;
            sign = if character == '+' { "plus " } else { "minus " };
        }
    }
    let mixed = whole.is_some_and(|whole| whole.bytes().any(|byte| byte != b'0'));
    let unit = if typed { Some(typed_unit_after(text, end, mixed)?) } else { unit_after(text, end, mixed) };
    let final_end = unit.as_ref().map_or(end, |(_, end)| *end);
    if !numeric_token_boundary(text, start, final_end) {
        return None;
    }
    let phrase = match (whole, unit) {
        (_, Some((unit, _))) if !mixed => {
            let article = unit_article(&unit);
            match character {
                '½' => format!("{sign}half {article} {unit}"),
                '¼' => format!("{sign}a quarter of {article} {unit}"),
                _ => format!("{sign}{fraction} of {article} {unit}"),
            }
        }
        (Some(whole), Some((unit, _))) => {
            let whole = whole.parse::<u32>().map(cardinal).unwrap_or_else(|_| whole.to_string());
            format!("{sign}{whole} and {mixed_fraction} {unit}")
        }
        (Some(whole), None) => {
            let whole = whole.parse::<u32>().map(cardinal).unwrap_or_else(|_| whole.to_string());
            format!("{sign}{whole} and {mixed_fraction}")
        }
        (None, _) => format!("{sign}{fraction}"),
    };
    Some(Replacement::new(start..final_end, phrase))
}
