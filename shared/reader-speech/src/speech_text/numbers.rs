//! English spoken-number wording; numeric recognition belongs to the parser.
const SMALL_NUMBERS: [&str; 20] = ["zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven", "twelve", "thirteen", "fourteen", "fifteen", "sixteen", "seventeen", "eighteen", "nineteen"];
const SMALL_ORDINALS: [&str; 20] =
    ["zeroth", "first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth", "ninth", "tenth", "eleventh", "twelfth", "thirteenth", "fourteenth", "fifteenth", "sixteenth", "seventeenth", "eighteenth", "nineteenth"];
const TENS: [&str; 10] = ["", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety"];
const ORDINAL_TENS: [&str; 10] = ["", "", "twentieth", "thirtieth", "fortieth", "fiftieth", "sixtieth", "seventieth", "eightieth", "ninetieth"];
const SCALES: [(u32, &str); 4] = [(1_000_000_000, "billion"), (1_000_000, "million"), (1_000, "thousand"), (100, "hundred")];

/// Local notation must occupy a whole numeric token. A terminal period is
/// allowed, but version components, slash forms, and identifiers are not.
pub(super) fn numeric_token_start(text: &str, start: usize) -> bool {
    let mut before = text[..start].chars().rev();
    let previous = before.next();
    if previous.is_some_and(|character| character.is_alphanumeric() || matches!(character, '_' | '.' | '/')) || (previous == Some('-') && before.next().is_some_and(char::is_alphanumeric)) {
        return false;
    }
    true
}

pub(super) fn numeric_token_boundary(text: &str, start: usize, end: usize) -> bool {
    if !numeric_token_start(text, start) {
        return false;
    }
    let mut after = text[end..].chars();
    let next = after.next();
    !next.is_some_and(|character| character.is_alphanumeric() || matches!(character, '_' | '/')) && !(next == Some('.') && after.next().is_some_and(char::is_numeric))
}

pub(super) fn cardinal(value: u32) -> String {
    if value < 20 {
        return SMALL_NUMBERS[value as usize].to_string();
    }
    if value < 100 {
        let tens = TENS[value as usize / 10];
        return if value % 10 == 0 { tens.to_string() } else { format!("{tens}-{}", SMALL_NUMBERS[value as usize % 10]) };
    }
    let (scale, name) = SCALES.into_iter().find(|(scale, _)| value >= *scale).expect("a large number has a scale");
    let head = format!("{} {name}", cardinal(value / scale));
    let remainder = value % scale;
    match remainder {
        0 => head,
        1..=99 => format!("{head} and {}", cardinal(remainder)),
        _ => format!("{head} {}", cardinal(remainder)),
    }
}

/// Spell source digits without floating-point conversion or integer overflow.
/// Leading zeroes and values beyond u64 retain every digit individually.
pub(super) fn integer_words(digits: &str) -> Option<String> {
    if digits.is_empty() || !digits.bytes().all(|digit| digit.is_ascii_digit()) {
        return None;
    }
    if (digits.len() > 1 && digits.starts_with('0')) || digits.parse::<u64>().is_err() {
        return Some(digits.bytes().map(|digit| SMALL_NUMBERS[usize::from(digit - b'0')]).collect::<Vec<_>>().join(" "));
    }
    Some(large_cardinal(digits.parse().ok()?))
}

fn large_cardinal(value: u64) -> String {
    if let Ok(value) = u32::try_from(value) {
        return cardinal(value);
    }
    let (scale, name) =
        [(1_000_000_000_000_000_000, "quintillion"), (1_000_000_000_000_000, "quadrillion"), (1_000_000_000_000, "trillion"), (1_000_000_000, "billion")].into_iter().find(|(scale, _)| value >= *scale).expect("a large integer has a scale");
    let head = format!("{} {name}", large_cardinal(value / scale));
    match value % scale {
        0 => head,
        remainder @ 1..=99 => format!("{head} and {}", large_cardinal(remainder)),
        remainder => format!("{head} {}", large_cardinal(remainder)),
    }
}

pub(super) fn ordinal(value: u32) -> String {
    if value < 20 {
        return SMALL_ORDINALS[value as usize].to_string();
    }
    if value < 100 {
        return if value % 10 == 0 { ORDINAL_TENS[value as usize / 10].to_string() } else { format!("{}-{}", TENS[value as usize / 10], SMALL_ORDINALS[value as usize % 10]) };
    }
    let (scale, name) = SCALES.into_iter().find(|(scale, _)| value >= *scale).expect("a large ordinal has a scale");
    let head = format!("{} {name}", cardinal(value / scale));
    let remainder = value % scale;
    match remainder {
        0 => format!("{head}th"),
        1..=99 => format!("{head} and {}", ordinal(remainder)),
        _ => format!("{head} {}", ordinal(remainder)),
    }
}
