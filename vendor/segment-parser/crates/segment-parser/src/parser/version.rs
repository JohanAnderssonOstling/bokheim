use super::*;

pub(super) fn parse_version_phrase_at(context: &ParseContext<'_>, start_idx: usize, token: &BoundedToken<'_>) -> Option<Detection> {
    let stream = context.stream();
    let candidate = token.trimmed()?;
    if structure::parser_api::units(stream).get(start_idx)?.kind() == UnitKind::Numeric && parse_date_phrase(context, start_idx, candidate.end_index()).is_some() {
        return None;
    }
    let (prefix_style, mut index, min_dots) = match structure::parser_api::units(stream).get(start_idx)?.kind() {
        UnitKind::Numeric => (VersionPrefixStyle::None, start_idx, if preceded_by_version_cue(stream, start_idx) { 1usize } else { 2usize }),
        UnitKind::Word if structure::parser_api::unit_text(stream, start_idx) == Some("v") => (VersionPrefixStyle::LowerV, visible_stream_unit_after(stream, start_idx)?, 1usize),
        UnitKind::Word if structure::parser_api::unit_text(stream, start_idx) == Some("V") => (VersionPrefixStyle::UpperV, visible_stream_unit_after(stream, start_idx)?, 1usize),
        _ => return None,
    };
    if structure::parser_api::units(stream).get(index)?.kind() != UnitKind::Numeric { return None; }
    let component_end = visible_unit_run_end(stream, index, UnitKind::Numeric);
    let mut components = vec![unit_range_text(stream, index, component_end)?.to_string()];
    index = skip_tag_stream_units(stream, component_end);
    let mut dots = 0usize;
    while is_punct_text(stream, index, ".") {
        let Some(component_start) = visible_stream_unit_after(stream, index).filter(|index| structure::parser_api::units(stream).get(*index).is_some_and(|unit| unit.kind() == UnitKind::Numeric)) else { break };
        let component_end = visible_unit_run_end(stream, component_start, UnitKind::Numeric);
        dots += 1;
        components.push(unit_range_text(stream, component_start, component_end)?.to_string());
        index = skip_tag_stream_units(stream, component_end);
    }
    if dots < min_dots { return None; }
    let (after_prerelease, prerelease) = consume_version_label(stream, index, candidate.end_index(), "-", version_suffix_head_allowed);
    let (end, build_metadata) = consume_version_label(stream, after_prerelease, candidate.end_index(), "+", |_| true);
    let semantic = VersionSemantic::new(prefix_style, components).with_prerelease(prerelease).with_build_metadata(build_metadata);
    Some(Detection::protected(Detected::semantic(UnitRange::new(stream, start_idx..end), NodeSemantic::Version(Box::new(semantic)))))
}

fn preceded_by_version_cue(stream: &SegmentTree, start_idx: usize) -> bool {
    (0..start_idx).rev()
        .find(|index| !matches!(structure::parser_api::units(stream)[*index].kind(), UnitKind::Whitespace | UnitKind::Tag))
        .and_then(|index| structure::parser_api::unit_text(stream, index))
        .is_some_and(|text| text.eq_ignore_ascii_case("version"))
}

fn consume_version_label(stream: &SegmentTree, index: usize, candidate_end: usize, separator: &str, head_allowed: impl Fn(&str) -> bool) -> (usize, Option<String>) {
    if !is_punct_text(stream, index, separator) { return (index, None); }
    let Some(head) = visible_stream_unit_after(stream, index) else { return (index, None); };
    let Some(text) = structure::parser_api::unit_text(stream, head) else { return (index, None); };
    if !matches!(structure::parser_api::units(stream).get(head).map(TokenUnit::kind), Some(UnitKind::Word | UnitKind::Numeric)) || !head_allowed(text) { return (index, None); }
    let mut cursor = head;
    let mut end = head;
    while cursor < candidate_end {
        let unit = &structure::parser_api::units(stream)[cursor];
        match unit.kind() {
            UnitKind::Word | UnitKind::Numeric if !unit.is_tag() => end = cursor + 1,
            UnitKind::Punct if matches!(structure::parser_api::unit_text(stream, cursor), Some("." | "-"))
                && visible_stream_unit_after(stream, cursor).is_some_and(|next| structure::parser_api::units(stream).get(next).is_some_and(|unit| matches!(unit.kind(), UnitKind::Word | UnitKind::Numeric))) => end = cursor + 1,
            _ => break,
        }
        cursor = skip_tag_stream_units(stream, cursor + 1);
    }
    let value = version_label_text(stream, head, end);
    (end, value)
}

fn version_label_text(stream: &SegmentTree, start: usize, end: usize) -> Option<String> {
    let span_start = structure::parser_api::units(stream).get(start)?.span().start;
    let span_end = structure::parser_api::units(stream).get(end.checked_sub(1)?)?.span().end;
    stream.text().get(span_start..span_end).map(ToString::to_string)
}

fn version_suffix_head_allowed(text: &str) -> bool { ["alpha", "a", "beta", "b", "rc", "pre", "preview", "snapshot", "final", "dev", "build"].iter().any(|suffix| text.eq_ignore_ascii_case(suffix)) }
