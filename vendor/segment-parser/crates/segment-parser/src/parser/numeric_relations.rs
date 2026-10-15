use super::*;

pub(super) fn annotate_numeric_relations(context: &ParseContext<'_>, nodes: &mut Vec<Node>) {
    wrap_number_ranges(context, nodes);
}

fn wrap_number_ranges(context: &ParseContext<'_>, nodes: &mut Vec<Node>) {
    let mut index = 0usize;
    while index < nodes.len() {
        let Some((separator_index, right_index, separator)) = number_range_parts(context, nodes, index) else {
            index += 1;
            continue;
        };
        let start_value = nodes[index].number_values()[0];
        let end_value = nodes[right_index].number_values()[0];
        let span = nodes[index].start()..nodes[right_index].end();
        let typography = number_range_typography(context.stream(), &span, &nodes[index], &nodes[separator_index], &nodes[right_index], separator);
        let children = nodes.drain(index..=right_index).collect::<Vec<_>>();
        nodes.insert(index, Node::with_semantic(span, None, NodeSemantic::NumberRange(NumberRangeSemantic::new(start_value, end_value, typography)), children));
        index += 1;
    }
}

fn number_range_parts(context: &ParseContext<'_>, nodes: &[Node], left_index: usize) -> Option<(usize, usize, char)> {
    let stream = context.stream();
    let left = nodes.get(left_index)?;
    if left.kind() != NodeKind::Number || left.number_values().len() != 1 {
        return None;
    }
    let separator_index = next_non_layout_node(nodes, left_index + 1)?;
    let separator = single_character(stream, nodes.get(separator_index)?)?;
    if !context.profile().range_separators.contains_character(separator) {
        return None;
    }
    let right_index = next_non_layout_node(nodes, separator_index + 1)?;
    let right = nodes.get(right_index)?;
    if right.kind() != NodeKind::Number
        || right.number_values().len() != 1
        || !compact_range_is_confident(stream, nodes, left_index, separator_index, right_index, separator)
    {
        return None;
    }
    Some((separator_index, right_index, separator))
}

fn compact_range_is_confident(stream: &SegmentTree, nodes: &[Node], left_index: usize, separator_index: usize, right_index: usize, separator_character: char) -> bool {
    let left = &nodes[left_index];
    let separator = &nodes[separator_index];
    let right = &nodes[right_index];
    let has_spacing = left.end() < separator.start() || separator.end() < right.start();
    if has_spacing {
        return matches!(separator_character, '~' | '～');
    }
    if !matches!(separator_character, '-' | '‐' | '‑') {
        return true;
    }
    if number_has_decimal_separator(stream, left) || number_has_decimal_separator(stream, right) || adjacent_mathematical_operator(stream, nodes, left_index, right_index) {
        return false;
    }
    let digit_count = |node: &Node| node_text(stream, node).chars().filter(char::is_ascii_digit).count();
    digit_count(left) != 3 || digit_count(right) != 3
}

fn number_has_decimal_separator(stream: &SegmentTree, node: &Node) -> bool { node_text(stream, node).chars().any(|character| matches!(character, '.' | ',')) }

fn adjacent_mathematical_operator(stream: &SegmentTree, nodes: &[Node], left_index: usize, right_index: usize) -> bool {
    previous_non_layout_node(nodes, left_index)
        .into_iter()
        .chain(next_non_layout_node(nodes, right_index + 1))
        .any(|index| matches!(node_text(stream, &nodes[index]), "+" | "-" | "−" | "*" | "×" | "/" | "÷" | "="))
}

fn number_range_typography(stream: &SegmentTree, parent_span: &Range<usize>, left: &Node, separator: &Node, right: &Node, separator_character: char) -> NumberRangeTypography {
    NumberRangeTypography::new(
        u16::try_from(left.end().saturating_sub(parent_span.start)).unwrap_or(u16::MAX),
        u16::try_from(right.start().saturating_sub(parent_span.start)).unwrap_or(u16::MAX),
        separator_character,
        typography_spacing(stream.text().get(left.end()..separator.start()).unwrap_or("")),
        typography_spacing(stream.text().get(separator.end()..right.start()).unwrap_or("")),
        '–',
    )
}

fn node_text<'a>(stream: &'a SegmentTree, node: &Node) -> &'a str { stream.text().get(node.span()).unwrap_or("") }

fn single_character(stream: &SegmentTree, node: &Node) -> Option<char> {
    let mut characters = node_text(stream, node).chars();
    let first = characters.next()?;
    characters.next().is_none().then_some(first)
}

pub(super) fn typography_spacing(text: &str) -> NumericTypographySpacing {
    if text.is_empty() {
        return NumericTypographySpacing::Attached;
    }
    if text.chars().all(|character| matches!(character, '\u{00A0}' | '\u{202F}')) {
        return NumericTypographySpacing::NonBreaking;
    }
    if text.chars().all(|character| matches!(character, ' ' | '\t')) {
        return NumericTypographySpacing::Breaking;
    }
    NumericTypographySpacing::Other
}
