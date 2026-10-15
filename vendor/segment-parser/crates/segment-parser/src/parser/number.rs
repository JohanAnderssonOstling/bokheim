use super::*;

pub(super) fn parse_number_word_phrase_at(context: &ParseContext<'_>, start_idx: usize, token: &BoundedToken<'_>) -> Option<Detection> {
    let stream = context.stream();
    let integer = context.profile().number_word_integer(token.text())?;
    let value = to_fixed_point(integer as f64)?;
    context.known_number_values().contains(&value).then(|| {
        let units = UnitRange::new(stream, start_idx..token.end_index());
        let semantic = NumberSemantic::new(value, context.number_format("", None));
        Detection::grounded_semantic(Detected::semantic(units, NodeSemantic::Number(semantic)), PhraseConfidence::High, PhraseAmbiguity::Contextual)
    })
}

pub(super) fn parse_number_phrase_at(context: &ParseContext<'_>, start_idx: usize, _token: &BoundedToken<'_>) -> Option<Detection> {
    let stream = context.stream();
    let (span_start, number_start) = numeric_start_indices(stream, start_idx)?;
    let units = &structure::parser_api::units(stream);
    let first = units.get(number_start)?;
    if first.kind() != UnitKind::Numeric || first.is_tag() {
        return None;
    }

    let run_end_idx = contiguous_slice_end(units, number_start);
    if run_end_idx <= number_start {
        return None;
    }

    if let Some(entity) = parse_date_phrase(context, number_start, run_end_idx) {
        return Some(Detection::grounded_semantic(rebase_phrase(stream, entity, span_start), PhraseConfidence::High, PhraseAmbiguity::Contextual));
    }

    let policy = context.number_policy();
    let raw_number_end_idx = number_span_end(stream, number_start, policy);
    let number_end_idx = trimmed_number_span_end(stream, number_start, raw_number_end_idx, policy);
    let number = ParsedNumber::parse_range(context, span_start, number_end_idx)?;
    if let Some(entity) = parse_measurement_range_or_dimension_phrase(context, &number) {
        return Some(Detection::grounded_semantic(rebase_phrase(stream, entity, span_start), PhraseConfidence::Normal, PhraseAmbiguity::Contextual));
    }
    if let Some(entity) = parse_measurement_phrase(context, &number) {
        return Some(Detection::grounded_semantic(rebase_phrase(stream, entity, span_start), PhraseConfidence::Normal, PhraseAmbiguity::Contextual));
    }
    if raw_number_end_idx < run_end_idx && !is_non_ambiguous_number_boundary(context, raw_number_end_idx) {
        return None;
    }
    let (value, format, units) = number.into_parts();
    let semantic = NumberSemantic::new(value, format);
    Some(Detection::fallback(Detected::semantic(units, NodeSemantic::Number(semantic)), PhraseAmbiguity::Ambiguous))
}

pub(super) fn is_punct_text(stream: &SegmentTree, idx: usize, text: &str) -> bool {
    structure::parser_api::units(stream).get(idx).is_some_and(|unit| unit.kind() == UnitKind::Punct) && structure::parser_api::unit_text(stream, idx) == Some(text)
}

pub(super) fn numeric_start_indices(stream: &SegmentTree, start_idx: usize) -> Option<(usize, usize)> {
    let first = structure::parser_api::units(stream).get(start_idx)?;
    if first.kind() == UnitKind::Numeric {
        return Some((start_idx, start_idx));
    }
    if first.kind() == UnitKind::Punct && is_unary_numeric_sign_unit(stream, start_idx) {
        let next_idx = visible_stream_unit_after(stream, start_idx)?;
        return structure::parser_api::units(stream).get(next_idx).and_then(|next| (next.kind() == UnitKind::Numeric).then_some((start_idx, next_idx)));
    }
    None
}

