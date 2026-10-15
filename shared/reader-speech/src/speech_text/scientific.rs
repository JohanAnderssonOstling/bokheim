//! Scientific notation requires an explicit exponent marker, never a footnote alone.
use segment_parser::SegmentTree;

use super::decimals::{number_token_end, source_number_phrase};
use super::numbers::{cardinal, numeric_token_boundary, numeric_token_start};
use super::semantics::Replacement;

fn superscript_digit(character: char) -> Option<u32> {
    match character {
        '⁰' => Some(0),
        '¹' => Some(1),
        '²' => Some(2),
        '³' => Some(3),
        '⁴' => Some(4),
        '⁵' => Some(5),
        '⁶' => Some(6),
        '⁷' => Some(7),
        '⁸' => Some(8),
        '⁹' => Some(9),
        _ => None,
    }
}

fn exponent(text: &str, superscript: bool) -> Option<(usize, String)> {
    let first = text.chars().next()?;
    let sign = if superscript { matches!(first, '⁺' | '⁻') } else { matches!(first, '+' | '-' | '−') };
    let sign_end = if sign { first.len_utf8() } else { 0 };
    let negative = matches!(first, '-' | '−' | '⁻');
    let mut value = 0_u32;
    let mut end = sign_end;
    for character in text[sign_end..].chars() {
        let digit = if superscript { superscript_digit(character) } else { character.is_ascii_digit().then(|| u32::from(character as u8 - b'0')) };
        let Some(digit) = digit else { break };
        value = value.checked_mul(10)?.checked_add(digit)?;
        end += character.len_utf8();
    }
    if end == sign_end {
        return None;
    }
    Some((end, format!("{}{}", if negative { "minus " } else { "" }, cardinal(value))))
}

fn scientific_phrase(text: &str) -> Option<(usize, String)> {
    let mantissa_end = number_token_end(text)?;
    let mantissa = &text[..mantissa_end];
    let tail = &text[mantissa_end..];
    let (end, power) = if tail.starts_with(['e', 'E']) {
        let (length, power) = exponent(&tail[1..], false)?;
        (mantissa_end + 1 + length, power)
    } else {
        let marker = tail.trim_start();
        let multiplication = marker.chars().next().filter(|character| matches!(character, '×' | 'x' | 'X' | '*'))?;
        let base = marker[multiplication.len_utf8()..].trim_start().strip_prefix("10")?;
        let power_text = base.trim_start();
        let offset = text.len() - power_text.len();
        if let Some(power_text) = power_text.strip_prefix('^') {
            let power_text = power_text.trim_start();
            let braced = power_text.starts_with('{');
            let digits = if braced { &power_text[1..] } else { power_text };
            let (length, power) = exponent(digits, false)?;
            if braced && !digits[length..].starts_with('}') {
                return None;
            }
            (text.len() - digits.len() + length + usize::from(braced), power)
        } else {
            let (length, power) = exponent(power_text, true)?;
            (offset + length, power)
        }
    };
    let mantissa = source_number_phrase(mantissa, mantissa.contains('.').then_some('.'), None)?;
    Some((end, format!("{mantissa} times ten to the power of {power}")))
}

pub(super) fn scientific_replacements(tree: &SegmentTree) -> Vec<Replacement> {
    let text = tree.text();
    let mut cursor = 0;
    text.char_indices()
        .filter_map(|(start, character)| {
            if start < cursor || (!character.is_ascii_digit() && !matches!(character, '+' | '-' | '−')) || !numeric_token_start(text, start) {
                return None;
            }
            let (length, phrase) = scientific_phrase(&text[start..])?;
            let end = start + length;
            if !numeric_token_boundary(text, start, end) {
                return None;
            }
            cursor = end;
            Some(Replacement::new(start..end, phrase))
        })
        .collect()
}
