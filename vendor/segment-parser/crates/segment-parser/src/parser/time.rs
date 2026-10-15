use super::*;

pub(super) fn parse_time_phrase_at(context: &ParseContext<'_>, start_idx: usize, _token: &BoundedToken<'_>) -> Option<Detection> {
    if !allows_boundary_start(context, start_idx) {
        return None;
    }
    let detected = parse_meridiem_time_phrase_at(context, start_idx).or_else(|| parse_colon_time_phrase_at(context, start_idx))?;
    BoundaryPolicy::Time.allows_end(context, detected.units_end()).then(|| Detection::grounded_semantic(detected, PhraseConfidence::High, PhraseAmbiguity::Contextual))
}

fn parse_colon_time_phrase_at(context: &ParseContext<'_>, start_idx: usize) -> Option<Detected> {
    let stream = context.stream();
    let (_, hour) = numeric_unit_value_with_end(stream, start_idx)?;
    if hour > 23 { return None; }
    let separators = context.time_format().separators();
    let (end_idx, minute, second) = colon_time_components(stream, start_idx, separators)?;
    let unit_end = parse_time_suffix(context, end_idx).unwrap_or(end_idx);
    time_detected(stream, start_idx..unit_end, hour, minute, second)
}

fn parse_meridiem_time_phrase_at(context: &ParseContext<'_>, start_idx: usize) -> Option<Detected> {
    let stream = context.stream();
    if context.profile().meridiems.is_empty() {
        return None;
    }
    let (hour_end, hour) = numeric_unit_value_with_end(stream, start_idx)?;
    if !(1..=12).contains(&hour) {
        return None;
    }
    let separator_idx = skip_tag_stream_units(stream, hour_end);
    let (time_end, minute, second) = if is_time_separator(stream, separator_idx, context.time_format().separators()) {
        let detected = colon_time_components(stream, start_idx, context.time_format().separators())?;
        (detected.0, detected.1, detected.2)
    } else {
        (hour_end, 0, None)
    };
    let meridiem_start = skip_layout_stream_units(stream, time_end);
    let meridiem_end = contiguous_slice_end(structure::parser_api::units(stream), meridiem_start);
    let candidate = BoundedToken::from_range(stream, meridiem_start..meridiem_end)?.trimmed_with(|_, trailing| matches!(trailing, "," | ";" | ":" | "!" | "?"))?;
    let trimmed_end = candidate.end_index();
    let meridiem = candidate.text();
    let hour = normalized_meridiem_hour(hour, meridiem, context.profile().meridiems)?;
    let unit_end = parse_time_suffix(context, trimmed_end).unwrap_or(trimmed_end);
    time_detected(stream, start_idx..unit_end, hour, minute, second)
}

fn colon_time_components(stream: &SegmentTree, start_idx: usize, separators: &[char]) -> Option<(usize, u32, Option<u32>)> {
    let (hour_end, _) = numeric_unit_value_with_end(stream, start_idx)?;
    let first_separator = skip_tag_stream_units(stream, hour_end);
    if !is_time_separator(stream, first_separator, separators) { return None; }
    let minute_idx = visible_stream_unit_after(stream, first_separator)?;
    let (minute_end, minute) = numeric_unit_value_with_end(stream, minute_idx)?;
    if minute > 59 || !unit_text_len_between(stream, minute_idx, 2, 2) { return None; }
    let second_separator = skip_tag_stream_units(stream, minute_end);
    if !is_time_separator(stream, second_separator, separators) { return Some((minute_end, minute, None)); }
    let second_idx = visible_stream_unit_after(stream, second_separator)?;
    let (second_end, second) = numeric_unit_value_with_end(stream, second_idx)?;
    (second <= 59 && unit_text_len_between(stream, second_idx, 2, 2)).then_some((second_end, minute, Some(second)))
}

fn parse_time_suffix(context: &ParseContext<'_>, end_idx: usize) -> Option<usize> {
    let suffixes = context.time_format().suffixes();
    if suffixes.is_empty() { return None; }
    let stream = context.stream();
    let suffix_start = skip_layout_stream_units(stream, end_idx);
    let suffix_end = visible_unit_run_end(stream, suffix_start, UnitKind::Word);
    unit_range_text(stream, suffix_start, suffix_end).filter(|text| context.time_format().allows_suffix(text)).map(|_| suffix_end)
}

fn is_time_separator(stream: &SegmentTree, index: usize, separators: &[char]) -> bool {
    structure::parser_api::units(stream).get(index).is_some_and(|unit| unit.kind() == UnitKind::Punct)
        && structure::parser_api::unit_text(stream, index).is_some_and(|text| text.chars().count() == 1 && text.chars().next().is_some_and(|separator| separators.contains(&separator)))
}

fn time_detected(stream: &SegmentTree, units: Range<usize>, hour: u32, minute: u32, second: Option<u32>) -> Option<Detected> {
    let fact = TimeFact::new(u8::try_from(hour).ok()?, u8::try_from(minute).ok()?, second.map(u8::try_from).transpose().ok()?);
    Some(Detected::semantic(UnitRange::new(stream, units), NodeSemantic::Time(fact)))
}

fn unit_text_len_between(stream: &SegmentTree, idx: usize, min: usize, max: usize) -> bool {
    let end = visible_unit_run_end(stream, idx, UnitKind::Numeric);
    unit_range_text(stream, idx, end).is_some_and(|text| (min..=max).contains(&text.len()))
}

fn normalized_meridiem_hour(hour: u32, meridiem: &str, lexemes: &[crate::language_parser::MeridiemLexeme]) -> Option<u32> {
    let pm = lexemes.iter().find(|lexeme| meridiem.eq_ignore_ascii_case(lexeme.text)).map(|lexeme| lexeme.pm)?;
    Some(match (hour, pm) {
        (12, false) => 0,
        (12, true) => 12,
        (_, true) => hour + 12,
        _ => hour,
    })
}