pub(super) fn is_unary_numeric_sign_unit(stream: &SegmentTree, sign_idx: usize) -> bool {
    let Some(sign_text) = structure::parser_api::unit_text(stream, sign_idx) else {
        return false;
    };
    if !matches!(sign_text, "+" | "-" | "−") {
        return false;
    }
    match visible_stream_unit_before(stream, sign_idx).and_then(|index| structure::parser_api::units(stream).get(index).map(|unit| (index, unit.kind()))) {
        None => true,
        Some((_, UnitKind::Whitespace)) => true,
        Some((index, UnitKind::Punct)) => {
            let Some(prev_text) = structure::parser_api::unit_text(stream, index) else {
                return false;
            };
            matches!(
                prev_text,
                "(" | "[" | "{" | "<" | "“" | "‘" | "«" | "「" | "‹" | ")" | "]" | "}" | ">" | "”" | "’" | "»" | "」" | "›" | ","
                    | "." | ":" | ";" | "!"
            )
        }
        Some(_) => false,
    }
}

pub(super) fn rebase_phrase(stream: &SegmentTree, detected: Detected, span_start: usize) -> Detected { detected.with_units_start(stream, span_start) }

pub(super) fn number_span_end(stream: &SegmentTree, start_idx: usize, policy: NumberParsingPolicy) -> usize {
    let mut index = start_idx;
    let mut end_idx = start_idx;
    while index < structure::parser_api::units(stream).len() {
        let unit = &structure::parser_api::units(stream)[index];
        if unit.is_tag() {
            index += 1;
            continue;
        }
        if unit.kind() == UnitKind::Numeric {
            index += 1;
            end_idx = index;
            continue;
        }
        if unit.kind() == UnitKind::Punct {
            let text = structure::parser_api::unit_text(stream, index).unwrap_or_default();
            if number_separator(text, policy) {
                index += 1;
                end_idx = index;
                continue;
            }
        } else if unit.kind() == UnitKind::Whitespace {
            if is_grouping_whitespace_continuation(stream, index, policy) {
                index += 1;
                end_idx = index;
                continue;
            }
        }
        break;
    }
    end_idx
}

pub(super) fn trimmed_number_span_end(stream: &SegmentTree, start_idx: usize, mut end_idx: usize, policy: NumberParsingPolicy) -> usize {
    while end_idx > start_idx {
        let idx = end_idx - 1;
        let unit = &structure::parser_api::units(stream)[idx];
        let text = structure::parser_api::unit_text(stream, idx).unwrap_or_default();
        if unit.kind() != UnitKind::Punct || !number_separator(text, policy) {
            break;
        }
        end_idx -= 1;
    }
    end_idx
}

pub(super) fn is_non_ambiguous_number_boundary(context: &ParseContext<'_>, boundary_idx: usize) -> bool {
    let stream = context.stream();
    let policy = context.number_policy();
    if stream.unit(boundary_idx).and_then(|unit| unit.tag()).is_some_and(|tag| tag.kind() == structure::TagKind::Standalone) {
        return true;
    }
    let boundary_idx = skip_tag_stream_units(stream, boundary_idx);
    structure::parser_api::units(stream).get(boundary_idx).is_some_and(|unit| {
        let text = structure::parser_api::unit_text(stream, boundary_idx).unwrap_or_default();
        match unit.kind() {
            UnitKind::Whitespace => {
                is_grouping_whitespace_continuation(stream, boundary_idx, policy)
            }
            UnitKind::Punct => {
                (text.chars().count() == 1 && !number_separator(text, policy))
                    || is_context_close_unit(context, text)
            }
            // The letter "x"/"X" glued directly onto digits ("600x400") is the
            // ASCII dimension separator — same role as "×" (already covered
            // above as ordinary single-char punctuation), just spelled with a
            // letter instead of a symbol.
            UnitKind::Word => matches!(text, "x" | "X") || capitalized_word_starts_after_number(text),
            _ => false,
        }
    })
}

fn capitalized_word_starts_after_number(text: &str) -> bool {
    let mut characters = text.chars();
    characters.next().is_some_and(char::is_uppercase) && characters.any(char::is_lowercase)
}

pub(super) fn number_separator(text: &str, policy: NumberParsingPolicy) -> bool {
    let mut chars = text.chars();
    let Some(first) = chars.next() else { return false; };
    if chars.next().is_some() {
        return false;
    }
    policy.decimal_separators().contains(&first) || policy.grouping_separators().contains(&first)
}

