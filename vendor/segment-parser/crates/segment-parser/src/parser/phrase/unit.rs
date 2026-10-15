use super::*;

#[derive(Clone, Debug, PartialEq)]
pub(in crate::parser) struct MeasurementUnitPhrase {
    units: UnitRange,
    canonical: String,
    resolved: Option<ResolvedMeasurementUnit>,
}

impl MeasurementUnitPhrase {
    pub(in crate::parser) fn parse_direct(context: &ParseContext<'_>, boundary_idx: usize) -> Option<Self> {
        let end_idx = parse_direct_measurement_unit_end(context.stream(), boundary_idx, context.locale())?;
        Self::from_range(context, boundary_idx..end_idx, UnitAttachment::Attached).filter(|unit| unit.canonical() != "rem")
    }

    pub(in crate::parser) fn parse_after_number(context: &ParseContext<'_>, number_end_idx: usize) -> Option<Self> {
        let direct_idx = skip_tag_stream_units(context.stream(), number_end_idx);
        let unit_idx = skip_layout_stream_units(context.stream(), number_end_idx);
        let source_grounded = stream_unit_range_has_whitespace(context.stream(), number_end_idx..unit_idx).then(|| Self::parse_source_grounded(context, unit_idx)).flatten();
        source_grounded
            .or_else(|| Self::parse_direct(context, direct_idx))
            .or_else(|| stream_unit_range_has_whitespace(context.stream(), number_end_idx..unit_idx).then(|| Self::parse_space_grounded(context, unit_idx)).flatten())
    }

    fn parse_source_grounded(context: &ParseContext<'_>, unit_idx: usize) -> Option<Self> {
        let end_idx = parse_measurement_unit_end_with(context.stream(), unit_idx, |text| source_grounded_measurement_unit_canonical(text, context).is_some())?;
        source_grounded_measurement_unit_canonical(unit_range_text(context.stream(), unit_idx, end_idx)?, context)?;
        Self::from_range(context, unit_idx..end_idx, UnitAttachment::Separated)
    }

    pub(in crate::parser) fn parse_space_grounded(context: &ParseContext<'_>, unit_idx: usize) -> Option<Self> {
        let end_idx = parse_measurement_unit_end_with(context.stream(), unit_idx, |text| {
            is_unambiguous_space_unit_for_locale(text, context.locale())
                || measurement_unit_is_source_grounded(text, context)
        })?;
        Self::from_range(context, unit_idx..end_idx, UnitAttachment::Separated)
    }

    pub(in crate::parser) fn units(&self) -> &UnitRange { &self.units }
    pub(in crate::parser) fn canonical(&self) -> &str { &self.canonical }
    pub(in crate::parser) fn resolved(&self) -> Option<&ResolvedMeasurementUnit> { self.resolved.as_ref() }

    fn from_range(context: &ParseContext<'_>, range: Range<usize>, attachment: UnitAttachment) -> Option<Self> {
        let text = unit_range_text(context.stream(), range.start, range.end)?;
        let normalized = normalized_measurement_unit_suffix(text);
        let canonical = if normalized.is_empty() {
            "°".to_string()
        } else {
            canonical_numeric_magnitude_suffix(normalized, context.language())
                .map(str::to_string)
                .or_else(|| canonical_measurement_unit_for_locale(normalized, context.locale(), attachment))
                .or_else(|| source_grounded_measurement_unit_canonical(normalized, context))?
        };
        let resolved = resolve_canonical_measurement_unit(&canonical);
        Some(Self { units: UnitRange::new(context.stream(), range), canonical, resolved })
    }
}

fn measurement_unit_is_source_grounded(text: &str, context: &ParseContext<'_>) -> bool {
    if source_grounded_measurement_unit_canonical(text, context).is_some() {
        return true;
    }
    let Some(candidate) = resolve_measurement_unit_for_locale(text, context.locale(), UnitAttachment::Separated) else { return false };
    context.known_units().iter().any(|known| {
        resolve_canonical_measurement_unit(known)
            .is_some_and(|source| source.canonical() == candidate.canonical() || source.dimension() == candidate.dimension() && source.quantity_kind() == candidate.quantity_kind())
    })
}

fn source_grounded_measurement_unit_canonical(text: &str, context: &ParseContext<'_>) -> Option<String> {
    let candidate = parse_source_grounded_unit_canonical(text, context.language())?;
    resolve_canonical_measurement_unit(&candidate)?;
    context
        .known_units()
        .iter()
        .any(|known| *known == candidate)
        .then_some(candidate)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::parser) struct CurrencyUnitPhrase {
    units: UnitRange,
    canonical: Option<&'static str>,
}

impl CurrencyUnitPhrase {
    pub(in crate::parser) fn parse_at(context: &ParseContext<'_>, idx: usize) -> Option<Self> {
        const MAX_CURRENCY_PHRASE_UNITS: usize = 3;
        let stream = context.stream();
        let candidate_ends = structure::parser_api::units(stream).iter().enumerate().skip(idx).filter(|(_, unit)| !unit.is_tag()).take(MAX_CURRENCY_PHRASE_UNITS).map(|(index, _)| index + 1).collect::<Vec<_>>();
        candidate_ends.into_iter().rev().find_map(|end| {
            let last = structure::parser_api::units(stream).get(end - 1)?;
            if last.kind() == UnitKind::Whitespace || last.is_tag() || end > idx + 1 && last.kind() != UnitKind::Word {
                return None;
            }
            let text = unit_range_text(stream, idx, end)?;
            let resolution = resolve_currency_unit_for_profile(text, context.profile(), context.known_currency_units())?;
            Some(Self { units: UnitRange::new(stream, idx..end), canonical: resolution.canonical() })
        })
    }

    pub(in crate::parser) fn units(&self) -> &UnitRange { &self.units }
    pub(in crate::parser) fn canonical(&self) -> Option<&'static str> { self.canonical }
}
