use super::*;

pub(super) use crate::stream_util::{contiguous_slice_end, unit_range_text};

pub(super) struct BoundedToken<'a> {
    stream: &'a SegmentTree,
    text: &'a str,
    units: Range<usize>,
}

impl<'a> BoundedToken<'a> {
    pub(super) fn new(stream: &'a SegmentTree, start_idx: usize) -> Option<Self> {
        let first = structure::parser_api::units(stream).get(start_idx)?;
        if first.kind() == UnitKind::Whitespace || first.is_tag() { return None; }
        Self::from_range(stream, start_idx..contiguous_slice_end(structure::parser_api::units(stream), start_idx))
    }
    pub(super) fn from_range(stream: &'a SegmentTree, units: Range<usize>) -> Option<Self> {
        (units.start < units.end).then_some(())?;
        Some(Self { stream, text: unit_range_text(stream, units.start, units.end)?, units })
    }
    pub(super) fn text(&self) -> &'a str { self.text }
    pub(super) fn end_index(&self) -> usize { self.units.end }
    pub(super) fn prefix_through_byte(&self, byte_end: usize) -> Option<Self> {
        let start = structure::parser_api::unit_start(self.stream, self.units.start)?;
        let end = start.checked_add(byte_end)?;
        let unit_end = (self.units.start..self.units.end).find(|index| structure::parser_api::unit_end(self.stream, *index) == Some(end))? + 1;
        Self::from_range(self.stream, self.units.start..unit_end)
    }
    pub(super) fn trimmed(&self) -> Option<Self> { self.trimmed_with(|_, trailing| trailing.as_bytes().last().copied().is_some_and(is_trim_punct)) }
    pub(super) fn before_first_apostrophe(&self) -> Option<Self> {
        let end = (self.units.start..self.units.end)
            .find(|index| matches!(structure::parser_api::unit_text(self.stream, *index), Some("'" | "’")))
            .unwrap_or(self.units.end);
        Self::from_range(self.stream, self.units.start..end)
    }
    pub(super) fn trimmed_with(&self, accept: impl Fn(&str, &str) -> bool) -> Option<Self> {
        let mut end = self.units.end;
        while end > self.units.start {
            let trailing = structure::parser_api::unit_text(self.stream, end - 1)?;
            let text = unit_range_text(self.stream, self.units.start, end)?;
            if structure::parser_api::units(self.stream)[end - 1].kind() != UnitKind::Punct || !accept(text, trailing) { break; }
            end -= 1;
        }
        Self::from_range(self.stream, self.units.start..end)
    }
    pub(super) fn detected_if(&self, kind: NodeKind, accept: impl Fn(&str) -> bool) -> Option<Detected> { accept(self.text).then(|| Detected::simple(kind, UnitRange::new(self.stream, self.units.clone()))) }
    pub(super) fn protected_if(&self, node_kind: NodeKind, accept: impl Fn(&str) -> bool) -> Option<Detection> { self.detected_if(node_kind, accept).map(Detection::protected) }
}

pub(super) fn numeric_unit_value_with_end(stream: &SegmentTree, idx: usize) -> Option<(usize, u32)> {
    let unit = structure::parser_api::units(stream).get(idx)?;
    if unit.kind() != UnitKind::Numeric || unit.is_tag() { return None; }
    let end = visible_unit_run_end(stream, idx, UnitKind::Numeric);
    Some((end, unit_range_text(stream, idx, end)?.parse::<u32>().ok()?))
}

pub(super) fn unit_range_for_span(stream: &SegmentTree, start_idx: usize, span: &Range<usize>) -> Option<Range<usize>> {
    let mut start = None;
    let mut end = None;
    for (idx, unit) in structure::parser_api::units(stream).iter().enumerate().skip(start_idx) {
        if unit.is_tag() {
            continue;
        }
        let unit_span = unit.span();
        if unit_span.end <= span.start {
            continue;
        }
        start.get_or_insert(idx);
        if unit_span.end >= span.end {
            end = Some(idx + 1);
            break;
        }
    }
    start.zip(end).map(|(start, end)| start..end)
}


pub(super) fn skip_layout_stream_units(stream: &SegmentTree, mut index: usize) -> usize {
    while structure::parser_api::units(stream).get(index).is_some_and(|unit| unit.kind() == UnitKind::Whitespace || unit.is_tag()) {
        index += 1;
    }
    index
}

pub(super) fn skip_tag_stream_units(stream: &SegmentTree, mut index: usize) -> usize {
    while structure::parser_api::units(stream).get(index).is_some_and(|unit| unit.is_tag()) {
        index += 1;
    }
    index
}

pub(super) fn visible_stream_unit_after(stream: &SegmentTree, index: usize) -> Option<usize> {
    let index = skip_tag_stream_units(stream, index.checked_add(1)?);
    structure::parser_api::units(stream).get(index).map(|_| index)
}

pub(super) fn visible_stream_unit_before(stream: &SegmentTree, index: usize) -> Option<usize> {
    (0..index).rev().find(|index| structure::parser_api::units(stream).get(*index).is_some_and(|unit| !unit.is_tag()))
}

pub(super) fn visible_unit_run_end(stream: &SegmentTree, start_idx: usize, kind: UnitKind) -> usize {
    let units = structure::parser_api::units(stream);
    let mut index = start_idx;
    let mut end = start_idx;
    while units.get(index).is_some_and(|unit| !unit.is_tag() && unit.kind() == kind) {
        end = index + 1;
        index = skip_tag_stream_units(stream, end);
    }
    end
}

pub(super) fn stream_unit_range_has_whitespace(stream: &SegmentTree, range: Range<usize>) -> bool {
    structure::parser_api::units(stream).get(range).is_some_and(|units| units.iter().any(|unit| unit.kind() == UnitKind::Whitespace && !unit.is_tag()))
}

pub(super) fn ascii_digits(text: &str) -> bool { text.bytes().all(|b| b.is_ascii_digit()) }
