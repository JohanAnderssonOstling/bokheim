use super::*;
use crate::language_parser::{detect_abbreviation_for_profile, detect_abbreviation_for_profile_in_unit_range, detect_apostrophe_word_at};

pub(super) fn parse_language_word_phrase_at(context: &ParseContext<'_>, start_idx: usize, _token: &BoundedToken<'_>) -> Option<Detection> {
    let stream = context.stream();
    let Some(span) = context
        .profile()
        .join_apostrophe_words
        .then(|| detect_apostrophe_word_at(stream, start_idx))
        .flatten() else {
        return None;
    };
    let units = unit_range_for_span(stream, start_idx, &span)?;
    Some(Detection::weak_language(Detected::language(NodeKind::Word, UnitRange::new(stream, units), span)))
}

pub(super) fn parse_hyphenated_compound_phrase_at(context: &ParseContext<'_>, start_idx: usize, _token: &BoundedToken<'_>) -> Option<Detection> {
    let stream = context.stream();
    let (first, first_is_abbreviation) = compound_component_at(context, start_idx)?;
    let mut end = first.end_index();
    let mut abbreviation_units = first_is_abbreviation.then_some(first).into_iter().collect::<Vec<_>>();
    let mut saw_hyphen = false;
    while is_hyphen_unit(stream, end) {
        let Some((component, is_abbreviation)) = compound_component_at(context, end + 1) else { break; };
        saw_hyphen = true;
        end = component.end_index();
        if is_abbreviation {
            abbreviation_units.push(component);
        }
    }
    saw_hyphen.then(|| Detection::weak_language(Detected::compound_word(UnitRange::new(stream, start_idx..end), abbreviation_units)))
}

fn compound_component_at(context: &ParseContext<'_>, start_idx: usize) -> Option<(UnitRange, bool)> {
    let stream = context.stream();
    let unit = structure::parser_api::units(stream).get(start_idx)?;
    if unit.kind() == UnitKind::Abbreviation {
        return Some((UnitRange::new(stream, start_idx..(start_idx + 1)), true));
    }
    if unit.kind() != UnitKind::Word {
        return None;
    }
    let contiguous_end = contiguous_slice_end(structure::parser_api::units(stream), start_idx);
    let abbreviation_end = ((start_idx + 2)..=contiguous_end).rev().find(|end_idx| detect_abbreviation_for_profile_in_unit_range(stream, start_idx, *end_idx, context.profile()).is_some());
    Some(abbreviation_end.map_or_else(|| (UnitRange::new(stream, start_idx..(start_idx + 1)), false), |end_idx| (UnitRange::new(stream, start_idx..end_idx), true)))
}

pub(super) fn is_hyphen_unit(stream: &SegmentTree, idx: usize) -> bool { structure::parser_api::units(stream).get(idx).is_some_and(|unit| unit.kind() == UnitKind::Punct && matches!(structure::parser_api::unit_text(stream, idx), Some("-" | "‑" | "–"))) }

pub(super) fn parse_abbreviation_phrase_at(context: &ParseContext<'_>, start_idx: usize, _token: &BoundedToken<'_>) -> Option<Detection> {
    let stream = context.stream();
    let span = detect_abbreviation_for_profile(stream, start_idx, context.profile())?;
    let units = unit_range_for_span(stream, start_idx, &span)?;
    Some(Detection::weak_language(Detected::language(NodeKind::Abbreviation, UnitRange::new(stream, units), span)))
}
