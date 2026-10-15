use crate::language_parser::{language_text_eq, supported_language_profile, DateFieldOrder, LanguageProfile, ParseLocale};
use std::ops::Range;
use structure::{DateFact, TimeFact};

#[derive(Clone, Copy)]
enum DateComponent {
    Year,
    Month,
    Day,
}

#[derive(Clone, Copy)]
enum MonthStyle {
    Full,
    Abbreviated,
    AbbreviatedWithPeriod,
}

struct MonthSurface {
    span: Range<usize>,
    text: String,
    style: MonthStyle,
}

struct MeridiemSurface {
    span: Range<usize>,
    text: String,
}

/// Renders `value` in the target language while retaining the recognized
/// numeric/textual layout, month-name style, casing, and zero-padding of
/// `target_surface`. The result is returned only when it parses back to the
/// same semantic date.
pub fn localized_date_text(value: DateFact, target_language: &str, target_surface: &str) -> Option<String> {
    let profile = supported_language_profile(target_language)?;
    let locale = ParseLocale::from_bcp47(target_language);
    let previous = parsed_date(target_surface, target_language)?;
    let rendered = match month_surface(target_surface, profile, previous.month()) {
        Some(month) => render_textual_date(value, previous, target_surface, profile, month),
        None => render_numeric_date(value, previous, target_surface, locale.date_field_order()),
    }?;
    (parsed_date(&rendered, target_language) == Some(value)).then_some(rendered)
}

/// Renders `value` in the target language while retaining the recognized
/// 24-hour or meridiem layout, separators, casing, and zero-padding of
/// `target_surface`. The result is returned only when it parses back to the
/// same semantic time.
pub fn localized_time_text(value: TimeFact, target_language: &str, target_surface: &str) -> Option<String> {
    let profile = supported_language_profile(target_language)?;
    let previous = parsed_time(target_surface, target_language)?;
    let rendered = match meridiem_surface(target_surface, profile, previous) {
        Some(meridiem) => render_meridiem_time(value, target_surface, profile, meridiem),
        None => render_colon_time(value, target_surface),
    }?;
    (parsed_time(&rendered, target_language) == Some(value)).then_some(rendered)
}

fn parsed_date(text: &str, language: &str) -> Option<DateFact> {
    let mut tree = crate::tokenized_text_tree(text);
    crate::parse(&mut tree, language);
    let facts = crate::date_facts(&tree).filter_map(|fact| (fact.span() == (0..text.len())).then(|| fact.date_fact())).collect::<Vec<_>>();
    let [fact] = facts.as_slice() else { return None };
    Some(*fact)
}

fn parsed_time(text: &str, language: &str) -> Option<TimeFact> {
    let mut tree = crate::tokenized_text_tree(text);
    crate::parse(&mut tree, language);
    let facts = crate::time_facts(&tree).filter_map(|fact| (fact.span() == (0..text.len())).then(|| fact.time_fact())).collect::<Vec<_>>();
    let [fact] = facts.as_slice() else { return None };
    Some(*fact)
}

fn render_numeric_date(value: DateFact, previous: DateFact, surface: &str, order: DateFieldOrder) -> Option<String> {
    let ranges = ascii_digit_ranges(surface);
    let component_order = numeric_date_component_order(surface, &ranges, previous, order)?;
    let replacements = ranges.into_iter().zip(component_order).map(|(range, component)| {
        let component_value = date_component_value(value, component);
        let width = range.len();
        (range, format!("{component_value:0width$}"))
    });
    replace_ranges(surface, replacements)
}

fn render_textual_date(value: DateFact, previous: DateFact, surface: &str, profile: &LanguageProfile, month: MonthSurface) -> Option<String> {
    let ranges = ascii_digit_ranges(surface);
    let year = unique_digit_range(surface, &ranges, previous.year(), &[])?;
    let day = unique_digit_range(surface, &ranges, u32::from(previous.day()), std::slice::from_ref(&year))?;
    let target_month = preferred_month_variant(profile, value.month(), month.style)?;
    let month_text = apply_case_pattern(&month.text, target_month);
    let replacements = [
        (year.clone(), number_preserving_explicit_padding(value.year(), surface, &year)),
        (day.clone(), number_preserving_explicit_padding(u32::from(value.day()), surface, &day)),
        (month.span, month_text),
    ];
    replace_ranges(surface, replacements)
}

fn numeric_date_component_order(surface: &str, ranges: &[Range<usize>], previous: DateFact, order: DateFieldOrder) -> Option<[DateComponent; 3]> {
    let [first, second, third] = ranges else { return None };
    let parsed = [first, second, third].map(|range| surface.get((*range).clone()).and_then(|text| text.parse::<u32>().ok())).into_iter().collect::<Option<Vec<_>>>()?;
    date_component_orders(order).into_iter().find(|candidate| candidate.iter().zip(&parsed).all(|(component, parsed)| date_component_value(previous, *component) == *parsed))
}

