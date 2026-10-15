use super::*;

pub(super) fn next_non_layout_node(nodes: &[Node], start: usize) -> Option<usize> { (start..nodes.len()).find(|index| !matches!(nodes[*index].kind(), NodeKind::Whitespace | NodeKind::Tag)) }

pub(super) fn previous_non_layout_node(nodes: &[Node], before: usize) -> Option<usize> { (0..before).rev().find(|index| !matches!(nodes[*index].kind(), NodeKind::Whitespace | NodeKind::Tag)) }

pub(super) fn nodes_are_ordered_without_overlapping_text(nodes: &[Node]) -> bool { nodes.iter().filter(|node| node.kind() != NodeKind::Tag).try_fold(0usize, |previous_end, node| (node.start() >= previous_end).then_some(previous_end.max(node.end()))).is_some() }

pub(super) fn unit_span(stream: &SegmentTree, units: &UnitRange) -> Range<usize> {
    debug_assert!(stream.token_index(units.first_id()).is_some());
    debug_assert!(stream.token_index(units.last_id()).is_some());
    let indices = units.indices();
    structure::parser_api::unit_start(stream, indices.start).expect("unit start")..structure::parser_api::unit_end(stream, indices.end - 1).expect("unit end")
}

pub(super) fn emit_interior_tags(stream: &SegmentTree, start_idx: usize, end_idx: usize, nodes: &mut Vec<Node>) {
    for idx in start_idx..end_idx {
        if structure::parser_api::units(stream).get(idx).is_some_and(TokenUnit::is_tag) {
            nodes.push(leaf_from_unit(stream, idx));
        }
    }
}

pub(super) fn materialize_detected(stream: &SegmentTree, detected: Detected) -> Node {
    match detected {
        Detected::Simple { kind, units } => node_from_units(stream, kind, &units, Vec::new()),
        Detected::Language { kind, span, .. } => Node::new(kind, span, None, Vec::new()),
        Detected::Semantic { units, semantic } => node_from_units_with_semantic(stream, &units, semantic, Vec::new()),
        Detected::CompoundWord { units, abbreviation_units } => {
            let children = compound_word_children(stream, &units, &abbreviation_units);
            node_from_units(stream, NodeKind::CompoundWord, &units, children)
        }
    }
}

fn compound_word_children(stream: &SegmentTree, units: &UnitRange, abbreviation_units: &[UnitRange]) -> Vec<Node> {
    let mut children = Vec::new();
    let mut index = units.indices().start;
    while index < units.indices().end {
        if let Some(abbreviation) = abbreviation_units.iter().find(|candidate| candidate.indices().start == index) {
            index = abbreviation.indices().end;
            children.push(node_from_units(stream, NodeKind::Abbreviation, abbreviation, Vec::new()));
            continue;
        }
        children.push(leaf_from_unit(stream, index));
        index += 1;
    }
    children
}

fn node_from_units(stream: &SegmentTree, kind: NodeKind, units: &UnitRange, children: Vec<Node>) -> Node {
    let span = unit_span(stream, units);
    let span = if kind == NodeKind::Url { expanded_url_span(stream.text(), span) } else { span };
    Node::new(kind, span, None, children)
}

fn node_from_units_with_semantic(stream: &SegmentTree, units: &UnitRange, semantic: NodeSemantic, children: Vec<Node>) -> Node { Node::with_semantic(unit_span(stream, units), None, semantic, children) }

pub(super) fn leaf_from_unit(stream: &SegmentTree, idx: usize) -> Node {
    let unit = &structure::parser_api::units(stream)[idx];
    let kind = unit.kind().node_kind();
    let semantic = stream.iter_nodes().find(|node| node.kind() == kind && node.span() == unit.span()).and_then(|node| structure::parser_api::node_semantic(node).cloned());
    let tag_index = unit.tag_index().map(|idx| u16::try_from(idx).expect("tag index exceeds u16 range"));
    semantic.map(|semantic| { assert_eq!(semantic.node_kind(), kind, "unit kind must agree with semantic payload"); Node::with_semantic(unit.span(), tag_index, semantic, Vec::new()) }).unwrap_or_else(|| Node::new(kind, unit.span(), tag_index, Vec::new()))
}
