//! Calendar wording requires named months or explicit temporal prose cues.
use std::ops::Range;

use segment_parser::{SegmentTree, currency_facts, measurement_facts, number_facts};
use segment_rules::months::ENGLISH_MONTHS;

use super::months::spoken_month;
use super::numbers::{cardinal, numeric_token_boundary, ordinal};
use super::semantics::Replacement;
use super::{Word, abbreviation_end, words};

const DECADES: [&str; 10] = ["hundreds", "tens", "twenties", "thirties", "forties", "fifties", "sixties", "seventies", "eighties", "nineties"];

pub(super) fn year_phrase(year: u32) -> String {
    if year == 1000 {
        return "one thousand".into();
    }
    if year == 2000 {
        return "two thousand".into();
    }
    if (2001..=2009).contains(&year) {
        return format!("two thousand and {}", cardinal(year - 2000));
    }
    if !(1000..=2999).contains(&year) {
        return cardinal(year);
    }
    let century = cardinal(year / 100);
    match year % 100 {
        0 => format!("{century} hundred"),
        remainder @ 1..=9 => format!("{century} oh {}", cardinal(remainder)),
        remainder => format!("{century} {}", cardinal(remainder)),
    }
}

fn horizontal_gap(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|character| character.is_whitespace() && !matches!(character, '\n' | '\r'))
}

fn previous_word<'a>(text: &str, lexical: &'a [Word<'a>], start: usize) -> Option<&'a Word<'a>> {
    let index = lexical.partition_point(|word| word.end <= start).checked_sub(1)?;
    let word = &lexical[index];
    horizontal_gap(&text[word.end..start]).then_some(word)
}