fn date_component_orders(order: DateFieldOrder) -> [[DateComponent; 3]; 3] {
    match order {
        DateFieldOrder::MonthDayYear => [
            [DateComponent::Month, DateComponent::Day, DateComponent::Year],
            [DateComponent::Year, DateComponent::Month, DateComponent::Day],
            [DateComponent::Day, DateComponent::Month, DateComponent::Year],
        ],
        DateFieldOrder::DayMonthYear => [
            [DateComponent::Day, DateComponent::Month, DateComponent::Year],
            [DateComponent::Year, DateComponent::Month, DateComponent::Day],
            [DateComponent::Month, DateComponent::Day, DateComponent::Year],
        ],
    }
}

fn date_component_value(value: DateFact, component: DateComponent) -> u32 {
    match component {
        DateComponent::Year => value.year(),
        DateComponent::Month => u32::from(value.month()),
        DateComponent::Day => u32::from(value.day()),
    }
}

fn month_surface(surface: &str, profile: &LanguageProfile, month: u8) -> Option<MonthSurface> {
    profile.months.iter().filter(|(_, number)| *number == u32::from(month)).enumerate().filter_map(|(variant, (candidate, _))| {
        unique_text_span(surface, candidate).map(|span| {
            let style = if candidate.ends_with('.') { MonthStyle::AbbreviatedWithPeriod } else if variant == 0 { MonthStyle::Full } else { MonthStyle::Abbreviated };
            MonthSurface { span, text: candidate_surface_text(surface, candidate).unwrap_or(candidate).to_string(), style }
        })
    }).max_by_key(|month| month.span.len())
}

fn preferred_month_variant(profile: &LanguageProfile, month: u8, style: MonthStyle) -> Option<&'static str> {
    let variants = profile.months.iter().filter(|(_, number)| *number == u32::from(month)).map(|(text, _)| *text).collect::<Vec<_>>();
    match style {
        MonthStyle::Full => variants.first().copied(),
        MonthStyle::Abbreviated => variants.iter().copied().skip(1).find(|text| !text.ends_with('.')).or_else(|| variants.first().copied()),
        MonthStyle::AbbreviatedWithPeriod => variants.iter().copied().find(|text| text.ends_with('.')).or_else(|| variants.first().copied()),
    }
}

fn candidate_surface_text<'a>(surface: &'a str, candidate: &str) -> Option<&'a str> { unique_text_span(surface, candidate).and_then(|span| surface.get(span)) }

fn render_colon_time(value: TimeFact, surface: &str) -> Option<String> {
    if !surface.contains(':') { return None; }
    let ranges = ascii_digit_ranges(surface);
    let values = time_components(value, false);
    if ranges.len() != values.len() { return None; }
    replace_ranges(surface, ranges.into_iter().zip(values).map(|(range, value)| {
        let width = range.len();
        (range, format!("{value:0width$}"))
    }))
}

fn render_meridiem_time(value: TimeFact, surface: &str, profile: &LanguageProfile, meridiem: MeridiemSurface) -> Option<String> {
    let ranges = ascii_digit_ranges(surface);
    let values = meridiem_time_components(value, ranges.len())?;
    let target_meridiem = preferred_meridiem(profile, value.hour() >= 12, &meridiem.text)?;
    let replacements = ranges.into_iter().zip(values).map(|(range, value)| {
        let width = range.len();
        (range, format!("{value:0width$}"))
    }).chain(std::iter::once((meridiem.span, apply_case_pattern(&meridiem.text, target_meridiem))));
    replace_ranges(surface, replacements)
}

fn time_components(value: TimeFact, meridiem: bool) -> Vec<u8> {
    let hour = if meridiem { match value.hour() % 12 { 0 => 12, hour => hour } } else { value.hour() };
    [Some(hour), Some(value.minute()), value.second()].into_iter().flatten().collect()
}

fn meridiem_time_components(value: TimeFact, component_count: usize) -> Option<Vec<u8>> {
    let values = time_components(value, true);
    match component_count {
        1 if value.minute() == 0 && value.second().is_none() => Some(values.into_iter().take(1).collect()),
        2 if value.second().is_none() => Some(values),
        3 if value.second().is_some() => Some(values),
        _ => None,
    }
}

fn meridiem_surface(surface: &str, profile: &LanguageProfile, previous: TimeFact) -> Option<MeridiemSurface> {
    profile.meridiems.iter().filter(|meridiem| meridiem.pm == (previous.hour() >= 12)).filter_map(|meridiem| unique_text_span(surface, meridiem.text).map(|span| MeridiemSurface { text: surface.get(span.clone()).unwrap_or(meridiem.text).to_string(), span })).max_by_key(|meridiem| meridiem.span.len())
}

fn preferred_meridiem(profile: &LanguageProfile, pm: bool, exemplar: &str) -> Option<&'static str> {
    let dotted = exemplar.contains('.');
    profile.meridiems.iter().filter(|meridiem| meridiem.pm == pm).find(|meridiem| meridiem.text.contains('.') == dotted).or_else(|| profile.meridiems.iter().find(|meridiem| meridiem.pm == pm)).map(|meridiem| meridiem.text)
}

