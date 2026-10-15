//! Spoken month names stay local; recognized spellings come from segment-rules.
use segment_rules::months::ENGLISH_MONTHS;
use std::borrow::Cow;

use super::{TextEdits, Word, abbreviation_end, words};

const SPOKEN_MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

pub(super) fn spoken_month(month: u8) -> Option<&'static str> {
    month.checked_sub(1).and_then(|index| SPOKEN_MONTHS.get(usize::from(index))).copied()
}

fn date_number(word: &Word<'_>) -> bool {
    word.text.parse::<u32>().is_ok_and(|number| (1..=31).contains(&number) || (1000..=9999).contains(&number))
}

fn date_context(text: &str, word: &Word<'_>, previous: Option<&Word<'_>>, next: Option<&Word<'_>>) -> bool {
    let previous_number = previous.is_some_and(|previous| date_number(previous) && !text[..previous.start].ends_with('.') && !text[previous.end..word.start].is_empty() && text[previous.end..word.start].chars().all(char::is_whitespace));
    let next_number = next.is_some_and(|next| {
        date_number(next) && !text[next.end..].chars().next().is_some_and(char::is_alphanumeric) && !text[word.end..next.start].is_empty() && text[word.end..next.start].chars().all(|character| character.is_whitespace() || character == '.')
    });
    previous_number || next_number
}

pub(super) fn normalize_months(text: &str) -> Cow<'_, str> {
    if !text.bytes().any(|character| character.is_ascii_digit()) {
        return Cow::Borrowed(text);
    }
    let mut words = words(text).peekable();
    let mut result = TextEdits::new(text);
    let mut previous = None;
    while let Some(word) = words.next() {
        let next = words.peek();
        let period = text[word.end..].starts_with('.') && !text[word.end + 1..].starts_with('.');
        let separated_end = !text[word.end + usize::from(period)..].chars().next().is_some_and(char::is_alphanumeric);
        let capitalized = word.text.chars().next().is_some_and(char::is_uppercase);
        let month = ((period || capitalized) && separated_end && date_context(text, &word, previous.as_ref(), next))
            .then(|| ENGLISH_MONTHS.iter().find_map(|(spelling, month)| spelling.eq_ignore_ascii_case(word.text).then_some(*month)))
            .flatten()
            .map(|month| SPOKEN_MONTHS[month as usize - 1]);
        if let Some(month) = month.filter(|month| !month.eq_ignore_ascii_case(word.text)) {
            let end = if period { abbreviation_end(text, word.end, false) } else { word.end };
            result.replace(word.start..end, month);
        }
        previous = Some(word);
    }
    result.finish()
}