fn temporal_context(text: &str, lexical: &[Word<'_>], span: Range<usize>) -> bool {
    let cue = previous_word(text, lexical, span.start).map(|word| word.text.to_ascii_lowercase());
    if !cue.as_deref().is_some_and(|cue| matches!(cue, "in" | "during" | "since" | "until" | "before" | "after" | "around" | "circa" | "year" | "born" | "died" | "from")) {
        return false;
    }
    // A following noun can instead count objects ("in 1905 houses").
    // Punctuation, a completed phrase, or ordinary clause starters support a year.
    let tail = text[span.end..].trim_start();
    match tail.chars().next() {
        None | Some(',' | '.' | ';' | '?' | '!' | ')' | ']' | '}' | '"' | '”' | '’') => true,
        Some(character) if character.is_alphabetic() => {
            let next = lexical.get(lexical.partition_point(|word| word.start < span.end));
            next.is_some_and(|word| {
                matches!(word.text.to_ascii_lowercase().as_str(), "he" | "she" | "it" | "they" | "we" | "i" | "the" | "a" | "an" | "this" | "that" | "there" | "when" | "while" | "was" | "were" | "is" | "began" | "ended")
            })
        }
        _ => false,
    }
}

fn month_number(word: &Word<'_>) -> Option<u8> {
    ENGLISH_MONTHS.iter().find_map(|(spelling, month)| spelling.trim_end_matches('.').eq_ignore_ascii_case(word.text).then_some(*month as u8))
}

fn day_at(text: &str, start: usize) -> Option<(u32, usize)> {
    let digits = text[start..].bytes().take_while(u8::is_ascii_digit).count();
    if !(1..=2).contains(&digits) {
        return None;
    }
    let day = text[start..start + digits].parse::<u32>().ok().filter(|day| (1..=31).contains(day))?;
    let mut end = start + digits;
    let suffix = match (day % 100, day % 10) {
        (11..=13, _) => "th",
        (_, 1) => "st",
        (_, 2) => "nd",
        (_, 3) => "rd",
        _ => "th",
    };
    if text.get(end..end + 2).is_some_and(|candidate| candidate.eq_ignore_ascii_case(suffix)) {
        end += 2;
    }
    if text[end..].chars().next().is_some_and(char::is_alphanumeric) {
        return None;
    }
    Some((day, end))
}

fn valid_day(month: u8, day: u32) -> bool {
    let maximum = match month {
        2 => 29,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    day <= maximum
}

fn partial_dates(text: &str, lexical: &[Word<'_>], quantities: &[Range<usize>]) -> Vec<Replacement> {
    let mut replacements = Vec::new();
    for (index, month_word) in lexical.iter().enumerate() {
        let Some(month) = month_number(month_word) else { continue };
        let name = spoken_month(month).expect("catalog month");
        let capitalized = month_word.text.chars().next().is_some_and(char::is_uppercase);
        let abbreviated = !month_word.text.eq_ignore_ascii_case(name);
        let dotted = abbreviated && text[month_word.end..].starts_with('.') && !text[month_word.end..].starts_with("..");
        if !capitalized && (abbreviated && !dotted || month == 5) {
            continue;
        }
        let after_month = month_word.end + usize::from(dotted);
        let next = lexical.get(index + 1);
        let month_first = next.and_then(|next| {
            let gap = &text[after_month..next.start];
            if !(horizontal_gap(gap) || dotted && gap.is_empty()) {
                return None;
            }
            let (day, end) = day_at(text, next.start)?;
            Some((day, next.start..end, month_word.start..end))
        });
        let day_first = index
            .checked_sub(1)
            .and_then(|previous_index| {
                let previous = lexical.get(previous_index)?;
                if matches!(previous.text.to_ascii_lowercase().as_str(), "st" | "nd" | "rd" | "th") { lexical.get(previous_index.checked_sub(1)?).filter(|number| number.end == previous.start) } else { Some(previous) }
            })
            .and_then(|previous| {
                let (day, end) = day_at(text, previous.start)?;
                if !horizontal_gap(&text[end..month_word.start]) {
                    return None;
                }
                let month_end = if dotted { abbreviation_end(text, month_word.end, false) } else { month_word.end };
                Some((day, previous.start..end, previous.start..month_end))
            });
        let Some((day, numeric, span)) = month_first.or(day_first) else { continue };
        if !valid_day(month, day) || !numeric_token_boundary(text, span.start, span.end) || quantities.iter().any(|quantity| quantity.start < numeric.end && numeric.start < quantity.end) {
            continue;
        }
        // Day ranges and clocks need separate semantics. An isolated May can
        // also be a verb; require a date cue or a clear phrase boundary for it.
        let next = text[span.end..].trim_start().chars().next();
        if next.is_some_and(|character| matches!(character, ':' | '-' | '–' | '—' | '/')) {
            continue;
        }
        if month == 5 && next.is_some_and(char::is_alphabetic) {
            let cue = previous_word(text, lexical, span.start).map(|word| word.text.to_ascii_lowercase());
            if !cue.as_deref().is_some_and(|cue| matches!(cue, "on" | "by" | "before" | "after" | "since" | "until")) {
                continue;
            }
        }
        replacements.push(Replacement::new(span, format!("{name} {}", ordinal(day))));
    }
    replacements
}

pub(super) fn calendar_replacements(tree: &SegmentTree) -> Vec<Replacement> {
    let text = tree.text();
    let lexical = words(text).collect::<Vec<_>>();
    let quantities = measurement_facts(tree).map(|fact| fact.span()).chain(currency_facts(tree).map(|fact| fact.span())).collect::<Vec<_>>();
    let years = number_facts(tree).filter_map(|fact| {
        if fact.text().len() != 4 || !fact.text().bytes().all(|byte| byte.is_ascii_digit()) || !numeric_token_boundary(text, fact.span().start, fact.span().end) || !temporal_context(text, &lexical, fact.span()) {
            return None;
        }
        let year = fact.text().parse::<u32>().ok().filter(|year| (1000..=2999).contains(year))?;
        Some(Replacement::new(fact.span(), year_phrase(year)))
    });
    let decades = lexical.iter().enumerate().filter_map(|(index, word)| {
        if word.text.len() != 4 || !word.text.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        let suffix = lexical.get(index + 1).filter(|suffix| suffix.start == word.end && suffix.text == "s")?;
        if !numeric_token_boundary(text, word.start, suffix.end) {
            return None;
        }
        let cue = previous_word(text, &lexical, word.start)?.text.to_ascii_lowercase();
        // "in 1990s" can be a duration in seconds; the definite article or
        // a decade modifier provides stronger calendar evidence.
        if !matches!(cue.as_str(), "the" | "early" | "late" | "mid") {
            return None;
        }
        let next = lexical.get(index + 2).map(|word| word.text.to_ascii_lowercase());
        if next.as_deref().is_some_and(|word| matches!(word, "duration" | "delay" | "interval" | "timeout" | "exposure" | "cooldown" | "countdown" | "timer" | "wait" | "sleep" | "latency")) {
            return None;
        }
        let year = word.text.parse::<u32>().ok().filter(|year| (1000..=2990).contains(year) && year % 10 == 0)?;
        let phrase = if year == 2000 { "two thousands".into() } else { format!("{} {}", cardinal(year / 100), DECADES[(year % 100 / 10) as usize]) };
        Some(Replacement::new(word.start..suffix.end, phrase))
    });
    years.chain(decades).chain(partial_dates(text, &lexical, &quantities)).collect()
}