fn unique_digit_range(surface: &str, ranges: &[Range<usize>], value: u32, excluded: &[Range<usize>]) -> Option<Range<usize>> {
    let matching = ranges.iter().filter(|range| !excluded.contains(range) && surface.get((*range).clone()).and_then(|text| text.parse::<u32>().ok()) == Some(value)).collect::<Vec<_>>();
    let [range] = matching.as_slice() else { return None };
    Some((*range).clone())
}

fn ascii_digit_ranges(text: &str) -> Vec<Range<usize>> {
    text.char_indices().filter(|(_, character)| character.is_ascii_digit()).fold(Vec::<Range<usize>>::new(), |mut ranges, (index, character)| {
        if let Some(last) = ranges.last_mut() && last.end == index {
            last.end += character.len_utf8();
        } else {
            ranges.push(index..index + character.len_utf8());
        }
        ranges
    })
}

fn unique_text_span(surface: &str, candidate: &str) -> Option<Range<usize>> {
    let matching = surface.char_indices().filter_map(|(start, _)| {
        let end = start.checked_add(candidate.len())?;
        let text = surface.get(start..end)?;
        (language_text_eq(text, candidate) && text_boundary(surface, start, end)).then_some(start..end)
    }).collect::<Vec<_>>();
    let [span] = matching.as_slice() else { return None };
    Some(span.clone())
}

fn text_boundary(surface: &str, start: usize, end: usize) -> bool {
    !surface.get(..start).and_then(|text| text.chars().next_back()).is_some_and(char::is_alphanumeric)
        && !surface.get(end..).and_then(|text| text.chars().next()).is_some_and(char::is_alphanumeric)
}

fn apply_case_pattern(exemplar: &str, text: &str) -> String {
    let letters = exemplar.chars().filter(|character| character.is_alphabetic()).collect::<Vec<_>>();
    if !letters.is_empty() && letters.iter().all(|character| character.is_uppercase()) { return text.to_uppercase(); }
    if letters.first().is_some_and(|character| character.is_uppercase()) && letters.iter().skip(1).all(|character| character.is_lowercase()) {
        let mut characters = text.chars();
        let Some(first) = characters.next() else { return String::new(); };
        return first.to_uppercase().chain(characters).collect();
    }
    text.to_lowercase()
}

fn number_preserving_explicit_padding(value: u32, surface: &str, range: &Range<usize>) -> String {
    let Some(text) = surface.get(range.clone()) else { return value.to_string(); };
    if text.len() > 1 && text.starts_with('0') {
        let width = text.len();
        return format!("{value:0width$}");
    }
    value.to_string()
}

fn replace_ranges(text: &str, replacements: impl IntoIterator<Item = (Range<usize>, String)>) -> Option<String> {
    let mut replacements = replacements.into_iter().collect::<Vec<_>>();
    replacements.sort_by_key(|(range, _)| range.start);
    if replacements.iter().any(|(range, _)| range.start > range.end || range.end > text.len() || !text.is_char_boundary(range.start) || !text.is_char_boundary(range.end)) || replacements.windows(2).any(|pair| pair[0].0.end > pair[1].0.start) { return None; }
    let mut copied = 0;
    let mut output = String::new();
    replacements.into_iter().for_each(|(range, replacement)| {
        output.push_str(&text[copied..range.start]);
        output.push_str(&replacement);
        copied = range.end;
    });
    output.push_str(&text[copied..]);
    Some(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn localizes_changed_textual_month_using_target_language_and_layout() {
        let localized = localized_date_text(DateFact::new(2027, 7, 1), "sv-SE", "30 juni 2026");
        assert_eq!(localized.as_deref(), Some("1 juli 2027"));
    }

    #[test]
    fn preserves_abbreviated_uppercase_month_style() {
        let localized = localized_date_text(DateFact::new(2027, 7, 1), "en-US", "JUN. 03, 2026");
        assert_eq!(localized.as_deref(), Some("JUL. 01, 2027"));
    }

    #[test]
    fn preserves_numeric_date_order_separators_and_padding() {
        let localized = localized_date_text(DateFact::new(2027, 7, 1), "en-US", "06/30/2026");
        assert_eq!(localized.as_deref(), Some("07/01/2027"));
    }

    #[test]
    fn localizes_meridiem_value_and_preserves_spelling() {
        let localized = localized_time_text(TimeFact::new(18, 0, None), "en-US", "5 p.m.");
        assert_eq!(localized.as_deref(), Some("6 p.m."));
    }

    #[test]
    fn localizes_meridiem_time_with_minutes() {
        let localized = localized_time_text(TimeFact::new(18, 45, None), "en-US", "5:30 PM");
        assert_eq!(localized.as_deref(), Some("6:45 PM"));
    }

    #[test]
    fn preserves_twenty_four_hour_time_layout() {
        let localized = localized_time_text(TimeFact::new(6, 5, Some(9)), "sv-SE", "17:30:15");
        assert_eq!(localized.as_deref(), Some("06:05:09"));
    }
}
