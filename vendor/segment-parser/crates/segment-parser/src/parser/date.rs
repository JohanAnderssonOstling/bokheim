use super::*;
use crate::language_parser::month_number_for_profile;

pub(super) fn parse_date_phrase(context: &ParseContext<'_>, start_idx: usize, end_idx: usize) -> Option<Detected> {
    let stream = context.stream();
    let candidate = BoundedToken::from_range(stream, start_idx..end_idx)?.trimmed()?;
    let end_idx = candidate.end_index();
    let text = candidate.text();
    let separator = text.chars().find(|character| matches!(character, '-' | '/' | '.'))?;
    if text.chars().any(|character| matches!(character, '-' | '/' | '.') && character != separator) {
        return None;
    }
    let parts: Vec<&str> = text.split(separator).collect();
    if parts.len() != 3 || !parts.iter().all(|part| ascii_digits(part)) {
        return None;
    }
    if !parts.iter().any(|part| part.len() >= 4 || part.parse::<u32>().ok().is_some_and(|n| n >= 1000)) {
        return None;
    }

    let a = parts[0].parse::<u32>().ok()?;
    let b = parts[1].parse::<u32>().ok()?;
    let c = parts[2].parse::<u32>().ok()?;
    let preferred = match context.locale().date_field_order() {
        DateFieldOrder::MonthDayYear => [(c, a, b), (a, b, c), (c, b, a)],
        DateFieldOrder::DayMonthYear => [(c, b, a), (a, b, c), (c, a, b)],
    };
    let (year, month, day) = preferred.into_iter().find(|date| valid_date(*date))?;

    date_detected(stream, start_idx..end_idx, year, month, day)
}

pub(super) fn parse_textual_date_phrase_at(context: &ParseContext<'_>, start_idx: usize, _token: &BoundedToken<'_>) -> Option<Detection> {
    let phrase = match structure::parser_api::units(context.stream()).get(start_idx)?.kind() {
        UnitKind::Numeric => parse_day_month_year_phrase_at(context, start_idx),
        UnitKind::Word => parse_month_day_year_phrase_at(context, start_idx),
        _ => None,
    }?;
    Some(Detection::grounded_semantic(phrase, PhraseConfidence::High, PhraseAmbiguity::Contextual))
}

fn parse_day_month_year_phrase_at(context: &ParseContext<'_>, start_idx: usize) -> Option<Detected> {
    let stream = context.stream();
    let (day_end, day) = numeric_unit_value_with_end(stream, start_idx)?;
    let after_day = skip_tag_stream_units(stream, day_end);
    let after_day = if is_punct_text(stream, after_day, ".") { after_day + 1 } else { after_day };
    let month_idx = skip_layout_stream_units(stream, after_day);
    let month_end = month_name_end(stream, month_idx);
    let month_text = unit_range_text(stream, month_idx, month_end)?;
    let month = month_number_for_profile(context.profile(), month_text)?;
    let year_idx = skip_layout_stream_units(stream, month_end);
    let (year_end, year) = numeric_unit_value_with_end(stream, year_idx)?;
    valid_date((year, month, day)).then(|| date_detected(stream, start_idx..year_end, year, month, day)).flatten()
}

fn parse_month_day_year_phrase_at(context: &ParseContext<'_>, start_idx: usize) -> Option<Detected> {
    let stream = context.stream();
    let month_end = month_name_end(stream, start_idx);
    let month_text = unit_range_text(stream, start_idx, month_end)?;
    let month = month_number_for_profile(context.profile(), month_text)?;
    let day_idx = skip_layout_stream_units(stream, month_end);
    let (day_end, day) = numeric_unit_value_with_end(stream, day_idx)?;
    let after_day = skip_tag_stream_units(stream, day_end);
    let after_day = if is_punct_text(stream, after_day, ",") { after_day + 1 } else { after_day };
    let year_idx = skip_layout_stream_units(stream, after_day);
    let (year_end, year) = numeric_unit_value_with_end(stream, year_idx)?;
    valid_date((year, month, day)).then(|| date_detected(stream, start_idx..year_end, year, month, day)).flatten()
}

fn date_detected(stream: &SegmentTree, units: Range<usize>, year: u32, month: u32, day: u32) -> Option<Detected> {
    let fact = DateFact::new(year, u8::try_from(month).ok()?, u8::try_from(day).ok()?);
    Some(Detected::semantic(UnitRange::new(stream, units), NodeSemantic::Date(fact)))
}

fn month_name_end(stream: &SegmentTree, start_idx: usize) -> usize {
    let mut cursor = start_idx;
    let mut end_idx = start_idx;
    while structure::parser_api::units(stream).get(cursor).is_some_and(|unit| !unit.is_tag() && (unit.kind() == UnitKind::Word || unit.kind() == UnitKind::Punct && structure::parser_api::unit_text(stream, cursor) == Some("."))) {
        end_idx = cursor + 1;
        cursor = skip_tag_stream_units(stream, end_idx);
    }
    end_idx
}

fn valid_date((year, month, day): (u32, u32, u32)) -> bool {
    year >= 1000 && (1..=12).contains(&month) && (1..=days_in_month(year, month)).contains(&day)
}

fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn leap_year(year: u32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}
