//! Clock wording uses parsed time values and explicit prose context.
use segment_parser::{SegmentTimeFactRef, SegmentTree, time_facts};
use segment_rules::abbreviations::previous_lexical_token;

use super::abbreviation_end;
use super::numbers::cardinal;
use super::semantics::Replacement;

fn meridiem(text: &str) -> Option<&'static str> {
    let letters = text.chars().filter(char::is_ascii_alphabetic).flat_map(char::to_lowercase).collect::<String>();
    match letters.as_str() {
        "am" => Some("Ay Em"),
        "pm" => Some("Pee Em"),
        _ => None,
    }
}

fn has_clock_context(tree: &SegmentTree, fact: &SegmentTimeFactRef<'_>, previous: Option<&SegmentTimeFactRef<'_>>, previous_is_clock: bool) -> bool {
    if meridiem(fact.text()).is_some() {
        return true;
    }
    // Three-part colon forms often represent elapsed time or media offsets.
    if fact.time_fact().second().is_some() {
        return false;
    }
    let start = fact.span().start;
    let cue = previous_lexical_token(tree.text(), start).map(str::to_ascii_lowercase);
    match cue.as_deref() {
        Some("at" | "by" | "until" | "till" | "around" | "before" | "after" | "from" | "departure" | "arrival") => true,
        Some("to") if previous_is_clock => previous.is_some_and(|previous| tree.text()[previous.span().end..start].trim().eq_ignore_ascii_case("to")),
        _ => false,
    }
}

fn clock_phrase(hour: u8, minute: u8, second: Option<u8>, marker: Option<&str>) -> String {
    let spoken_hour = if marker.is_some() { u32::from((hour + 11) % 12 + 1) } else { u32::from(hour) };
    let hour_word = cardinal(spoken_hour);
    let clock = match (minute, marker) {
        (0, Some(_)) => hour_word,
        (0, None) if hour == 0 => "midnight".to_string(),
        (0, None) if hour == 12 => "noon".to_string(),
        (0, None) if hour > 12 => format!("{hour_word} hundred"),
        (0, None) => format!("{hour_word} o'clock"),
        (1..=9, _) => format!("{hour_word} oh {}", cardinal(u32::from(minute))),
        (_, _) => format!("{hour_word} {}", cardinal(u32::from(minute))),
    };
    let clock = second.map_or_else(|| clock.clone(), |second| format!("{clock} and {} {}", cardinal(u32::from(second)), if second == 1 { "second" } else { "seconds" }));
    marker.map_or_else(|| clock.clone(), |marker| format!("{clock} {marker}"))
}

pub(super) fn time_replacements(tree: &SegmentTree) -> Vec<Replacement> {
    let facts = time_facts(tree).collect::<Vec<_>>();
    let mut previous_is_clock = false;
    facts
        .iter()
        .enumerate()
        .filter_map(|(index, fact)| {
            let previous = index.checked_sub(1).and_then(|index| facts.get(index));
            let is_clock = has_clock_context(tree, fact, previous, previous_is_clock);
            previous_is_clock = is_clock;
            if !is_clock {
                return None;
            }
            let time = fact.time_fact();
            let mut phrase = clock_phrase(time.hour(), time.minute(), time.second(), meridiem(fact.text()));
            // The last dot in p.m. can also terminate the sentence. Retain it when
            // the existing abbreviation rule sees a sentence boundary.
            if fact.text().ends_with('.') && abbreviation_end(tree.text(), fact.span().end - 1, false) == fact.span().end - 1 {
                phrase.push('.');
            }
            Some(Replacement::new(fact.span(), phrase))
        })
        .collect()
}