pub(super) fn is_grouping_whitespace(text: &str, grouping_separators: &[char]) -> bool {
    let Some(ch) = text.chars().next() else { return false; };
    if text.chars().count() != 1 {
        return false;
    }
    grouping_separators.contains(&ch) && ch.is_whitespace()
}

pub(super) fn is_grouping_whitespace_continuation(stream: &SegmentTree, whitespace_idx: usize, policy: NumberParsingPolicy) -> bool {
    let text = structure::parser_api::unit_text(stream, whitespace_idx).unwrap_or_default();
    let next_idx = skip_tag_stream_units(stream, whitespace_idx + 1);
    let next_end = visible_unit_run_end(stream, next_idx, UnitKind::Numeric);
    is_grouping_whitespace(text, policy.grouping_separators())
        && structure::parser_api::units(stream).get(next_idx).is_some_and(|unit| unit.kind() == UnitKind::Numeric)
        && unit_range_text(stream, next_idx, next_end).is_some_and(|next| next.len() == 3 && ascii_digits(next))
}
/// Fixed-point scale for normalized numeric annotations.
pub(crate) const NORMALIZED_VALUE_SCALE: f64 = 10_000_000_000.0;

pub(super) fn parse_fixed_number(text: &str, policy: NumberParsingPolicy) -> Option<(i128, Option<char>)> {
    let negative = text.starts_with(['-', '−']);
    let trimmed = trim_ascii_number_suffix(trim_number_sign_prefix(text));
    let (magnitude, decimal_separator) = if is_preferred_grouped_integer(trimmed, policy) {
        let digits = trimmed.chars().filter(char::is_ascii_digit).collect::<String>();
        (digits.parse::<f64>().ok()?, None)
    } else if let Some(decimal) = policy
        .decimal_separators()
        .iter()
        .copied()
        .find(|decimal| trimmed.contains(*decimal) && number_rejection_reason_for_decimal(trimmed, *decimal, policy.grouping_separators()).is_none())
    {
        let mut parts = trimmed.splitn(2, decimal);
        let int_part = parts.next()?.chars().filter(char::is_ascii_digit).collect::<String>();
        let frac_part = parts.next()?;
        let int_part = if int_part.is_empty() { "0" } else { &int_part };
        (format!("{int_part}.{frac_part}").parse::<f64>().ok()?, Some(decimal))
    } else {
        number_rejection_reason_for_integer(trimmed, policy.grouping_separators()).is_none().then_some(())?;
        let digits = trimmed.chars().filter(char::is_ascii_digit).collect::<String>();
        (digits.parse::<f64>().ok()?, None)
    };
    let value = if negative { -magnitude } else { magnitude };
    Some((to_fixed_point(value)?, decimal_separator))
}

pub(super) fn observed_number_format(text: &str, decimal_separator: Option<char>, policy: NumberParsingPolicy) -> NumberFormat {
    let numeric = trim_ascii_number_suffix(trim_number_sign_prefix(text));
    let grouping_separator = observed_grouping_separator(numeric, decimal_separator, policy.grouping_separators());
    let locale_decimal_separator = policy.decimal_separators()[0];
    let locale_grouping_separator = policy.canonical_grouping_separator();
    let fraction_digits = decimal_separator.and_then(|separator| numeric.rsplit_once(separator)).map(|(_, fraction)| fraction.chars().take(u8::MAX as usize).count() as u8);
    let assessment = assess_number_format(decimal_separator, grouping_separator, locale_decimal_separator, locale_grouping_separator);
    NumberFormat::builder()
        .decimal_separator(decimal_separator)
        .grouping_separator(grouping_separator)
        .locale_decimal_separator(Some(locale_decimal_separator))
        .locale_grouping_separator(Some(locale_grouping_separator))
        .fraction_digits(fraction_digits)
        .assessment(assessment)
        .build()
}

