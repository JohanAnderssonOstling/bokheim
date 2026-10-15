//! Spoken currencies, unambiguous dates, and English ordinals from parser facts.
use std::ops::Range;

use segment_parser::{SegmentTree, currency_facts, date_facts, number_facts, ordinal_facts};

use super::calendar::year_phrase;
use super::decimals::{number_token_end, source_number_phrase};
use super::months::spoken_month;
use super::numbers::{numeric_token_boundary, numeric_token_start, ordinal};
use super::semantics::Replacement;
use super::words;

struct CurrencyWords {
    major: [&'static str; 2],
    minor: Option<[&'static str; 2]>,
    minor_alone: Option<[&'static str; 2]>,
}

impl CurrencyWords {
    fn new(major: [&'static str; 2], minor: Option<[&'static str; 2]>, minor_alone: Option<[&'static str; 2]>) -> Self {
        Self { major, minor, minor_alone }
    }
}

fn currency_words(code: &str, text: &str) -> Option<CurrencyWords> {
    let explicit_code = words(text).any(|word| word.text.eq_ignore_ascii_case(code));
    match code {
        "USD" if explicit_code => Some(CurrencyWords::new(["United States dollar", "United States dollars"], Some(["cent", "cents"]), Some(["United States cent", "United States cents"]))),
        "USD" | "$" if text.contains('$') => Some(CurrencyWords::new(["dollar", "dollars"], Some(["cent", "cents"]), Some(["cent", "cents"]))),
        "¢" => Some(CurrencyWords::new(["cent", "cents"], None, None)),
        "EUR" | "€" if explicit_code || text.contains('€') || words(text).any(|word| matches!(word.text.to_ascii_lowercase().as_str(), "euro" | "euros")) => {
            Some(CurrencyWords::new(["euro", "euros"], Some(["cent", "cents"]), Some(["euro cent", "euro cents"])))
        }
        "GBP" | "£" if explicit_code || text.contains('£') => Some(CurrencyWords::new(["pound", "pounds"], Some(["penny", "pence"]), Some(["penny", "pence"]))),
        "JPY" if explicit_code || words(text).any(|word| word.text.eq_ignore_ascii_case("yen")) => Some(CurrencyWords::new(["yen", "yen"], None, None)),
        "CAD" if explicit_code => Some(CurrencyWords::new(["Canadian dollar", "Canadian dollars"], Some(["cent", "cents"]), Some(["Canadian cent", "Canadian cents"]))),
        "AUD" if explicit_code => Some(CurrencyWords::new(["Australian dollar", "Australian dollars"], Some(["cent", "cents"]), Some(["Australian cent", "Australian cents"]))),
        "NZD" if explicit_code => Some(CurrencyWords::new(["New Zealand dollar", "New Zealand dollars"], Some(["cent", "cents"]), Some(["New Zealand cent", "New Zealand cents"]))),
        "CHF" if explicit_code => Some(CurrencyWords::new(["Swiss franc", "Swiss francs"], Some(["centime", "centimes"]), Some(["Swiss centime", "Swiss centimes"]))),
        "SEK" if explicit_code => Some(CurrencyWords::new(["Swedish krona", "Swedish kronor"], Some(["öre", "öre"]), Some(["Swedish öre", "Swedish öre"]))),
        "NOK" if explicit_code => Some(CurrencyWords::new(["Norwegian krone", "Norwegian kroner"], Some(["øre", "øre"]), Some(["Norwegian øre", "Norwegian øre"]))),
        "DKK" if explicit_code => Some(CurrencyWords::new(["Danish krone", "Danish kroner"], Some(["øre", "øre"]), Some(["Danish øre", "Danish øre"]))),
        "CNY" if explicit_code => Some(CurrencyWords::new(["Chinese yuan", "Chinese yuan"], Some(["fen", "fen"]), Some(["Chinese fen", "Chinese fen"]))),
        "INR" if explicit_code => Some(CurrencyWords::new(["Indian rupee", "Indian rupees"], Some(["paisa", "paise"]), Some(["Indian paisa", "Indian paise"]))),
        _ => None,
    }
}

// Render the source digits exactly using the parser's recognized decimal
// separator. Its fixed-point values use floating-point conversion internally;
// reading source digits avoids rounding a price during speech normalization.
fn currency_amount(text: &str, decimal: Option<char>, fractional: bool) -> Option<(String, u8)> {
    let numeric = text.chars().filter(|character| character.is_ascii_digit() || Some(*character) == decimal).collect::<String>();
    let (major, fraction) = decimal.and_then(|decimal| numeric.split_once(decimal)).unwrap_or((&numeric, ""));
    let major = major.trim_start_matches('0');
    let major = if major.is_empty() { "0" } else { major };
    let fraction = fraction.trim_end_matches('0');
    if !major.bytes().all(|byte| byte.is_ascii_digit()) || !fraction.bytes().all(|byte| byte.is_ascii_digit()) || (!fractional && !fraction.is_empty()) || fraction.len() > 2 {
        return None;
    }
    let minor = match fraction.len() {
        0 => 0,
        1 => fraction.parse::<u8>().ok()? * 10,
        _ => fraction.parse::<u8>().ok()?,
    };
    Some((major.to_string(), minor))
}

fn currency_phrase(code: &str, evidence: &str, text: &str, decimal: Option<char>, negative: bool) -> Option<String> {
    let vocabulary = currency_words(code, evidence)?;
    let (major, minor) = currency_amount(text, decimal, vocabulary.minor.is_some())?;
    let minor_words = if major == "0" && minor > 0 { vocabulary.minor_alone } else { vocabulary.minor };
    let minor_phrase = minor_words.filter(|_| minor > 0).map(|words| format!("{minor} {}", words[usize::from(minor != 1)]));
    let phrase = match (major.as_str(), minor_phrase) {
        ("0", Some(minor)) => minor,
        (_, Some(minor)) => format!("{major} {} and {minor}", vocabulary.major[usize::from(major != "1")]),
        (_, None) => format!("{major} {}", vocabulary.major[usize::from(major != "1")]),
    };
    let sign = if negative || text.contains(['-', '−']) {
        "minus "
    } else if text.contains('+') {
        "plus "
    } else {
        ""
    };
    Some(format!("{sign}{phrase}"))
}

fn preceding_currency_code(text: &str, start: usize) -> Option<(&str, usize)> {
    let prefix = text[..start].trim_end();
    if prefix.len() == start {
        return None;
    }
    let code_start = prefix.len().checked_sub(3)?;
    let code = prefix.get(code_start..)?;
    if !code.bytes().all(|byte| byte.is_ascii_uppercase()) || prefix[..code_start].chars().next_back().is_some_and(|character| character.is_alphanumeric() || matches!(character, '_' | '.' | '/')) {
        return None;
    }
    currency_words(code, code).map(|_| (code, code_start))
}

fn following_currency_code(text: &str, end: usize) -> Option<(&str, usize)> {
    let tail = &text[end..];
    let suffix = tail.trim_start();
    if suffix.len() == tail.len() {
        return None;
    }
    let code = suffix.get(..3)?;
    if !code.bytes().all(|byte| byte.is_ascii_uppercase()) || suffix[3..].chars().next().is_some_and(|character| character.is_alphanumeric() || matches!(character, '_' | '/')) {
        return None;
    }
    if suffix[3..].strip_prefix('.').is_some_and(|tail| tail.chars().next().is_some_and(char::is_alphanumeric)) {
        return None;
    }
    currency_words(code, code).map(|_| (code, text.len() - suffix.len() + 3))
}

fn explicit_currency_span(text: &str, span: Range<usize>) -> Option<(&str, Range<usize>)> {
    match (preceding_currency_code(text, span.start), following_currency_code(text, span.end)) {
        (Some((prefix, start)), Some((suffix, end))) if prefix == suffix => Some((prefix, start..end)),
        (Some(_), Some(_)) => None,
        (Some((code, start)), None) => Some((code, start..span.end)),
        (None, Some((code, end))) => Some((code, span.start..end)),
        _ => None,
    }
}

// Only compact amounts with an explicit currency symbol qualify. Bare `60k`
// and suffixes attached to longer words or identifiers remain untouched.
fn compact_currency_replacements(text: &str) -> impl Iterator<Item = Replacement> + '_ {
    text.char_indices().filter_map(|(index, symbol)| {
        let currency = match symbol {
            '$' => "dollars",
            '€' => "euros",
            '£' => "pounds",
            _ => return None,
        };
        let mut start = index;
        let mut sign = "";
        if let Some(character @ ('-' | '−' | '+')) = text[..index].chars().next_back() {
            start -= character.len_utf8();
            sign = if character == '+' { "plus " } else { "minus " };
        }
        if !numeric_token_start(text, start) {
            return None;
        }
        let number_start = index + symbol.len_utf8();
        if !text[number_start..].starts_with(|character: char| character.is_ascii_digit()) {
            return None;
        }
        let number_end = number_start + number_token_end(&text[number_start..])?;
        let suffix = text[number_end..].chars().next()?;
        let scale = match suffix {
            'k' | 'K' => "thousand",
            'm' | 'M' => "million",
            'b' | 'B' => "billion",
            _ => return None,
        };
        let end = number_end + suffix.len_utf8();
        if !numeric_token_boundary(text, start, end) {
            return None;
        }
        let number = &text[number_start..number_end];
        let number = source_number_phrase(number, number.contains('.').then_some('.'), None)?;
        Some(Replacement::new(start..end, format!("{sign}{number} {scale} {currency}")))
    })
}

pub(super) fn currency_replacements(tree: &SegmentTree) -> impl Iterator<Item = Replacement> + '_ {
    let currencies = currency_facts(tree).filter_map(|fact| {
        if fact.number_format_requires_locale_change() || !numeric_token_boundary(tree.text(), fact.span().start, fact.span().end) {
            return None;
        }
        let currency = fact.currency_fact();
        let phrase = currency_phrase(currency.currency()?, fact.text(), fact.text(), currency.format().decimal_separator(), currency.amount() < 0)?;
        Some(Replacement::new(fact.span(), phrase))
    });
    // Additional explicit codes use the existing number facts. This local
    // speech vocabulary does not extend RustTrados's currency recognition.
    let explicit = number_facts(tree).filter_map(|fact| {
        if fact.number_format_requires_locale_change() || fact.number_values().len() != 1 {
            return None;
        }
        let (code, span) = explicit_currency_span(tree.text(), fact.span())?;
        let phrase = currency_phrase(code, code, fact.text(), fact.decimal_separator(), false)?;
        Some(Replacement::new(span, phrase))
    });
    currencies.chain(explicit).chain(compact_currency_replacements(tree.text()))
}

fn unambiguous_date(text: &str, year: u32, month: u8, day: u8) -> bool {
    // The parser has already validated the date. A spelled-out month makes its
    // order explicit; purely numeric forms need evidence independent of locale.
    if text.chars().any(char::is_alphabetic) {
        return true;
    }
    let parts = text.split(['-', '/', '.']).map(str::parse::<u32>).collect::<Result<Vec<_>, _>>();
    let Ok(parts) = parts else {
        return false;
    };
    let [first, second, third] = parts.as_slice() else {
        return false;
    };
    (*first == year && *second == u32::from(month) && *third == u32::from(day))
        || (*third == year && ((*first > 12 && *first == u32::from(day) && *second == u32::from(month)) || (*second > 12 && *second == u32::from(day) && *first == u32::from(month))))
}

pub(super) fn date_replacements(tree: &SegmentTree) -> impl Iterator<Item = Replacement> + '_ {
    date_facts(tree).filter_map(|fact| {
        let date = fact.date_fact();
        if !unambiguous_date(fact.text(), date.year(), date.month(), date.day()) {
            return None;
        }
        let month = spoken_month(date.month())?;
        Some(Replacement::new(fact.span(), format!("{month} {}, {}", ordinal(u32::from(date.day())), year_phrase(date.year()))))
    })
}

pub(super) fn ordinal_replacements(tree: &SegmentTree) -> impl Iterator<Item = Replacement> + '_ {
    ordinal_facts(tree)
        .filter(|fact| {
            let text = fact.text().to_ascii_lowercase();
            ["st", "nd", "rd", "th"].iter().any(|suffix| text.ends_with(suffix))
        })
        .map(|fact| Replacement::new(fact.span(), ordinal(fact.value())))
}
