use super::*;

#[derive(Clone, Copy)]
enum NumericPairKind {
    Range,
    Dimension,
}

impl NumericPairKind {
    fn separator_end(self, context: &ParseContext<'_>, index: usize) -> Option<usize> {
        match self {
            Self::Range => range_separator_end(context, index),
            Self::Dimension => is_dimension_separator(context.stream(), index).then_some(index + 1),
        }
    }
}

struct NumericPair<'a> {
    first: &'a ParsedNumber,
    separator_idx: usize,
    second: ParsedNumber,
}

impl<'a> NumericPair<'a> {
    fn new(first: &'a ParsedNumber, separator_idx: usize, second: ParsedNumber) -> Self { Self { first, separator_idx, second } }
    fn start_idx(&self) -> usize { self.first.units().indices().start }
    fn first_end_idx(&self) -> usize { self.first.units().end_index() }
    fn separator_idx(&self) -> usize { self.separator_idx }
    fn second_start_idx(&self) -> usize { self.second.units().indices().start }
    fn second_end_idx(&self) -> usize { self.second.units().end_index() }
    fn first(&self) -> &ParsedNumber { self.first }
    fn second(&self) -> &ParsedNumber { &self.second }
    fn into_numbers(self) -> Vec<ParsedNumber> { vec![self.first.clone(), self.second] }
}

pub(super) fn parse_measurement_range_or_dimension_phrase(context: &ParseContext<'_>, first: &ParsedNumber) -> Option<Detected> {
    parse_repeated_measurement_range_phrase(context, first)
        .or_else(|| parse_measurement_range_phrase(context, first))
        .or_else(|| parse_dimension_phrase(context, first))
}

pub(super) fn parse_measurement_phrase(context: &ParseContext<'_>, number: &ParsedNumber) -> Option<Detected> {
    let units = number.units().indices();
    let unit = MeasurementUnitPhrase::parse_after_number(context, units.end)?;
    resolve_measurement_phrase(context, units.start..unit.units().end_index(), vec![number.clone()], unit, None)
}

fn parse_measurement_range_phrase(context: &ParseContext<'_>, first: &ParsedNumber) -> Option<Detected> {
    let stream = context.stream();
    let pair = parse_numeric_pair(context, first, NumericPairKind::Range)?;
    let range_typography = numeric_range_typography(stream, pair.start_idx(), pair.first(), pair.separator_idx(), pair.second())?;
    let unit = MeasurementUnitPhrase::parse_after_number(context, pair.second_end_idx())?;
    resolve_measurement_phrase(context, pair.start_idx()..unit.units().end_index(), pair.into_numbers(), unit, Some(range_typography))
}

fn parse_repeated_measurement_range_phrase(context: &ParseContext<'_>, first: &ParsedNumber) -> Option<Detected> {
    let stream = context.stream();
    let first_unit = MeasurementUnitPhrase::parse_after_number(context, first.units().end_index())?;
    let separator_idx = skip_layout_stream_units(stream, first_unit.units().end_index());
    let second_start = skip_layout_stream_units(stream, range_separator_end(context, separator_idx)?);
    let second_end = trimmed_number_span_end(stream, second_start, number_span_end(stream, second_start, context.number_policy()), context.number_policy());
    let second = numeric_phrase_for_range(context, second_start, second_end)?;
    let second_unit = MeasurementUnitPhrase::parse_after_number(context, second.units().end_index())?;
    (first_unit.canonical() == second_unit.canonical()).then(|| {
        resolve_measurement_phrase(context, first.units().indices().start..second_unit.units().end_index(), vec![first.clone(), second], second_unit, None)
    })?
}

fn parse_dimension_phrase(context: &ParseContext<'_>, first: &ParsedNumber) -> Option<Detected> {
    let stream = context.stream();
    let pair = parse_numeric_pair(context, first, NumericPairKind::Dimension)?;
    let glued = !stream_unit_range_has_whitespace(stream, pair.first_end_idx()..pair.separator_idx()) && !stream_unit_range_has_whitespace(stream, pair.separator_idx() + 1..pair.second_start_idx());
    let detected = match MeasurementUnitPhrase::parse_after_number(context, pair.second_end_idx()) {
        Some(unit) => resolve_measurement_phrase(context, pair.start_idx()..unit.units().end_index(), pair.into_numbers(), unit, None)?,
        None if glued => Detected::simple(NodeKind::Number, UnitRange::new(stream, pair.start_idx()..pair.second_end_idx())),
        None => return None,
    };
    Some(detected)
}

fn parse_numeric_pair<'a>(context: &ParseContext<'_>, first: &'a ParsedNumber, kind: NumericPairKind) -> Option<NumericPair<'a>> {
    let stream = context.stream();
    let separator_idx = skip_layout_stream_units(stream, first.units().end_index());
    let second_start = skip_layout_stream_units(stream, kind.separator_end(context, separator_idx)?);
    let second_end = trimmed_number_span_end(stream, second_start, number_span_end(stream, second_start, context.number_policy()), context.number_policy());
    numeric_phrase_for_range(context, second_start, second_end).map(|second| NumericPair::new(first, separator_idx, second))
}

fn resolve_measurement_phrase(context: &ParseContext<'_>, units: Range<usize>, numbers: Vec<ParsedNumber>, unit: MeasurementUnitPhrase, range_typography: Option<NumberRangeTypography>) -> Option<Detected> {
    let stream = context.stream();
    let values = unit.resolved().map(|resolved| {
        numbers
            .iter()
            .filter_map(|number| {
                let normalized = resolved.normalized_value(number.value_as_f64());
                to_fixed_point(normalized)
                    .filter(|fixed| *fixed != 0 || normalized == 0.0 || !resolved.precision_limited())
                    .map(|fixed| MeasurementValueFact::new(resolved.dimension(), fixed).with_quantity_kind(resolved.quantity_kind()))
            })
            .collect()
    }).unwrap_or_default();
    let typography = measurement_typography(stream, &units, &numbers, &unit, context.language())?;
    let (number_values, number_formats) = numbers.iter().map(|number| (number.value(), number.format())).unzip();
    let semantic = MeasurementSemantic::new(unit.canonical().to_string(), values, number_values, number_formats).with_typography(typography);
    let semantic = match range_typography { Some(range) => semantic.with_number_range_typography(range), None => semantic };
    Some(Detected::semantic(UnitRange::new(stream, units), NodeSemantic::Measurement(semantic)))
}

fn numeric_range_typography(stream: &SegmentTree, measurement_start_idx: usize, first: &ParsedNumber, separator_idx: usize, second: &ParsedNumber) -> Option<NumberRangeTypography> {
    let measurement_start = structure::parser_api::unit_start(stream, measurement_start_idx)?;
    let first_end = unit_span(stream, first.units()).end;
    let separator_span = structure::parser_api::unit_span(stream, separator_idx)?;
    let second_start = unit_span(stream, second.units()).start;
    let separator = structure::parser_api::unit_text(stream, separator_idx)?.chars().next()?;
    Some(NumberRangeTypography::new(
        u16::try_from(first_end.checked_sub(measurement_start)?).ok()?,
        u16::try_from(second_start.checked_sub(measurement_start)?).ok()?,
        separator,
        typography_spacing(stream.text().get(first_end..separator_span.start)?),
        typography_spacing(stream.text().get(separator_span.end..second_start)?),
        '–',
    ))
}

fn numeric_phrase_for_range(context: &ParseContext<'_>, start_idx: usize, end_idx: usize) -> Option<ParsedNumber> {
    let (_, raw_start) = numeric_start_indices(context.stream(), start_idx)?;
    (end_idx > raw_start).then(|| ParsedNumber::parse_range(context, start_idx, end_idx)).flatten()
}

fn measurement_typography(stream: &SegmentTree, measurement_units: &Range<usize>, numbers: &[ParsedNumber], unit: &MeasurementUnitPhrase, language: ParseLanguage) -> Option<MeasurementTypography> {
    let measurement_start = structure::parser_api::unit_start(stream, measurement_units.start)?;
    let measurement_end = structure::parser_api::unit_span(stream, measurement_units.end.checked_sub(1)?)?.end;
    let separator_start = unit_span(stream, numbers.last()?.units()).end;
    let unit_span = unit_span(stream, unit.units());
    let separator = stream.text().get(separator_start..unit_span.start)?;
    let unit_text = stream.text().get(unit_span.clone())?;
    let observed = WhitespaceTypographyKind::from_text(separator);
    let preference = preferred_measurement_whitespace(unit.canonical(), language);
    let role = if unit.canonical() == "%" { WhitespaceTypographyRole::PercentSeparator } else { WhitespaceTypographyRole::MeasurementSeparator };
    let whitespace = WhitespaceTypographyFact::from_preference(observed, role, preference, separator_start..unit_span.start, measurement_start..measurement_end);
    let degree_style = measurement_degree_style(unit_text, unit.canonical());
    Some(MeasurementTypography::new(
        u16::try_from(separator_start.checked_sub(measurement_start)?).ok()?,
        u16::try_from(unit_span.start.checked_sub(measurement_start)?).ok()?,
        u16::try_from(unit_span.end.checked_sub(measurement_start)?).ok()?,
        whitespace,
        degree_style,
    ))
}

fn preferred_measurement_whitespace(canonical: &str, language: ParseLanguage) -> WhitespaceTypographyPreference {
    match canonical {
        "%" if matches!(language, ParseLanguage::Swedish | ParseLanguage::German) => WhitespaceTypographyPreference::any_present(WhitespaceTypographyKind::NonBreaking),
        "%" => WhitespaceTypographyPreference::exact(WhitespaceTypographyKind::Absent),
        "°" | "′" | "″" | "‴" => WhitespaceTypographyPreference::exact(WhitespaceTypographyKind::Absent),
        _ => WhitespaceTypographyPreference::any_present(WhitespaceTypographyKind::NonBreaking),
    }
}

fn measurement_degree_style(observed: &str, canonical: &str) -> MeasurementDegreeStyle {
    let symbolic = observed.chars().any(|character| matches!(character, '°' | 'º' | '˚' | '℃' | '℉' | 'С'));
    let letter_variant = matches!(observed, "oC" | "oF");
    let applies = matches!(canonical, "°" | "°C" | "°F") && (symbolic || letter_variant);
    if !applies {
        return MeasurementDegreeStyle::NotApplicable;
    }
    if observed == canonical { MeasurementDegreeStyle::Canonical } else { MeasurementDegreeStyle::Variant }
}

fn range_separator_end(context: &ParseContext<'_>, idx: usize) -> Option<usize> {
    structure::parser_api::unit_text(context.stream(), idx).is_some_and(|text| context.profile().range_separators.contains(text)).then_some(idx + 1)
}

fn is_dimension_separator(stream: &SegmentTree, idx: usize) -> bool {
    structure::parser_api::unit_text(stream, idx).is_some_and(|text| matches!(text, "x" | "X" | "×"))
}

pub(super) fn parse_direct_measurement_unit_end(stream: &SegmentTree, boundary_idx: usize, locale: ParseLocale) -> Option<usize> {
    parse_measurement_unit_end_with(stream, boundary_idx, |unit| is_measurement_unit_for_locale(unit, locale, UnitAttachment::Attached))
}

/// Shared unit-recognition logic behind both the glued ("20°C") and
/// whitespace-separated ("20 °C" / "20\u{00A0}°C") measurement paths. A
/// degree sign tokenizes as its own single-character `Punct` unit followed
/// by a separate `Word` unit ("C"/"F"), so both callers need to recombine
/// them the same way; `unit_allowed` is what differs — the glued path
/// accepts any known unit, the whitespace path only unambiguous ones (to
/// avoid misreading things like "Figure 5 A" as a measurement).
pub(super) fn parse_measurement_unit_end_with(stream: &SegmentTree, boundary_idx: usize, unit_allowed: impl Fn(&str) -> bool) -> Option<usize> {
    let first = structure::parser_api::units(stream).get(boundary_idx)?;
    let first_text = structure::parser_api::unit_text(stream, boundary_idx)?;
    let contiguous_end = structure::parser_api::units(stream)[boundary_idx..]
        .iter()
        .position(|unit| unit.kind() == UnitKind::Whitespace && !unit.is_tag())
        .map_or(structure::parser_api::units(stream).len(), |offset| boundary_idx + offset);
    if let Some(end_idx) = multiword_measurement_unit_end(stream, boundary_idx, &unit_allowed) {
        return Some(end_idx);
    }
    if let Some(end_idx) = ((boundary_idx + 1)..=contiguous_end)
        .rev()
        .find(|end_idx| unit_range_text(stream, boundary_idx, *end_idx).is_some_and(&unit_allowed))
    {
        if end_idx + 1 < contiguous_end
            && structure::parser_api::unit_text(stream, end_idx).is_some_and(|text| matches!(text, "/" | "·" | "⋅" | "*"))
            && structure::parser_api::units(stream).get(end_idx + 1).is_some_and(|unit| unit.kind() == UnitKind::Word)
        {
            return None;
        }
        return Some(end_idx);
    }
    if first.kind() == UnitKind::Word && unit_allowed(first_text) {
        return Some(boundary_idx + 1);
    }
    if first.kind() != UnitKind::Punct {
        return None;
    }
    if is_measurement_unit_prefix_symbol(first_text)
        && let Some(second) = structure::parser_api::units(stream).get(boundary_idx + 1)
        && let Some(second_text) = structure::parser_api::unit_text(stream, boundary_idx + 1)
        && second.kind() == UnitKind::Word
    {
        let mut unit = String::from(first_text);
        unit.push_str(second_text);
        if unit_allowed(&unit) {
            return Some(boundary_idx + 2);
        }
    }
    if is_standalone_measurement_unit(first_text) {
        return Some(boundary_idx + 1);
    }
    if unit_allowed(first_text) {
        return Some(boundary_idx + 1);
    }
    if is_measurement_unit_separator(first_text) {
        let second = structure::parser_api::units(stream).get(boundary_idx + 1)?;
        if second.kind() == UnitKind::Word {
            let second_text = structure::parser_api::unit_text(stream, boundary_idx + 1)?;
            if unit_allowed(second_text) {
                return Some(boundary_idx + 2);
            }
        }
    }
    None
}

fn multiword_measurement_unit_end(stream: &SegmentTree, boundary_idx: usize, unit_allowed: &impl Fn(&str) -> bool) -> Option<usize> {
    if structure::parser_api::units(stream).get(boundary_idx)?.kind() != UnitKind::Word {
        return None;
    }
    const MAX_UNIT_PARTS: usize = 12;
    structure::parser_api::units(stream)[boundary_idx..]
        .iter()
        .enumerate()
        .take(MAX_UNIT_PARTS)
        .scan((), |_, (offset, unit)| {
            if !unit.is_tag() && !matches!(unit.kind(), UnitKind::Word | UnitKind::Whitespace | UnitKind::Punct) {
                return None;
            }
            Some((boundary_idx + offset + 1, unit.kind()))
        })
        .filter(|(end_idx, kind)| *kind != UnitKind::Whitespace && !structure::parser_api::units(stream)[end_idx - 1].is_tag())
        .filter_map(|(end_idx, _)| unit_range_text(stream, boundary_idx, end_idx).filter(|text| text.chars().any(char::is_whitespace)).map(|text| (end_idx, text)))
        .filter(|(_, text)| unit_allowed(text))
        .map(|(end_idx, _)| end_idx)
        .last()
}

pub(super) fn is_measurement_unit_prefix_symbol(text: &str) -> bool {
    matches!(normalized_measurement_unit_suffix(text), "°")
}

pub(super) fn is_standalone_measurement_unit(text: &str) -> bool {
    matches!(normalized_measurement_unit_suffix(text), "°" | "")
}

pub(super) fn normalized_measurement_unit_suffix(text: &str) -> &str {
    text.trim_end_matches(['\u{FE0E}', '\u{FE0F}', '\u{200B}', '\u{200D}', '\u{2060}'])
}

pub(super) fn is_measurement_unit_separator(text: &str) -> bool {
    matches!(text, "\u{00A0}" | "\u{202F}")
}
pub(super) fn is_measurement_unit_for_locale(unit: &str, locale: ParseLocale, attachment: UnitAttachment) -> bool {
    canonical_measurement_unit_for_locale(unit, locale, attachment).is_some() || canonical_numeric_magnitude_suffix(unit, locale.language()).is_some()
}

/// Unambiguous enough to recognize across a whitespace gap without extra
/// grounding: localized full names, locale aliases, and prefixed SI forms
/// are accepted after a number. A bare SI base symbol ("m", "g", "s",
/// "A", ...) can collide with ordinary text and is accepted only when the
/// specific base is distinctive on its own (e.g. "Hz", "%", "ft").
pub(super) fn is_unambiguous_space_unit(unit: &str, language: ParseLanguage) -> bool {
    if unit_spelling_is_universally_ambiguous_after_number(unit) || language.profile().unit_lexicon().is_ambiguous_after_number(unit) {
        return false;
    }
    if canonical_numeric_magnitude_suffix(unit, language).is_some() {
        return true;
    }
    if parse_unit_alias(unit, language).is_some() || parse_unit_word(unit, language).is_some() {
        return true;
    }
    match parse_unit_symbol(unit).filter(|_| unit_spelling_allowed_in_language(unit, language)) {
        Some((Some(_prefix), _base)) => true,
        Some((None, base)) => base.bare_unambiguous(),
        None => canonical_measurement_unit(unit, language).is_some() && (unit.chars().count() > 1 || unit.chars().any(|c| !c.is_alphabetic())),
    }
}

pub(super) fn is_unambiguous_space_unit_for_locale(unit: &str, locale: ParseLocale) -> bool {
    locale_capacity_unit(unit, locale, UnitAttachment::Separated).is_some() || is_unambiguous_space_unit(unit, locale.language())
}

#[cfg(test)]
mod tests;