fn observed_grouping_separator(text: &str, decimal_separator: Option<char>, grouping_separators: &[char]) -> Option<char> {
    let integer = decimal_separator.and_then(|separator| text.rsplit_once(separator).map(|(integer, _)| integer)).unwrap_or(text);
    grouping_separators.iter().copied().find(|separator| Some(*separator) != decimal_separator && integer.contains(*separator))
}

fn assess_number_format(decimal_separator: Option<char>, grouping_separator: Option<char>, locale_decimal_separator: char, locale_grouping_separator: char) -> NumberFormatAssessment {
    if decimal_separator.is_some_and(|separator| separator != locale_decimal_separator) {
        return NumberFormatAssessment::Foreign;
    }
    match grouping_separator {
        Some(separator) if separator == locale_grouping_separator => NumberFormatAssessment::Canonical,
        Some('\u{202F}') if locale_grouping_separator == '\u{00A0}' => NumberFormatAssessment::Accepted,
        Some('’') if locale_grouping_separator == '\'' => NumberFormatAssessment::Accepted,
        Some(_) => NumberFormatAssessment::Foreign,
        None => NumberFormatAssessment::Canonical,
    }
}

pub(super) fn to_fixed_point(value: f64) -> Option<i128> {
    let fixed_point = (value * NORMALIZED_VALUE_SCALE).round();
    (fixed_point.is_finite() && fixed_point >= i128::MIN as f64 && fixed_point < i128::MAX as f64).then(|| fixed_point as i128)
}

fn is_preferred_grouped_integer(text: &str, policy: NumberParsingPolicy) -> bool {
    policy.preferred_grouping_separators().iter().any(|separator| text.contains(*separator))
        && valid_integer_grouping(text, policy.grouping_separators())
}

pub(super) fn number_rejection_reason_for_integer(text: &str, grouping_separators: &[char]) -> Option<&'static str> {
    if !valid_integer_grouping(text, grouping_separators) {
        return Some("invalid grouping separators");
    }
    if text.bytes().all(|b| b.is_ascii_digit()) && text.len() > 1 && text.starts_with('0') {
        return Some("leading zero in whole number");
    }
    None
}

pub(super) fn number_rejection_reason_for_decimal(text: &str, decimal: char, grouping_separators: &[char]) -> Option<&'static str> {
    let mut parts = text.split(decimal);
    let Some(int_part) = parts.next() else { return Some("empty integer component"); };
    let frac_part = parts.next();
    if parts.next().is_some() {
        return Some("multiple decimal separators");
    }
    if !valid_integer_grouping(int_part, grouping_separators) {
        return Some("invalid grouping separators");
    }
    if frac_part.is_none()
        && int_part.len() > 1
        && int_part.starts_with('0')
        && ascii_digits(int_part)
    {
        return Some("leading zero in whole number");
    }
    if let Some(frac_part) = frac_part
        && (frac_part.is_empty() || !ascii_digits(frac_part))
    {
        return Some("fractional part must be digits");
    }
    None
}

pub(super) fn valid_integer_grouping(int_part: &str, grouping_separators: &[char]) -> bool {
    if int_part.is_empty() {
        return false;
    }
    let separator = {
        let mut separator = None;
        for candidate in grouping_separators {
            if !int_part.contains(*candidate) {
                continue;
            }
            match separator {
                Some(sep) if sep != *candidate => return false,
                Some(_) => (),
                None => separator = Some(*candidate),
            }
        }
        separator
    };
    let Some(thousands) = separator else {
        return ascii_digits(int_part);
    };

    let mut groups = int_part.split(thousands);
    let Some(first) = groups.next() else {
        return false;
    };
    if first.is_empty() || first.len() > 3 || !ascii_digits(first) {
        return false;
    }

    groups.all(|group| group.len() == 3 && ascii_digits(group))
}
pub(super) fn trim_ascii_number_suffix(text: &str) -> &str {
    crate::stream_util::trim_trailing_punctuation(text)
}

pub(super) fn is_trim_punct(b: u8) -> bool { crate::stream_util::is_trim_punctuation(char::from(b)) }

pub(super) fn trim_number_sign_prefix(text: &str) -> &str { text.strip_prefix(['+', '-', '−']).unwrap_or(text) }

#[cfg(test)]
mod tests;
