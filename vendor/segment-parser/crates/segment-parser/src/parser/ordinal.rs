use super::*;
use crate::ordinal::{english_ordinal_suffix, swedish_ordinal_suffix};

pub(super) fn parse_ordinal_phrase_at(context: &ParseContext<'_>, start_idx: usize, _token: &BoundedToken<'_>) -> Option<Detection> {
    let stream = context.stream();
    let (number_end, value) = numeric_unit_value_with_end(stream, start_idx)?;
    let (style, end_idx) = english_ordinal_end(stream, number_end, value)
        .map(|end| (OrdinalStyle::EnglishSuffix, end))
        .or_else(|| swedish_ordinal_end(stream, number_end, value).map(|end| (OrdinalStyle::SwedishColonSuffix, end)))
        .or_else(|| german_ordinal_end(context, start_idx, number_end).map(|end| (OrdinalStyle::GermanPeriod, end)))?;
    let semantic = OrdinalFact::new(value, style, context.profile().preferred_ordinal_style);
    let detected = Detected::semantic(UnitRange::new(stream, start_idx..end_idx), NodeSemantic::Ordinal(semantic));
    Some(Detection::grounded_semantic(detected, PhraseConfidence::High, PhraseAmbiguity::Contextual))
}

fn english_ordinal_end(stream: &SegmentTree, number_end: usize, value: u32) -> Option<usize> {
    let suffix_idx = skip_tag_stream_units(stream, number_end);
    let suffix_end = visible_unit_run_end(stream, suffix_idx, UnitKind::Word);
    let suffix = unit_range_text(stream, suffix_idx, suffix_end)?;
    let expected = english_ordinal_suffix(value);
    (structure::parser_api::units(stream).get(suffix_idx)?.kind() == UnitKind::Word && suffix.eq_ignore_ascii_case(expected)).then_some(suffix_end)
}

fn swedish_ordinal_end(stream: &SegmentTree, number_end: usize, value: u32) -> Option<usize> {
    let separator_idx = skip_tag_stream_units(stream, number_end);
    let suffix_idx = visible_stream_unit_after(stream, separator_idx)?;
    let suffix_end = visible_unit_run_end(stream, suffix_idx, UnitKind::Word);
    let suffix = unit_range_text(stream, suffix_idx, suffix_end)?;
    let expected = swedish_ordinal_suffix(value);
    (is_punct_text(stream, separator_idx, ":") && structure::parser_api::units(stream).get(suffix_idx)?.kind() == UnitKind::Word && suffix.eq_ignore_ascii_case(expected)).then_some(suffix_end)
}

fn german_ordinal_end(context: &ParseContext<'_>, start_idx: usize, number_end: usize) -> Option<usize> {
    if context.profile().preferred_ordinal_style != OrdinalStyle::GermanPeriod {
        return None;
    }
    let period_idx = skip_tag_stream_units(context.stream(), number_end);
    if !is_punct_text(context.stream(), period_idx, ".") {
        return None;
    }
    if visible_stream_unit_after(context.stream(), period_idx).is_some_and(|index| structure::parser_api::units(context.stream()).get(index).is_some_and(|unit| unit.kind() == UnitKind::Numeric)) {
        return None;
    }
    german_ordinal_context(context.stream(), start_idx).then_some(period_idx + 1)
}

fn german_ordinal_context(stream: &SegmentTree, start_idx: usize) -> bool {
    let previous = (0..start_idx).rev().find(|index| !matches!(structure::parser_api::units(stream)[*index].kind(), UnitKind::Whitespace | UnitKind::Tag));
    let Some(previous_idx) = previous else { return true; };
    let previous_is_cue = structure::parser_api::units(stream)[previous_idx].kind() == UnitKind::Word
        && matches!(structure::parser_api::unit_text(stream, previous_idx).map(str::to_lowercase).as_deref(), Some("am" | "der" | "die" | "das" | "den" | "dem" | "des" | "im" | "vom" | "zum" | "zur"));
    previous_is_cue || german_ordinal_sequence_before(stream, previous_idx)
}

fn german_ordinal_sequence_before(stream: &SegmentTree, previous_idx: usize) -> bool {
    if !is_punct_text(stream, previous_idx, ".") {
        return false;
    }
    (0..previous_idx).rev()
        .find(|index| !matches!(structure::parser_api::units(stream)[*index].kind(), UnitKind::Whitespace | UnitKind::Tag))
        .is_some_and(|index| structure::parser_api::units(stream)[index].kind() == UnitKind::Numeric)
}

#[cfg(test)]
mod tests {
    use super::*;
    use structure::OrdinalAssessment;

    fn parsed(text: &str, language: &str) -> SegmentTree {
        let mut tree = crate::tokenized_text_tree(text);
        crate::parse(&mut tree, language);
        tree
    }

    #[test]
    fn parses_english_swedish_and_german_ordinal_styles_as_the_same_values() {
        let english = parsed("1st 2nd 3rd 11th", "en");
        let swedish = parsed("1:a 2:a 3:e 11:e", "sv");
        let german = parsed("1. 2. 3. 11.", "de");

        [english, swedish, german].into_iter().for_each(|tree| {
            assert_eq!(crate::ordinal_facts(&tree).map(|fact| fact.value()).collect::<Vec<_>>(), vec![1, 2, 3, 11]);
            assert!(crate::ordinal_facts(&tree).all(|fact| fact.assessment() == OrdinalAssessment::Canonical));
        });
    }

    #[test]
    fn german_grouped_integer_is_not_split_into_an_ordinal() {
        let tree = parsed("2.000 Einheiten", "de");

        assert!(crate::ordinal_facts(&tree).next().is_none());
        assert_eq!(crate::numeric_facts(&tree).next().map(|fact| fact.text()), Some("2.000"));
    }

}
