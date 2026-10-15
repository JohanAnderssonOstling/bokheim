use super::*;

type PhraseParserFn = for<'a> fn(&ParseContext<'a>, usize, &BoundedToken<'a>) -> Option<Detection>;

struct PhraseRule {
    start: PhraseStart,
    parser: PhraseParserFn,
}

impl PhraseRule {
    fn detect(&self, context: &ParseContext<'_>, index: usize, unit: &TokenUnit, token: &BoundedToken<'_>) -> Option<Detection> {
        self.start.accepts(unit).then(|| (self.parser)(context, index, token)).flatten()
    }
}

#[derive(Clone, Copy)]
enum PhraseStart {
    Any,
    Word,
    WordOrAbbreviation,
    WordNumericOrPunct,
    NumericOrPunct,
}

impl PhraseStart {
    fn accepts(self, unit: &TokenUnit) -> bool {
        match self {
            Self::Any => true,
            Self::Word => unit.kind() == UnitKind::Word,
            Self::WordOrAbbreviation => matches!(unit.kind(), UnitKind::Word | UnitKind::Abbreviation),
            Self::WordNumericOrPunct => matches!(unit.kind(), UnitKind::Word | UnitKind::Numeric | UnitKind::Punct),
            Self::NumericOrPunct => matches!(unit.kind(), UnitKind::Numeric | UnitKind::Punct),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum DetectionPriority {
    Fallback,
    WeakLanguage,
    GroundedSemantic,
    Protected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum PhraseConfidence {
    Low,
    Normal,
    High,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum PhraseAmbiguity {
    Ambiguous,
    Contextual,
    Certain,
}

const PHRASE_RULES: &[PhraseRule] = &[
    PhraseRule { start: PhraseStart::Any, parser: parse_hex_color_phrase_at },
    PhraseRule { start: PhraseStart::Any, parser: parse_uuid_phrase_from_token },
    PhraseRule { start: PhraseStart::Any, parser: parse_ip_address_phrase_from_token },
    PhraseRule { start: PhraseStart::Any, parser: parse_path_phrase_from_token },
    PhraseRule { start: PhraseStart::Any, parser: parse_code_identifier_phrase_from_token },
    PhraseRule { start: PhraseStart::Any, parser: parse_compact_alnum_code_phrase_from_token },
    PhraseRule { start: PhraseStart::WordNumericOrPunct, parser: parse_textual_date_phrase_at },
    PhraseRule { start: PhraseStart::WordNumericOrPunct, parser: parse_currency_phrase_at },
    PhraseRule { start: PhraseStart::Word, parser: parse_number_word_phrase_at },
    PhraseRule { start: PhraseStart::Word, parser: parse_url_phrase_from_token },
    PhraseRule { start: PhraseStart::Word, parser: parse_email_phrase_from_token },
    PhraseRule { start: PhraseStart::WordNumericOrPunct, parser: parse_version_phrase_at },
    PhraseRule { start: PhraseStart::NumericOrPunct, parser: parse_ordinal_phrase_at },
    PhraseRule { start: PhraseStart::Word, parser: parse_language_word_phrase_at },
    PhraseRule { start: PhraseStart::WordOrAbbreviation, parser: parse_hyphenated_compound_phrase_at },
    PhraseRule { start: PhraseStart::Word, parser: parse_abbreviation_phrase_at },
    PhraseRule { start: PhraseStart::Word, parser: parse_non_translatable_phrase_from_token },
    PhraseRule { start: PhraseStart::NumericOrPunct, parser: parse_time_phrase_at },
    PhraseRule { start: PhraseStart::NumericOrPunct, parser: parse_number_phrase_at },
];

pub(super) fn parse_nodes_from_phrases(context: &ParseContext<'_>) -> Vec<Node> {
    parse_nodes_in_range(context, 0, structure::parser_api::units(context.stream()).len())
}

fn parse_nodes_in_range(context: &ParseContext<'_>, start_idx: usize, end_idx: usize) -> Vec<Node> {
    let stream = context.stream();
    let mut nodes = Vec::new();
    let mut index = start_idx.min(end_idx);

    while index < end_idx {
        if let Some(pair) = matching_context_pair(context, index, end_idx) {
            let close_idx = pair.close_idx();
            nodes.push(delimiter_leaf(context, index, pair.family(), DelimiterRole::Open));
            if close_idx > index + 1 {
                nodes.extend(parse_nodes_in_range(context, index + 1, close_idx));
            }
            nodes.push(delimiter_leaf(context, close_idx, pair.family(), DelimiterRole::Close));
            index = close_idx + 1;
            continue;
        }

        let Some(unit) = structure::parser_api::units(stream).get(index) else {
            nodes.push(leaf_from_unit(stream, index));
            index += 1;
            continue;
        };
        if unit.is_tag() || unit.kind() == UnitKind::Whitespace {
            nodes.push(leaf_from_unit(stream, index));
            index += 1;
            continue;
        }

        if let Some(ellipsis_end) = ellipsis_units_end(stream, index, end_idx) {
            nodes.push(ellipsis_node(context, index, ellipsis_end));
            index = ellipsis_end;
            continue;
        }

        if let Some(annotation) = unmatched_delimiter_annotation(context, index) {
            nodes.push(annotated_punctuation_leaf(context, index).with_annotation(NodeAnnotation::Delimiter(annotation)));
            index += 1;
            continue;
        }

        if let Some(detected) = parse_detected_phrase_at(context, index).filter(|detected| detected.units_end() <= end_idx) {
            let units_end = detected.units_end();
            let node = materialize_detected(stream, detected);
            let consumed_end = (units_end..end_idx).take_while(|candidate| structure::parser_api::units(stream).get(*candidate).is_some_and(|unit| unit.start() < node.end())).last().map_or(units_end, |candidate| candidate + 1);
            nodes.push(node);
            emit_interior_tags(stream, index + 1, consumed_end, &mut nodes);
            index = consumed_end;
            continue;
        }

        nodes.push(annotated_punctuation_leaf(context, index));
        index += 1;
    }
    debug_assert!(nodes_are_ordered_without_overlapping_text(&nodes));
    nodes
}

fn ellipsis_units_end(stream: &SegmentTree, index: usize, end_idx: usize) -> Option<usize> {
    let unit = structure::parser_api::units(stream).get(index)?;
    if unit.kind() == UnitKind::Punct && structure::parser_api::unit_text(stream, index) == Some("\u{2026}") {
        return Some(index + 1);
    }
    let ellipsis_end = index.checked_add(3)?;
    (ellipsis_end <= end_idx
        && (index..ellipsis_end).all(|candidate| structure::parser_api::units(stream).get(candidate).is_some_and(|unit| unit.kind() == UnitKind::Punct) && structure::parser_api::unit_text(stream, candidate) == Some("."))
        && structure::parser_api::units(stream).get(ellipsis_end).is_none_or(|unit| unit.kind() != UnitKind::Punct || structure::parser_api::unit_text(stream, ellipsis_end) != Some(".")))
        .then_some(ellipsis_end)
}

fn ellipsis_node(context: &ParseContext<'_>, start: usize, end: usize) -> Node {
    let children = (start..end).map(|index| leaf_from_unit(context.stream(), index)).collect();
    ellipsis_whitespace_facts(context, start..end).into_iter().fold(
        Node::new(NodeKind::Ellipsis, unit_span(context.stream(), &UnitRange::new(context.stream(), start..end)), None, children),
        |node, fact| node.with_annotation(NodeAnnotation::WhitespaceTypography(fact)),
    )
}

fn delimiter_leaf(context: &ParseContext<'_>, index: usize, family: DelimiterFamily, role: DelimiterRole) -> Node {
    delimiter_interior_whitespace_fact(context, index, family, role)
        .into_iter()
        .fold(annotated_punctuation_leaf(context, index), |leaf, fact| leaf.with_annotation(NodeAnnotation::WhitespaceTypography(fact)))
        .with_annotation(NodeAnnotation::Delimiter(delimiter_annotation(context, family, role)))
}

fn annotated_punctuation_leaf(context: &ParseContext<'_>, index: usize) -> Node {
    punctuation_whitespace_facts(context, index).into_iter().fold(leaf_from_unit(context.stream(), index), |leaf, fact| leaf.with_annotation(NodeAnnotation::WhitespaceTypography(fact)))
}

fn parse_detected_phrase_at(context: &ParseContext<'_>, index: usize) -> Option<Detected> {
    let unit = structure::parser_api::units(context.stream()).get(index)?;
    phrase_candidates_at(context, index, unit).into_iter().max_by_key(|candidate| candidate.score).map(|candidate| candidate.detection.resolve())
}

fn phrase_candidates_at(context: &ParseContext<'_>, index: usize, unit: &TokenUnit) -> Vec<PhraseCandidate> {
    let Some(token) = BoundedToken::new(context.stream(), index) else { return Vec::new() };
    PHRASE_RULES
        .iter()
        .enumerate()
        .filter_map(|(order, rule)| rule.detect(context, index, unit, &token).filter(|detection| detection_respects_non_translatable_boundaries(context.stream(), detection) && detection_allowed_in_context(context, detection)).map(|detection| PhraseCandidate::new(detection, order)))
        .collect()
}

fn detection_allowed_in_context(context: &ParseContext<'_>, detection: &Detection) -> bool {
    if detection.detected().node_kind() != NodeKind::Opaque {
        return true;
    }
    let span = unit_span(context.stream(), detection.detected().units());
    context.stream().text().get(span).is_some_and(|text| context.allows_non_translatable(text))
}

fn detection_respects_non_translatable_boundaries(stream: &SegmentTree, detection: &Detection) -> bool {
    detection.detected().node_kind() != NodeKind::Opaque
        || detection.detected().units().indices().filter_map(|index| structure::parser_api::unit_text(stream, index)).all(|text| !text.contains('(') && !text.contains(')'))
}

struct PhraseCandidate {
    detection: Detection,
    score: CandidateScore,
}

impl PhraseCandidate {
    fn new(detection: Detection, order: usize) -> Self {
        let units = detection.detected().units().indices();
        let score = CandidateScore::new(detection.priority(), detection.confidence(), detection.ambiguity(), units.end.saturating_sub(units.start), order);
        Self { detection, score }
    }
}
