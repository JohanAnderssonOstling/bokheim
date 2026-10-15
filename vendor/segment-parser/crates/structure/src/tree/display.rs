use std::ops::Range;

use super::{SegmentTree, UnitRef};
use super::tag_model::TagNumbering;
use crate::UnitKind;

impl SegmentTree {
    fn display_units(&self) -> impl Iterator<Item = (usize, UnitRef<'_>, Range<usize>, Option<String>)> + '_ {
        let mut numbering = TagNumbering::default();
        let mut display = 0usize;
        self.iter_units().enumerate().map(move |(index, unit)| {
            let label = unit.tag().map(|tag| tag.display_label(numbering.next(tag.id()).0));
            let len = label.as_ref().map(String::len).unwrap_or_else(|| unit.len());
            let span = display..display + len;
            display = span.end;
            (index, unit, span, label)
        })
    }

    pub fn display_span_for_unit_index(&self, unit_index: usize) -> Option<Range<usize>> {
        self.display_units().find_map(|(index, _, span, _)| (index == unit_index).then_some(span))
    }

    pub fn unit_index_and_offset_for_display_pos(&self, display_pos: usize) -> Option<(usize, usize)> {
        self.display_units().find_map(|(index, unit, span, _)| {
            if span.start <= display_pos && display_pos < span.end { return (unit.kind() != UnitKind::Tag).then_some((index, display_pos - span.start)); }
            (display_pos == span.end && unit.kind() != UnitKind::Tag).then_some((index, unit.len()))
        })
    }

    pub fn display_len(&self) -> usize { self.display_units().last().map(|(_, _, span, _)| span.end).unwrap_or(0) }

    pub fn display_runs(&self) -> Vec<(usize, bool)> {
        self.display_units().fold(Vec::new(), |mut runs, (_, unit, span, _)| {
            let is_tag = unit.kind() == UnitKind::Tag;
            let len = span.end - span.start;
            if len == 0 { return runs; }
            if let Some(last) = runs.last_mut() && last.1 == is_tag {
                last.0 += len;
                return runs;
            }
            runs.push((len, is_tag));
            runs
        })
    }

    pub fn display_text(&self) -> String {
        let mut text = String::new();
        self.display_units().for_each(|(_, unit, _, label)| match label {
            Some(label) => text.push_str(&label),
            None => text.push_str(unit.text()),
        });
        text
    }

    pub fn to_xml(&self) -> String {
        let mut output = String::new();
        self.iter_units().for_each(|unit| match unit.kind() {
            UnitKind::Tag => output.push_str(unit.tag().expect("tag unit must carry tag data").raw()),
            _ => escape_xml_into(&mut output, unit.text()),
        });
        output
    }

    pub fn display_pos_for_unit_index(&self, unit_index: usize) -> usize { self.display_span_for_unit_index(unit_index).map(|span| span.start).unwrap_or_else(|| self.display_len()) }

    pub fn unit_index_for_display_pos(&self, display_pos: usize) -> usize {
        self.display_units().find_map(|(unit_index, _, span, _)| {
            if display_pos <= span.start { Some(unit_index) } else if display_pos < span.end { Some(unit_index + 1) } else { None }
        }).unwrap_or_else(|| self.unit_count())
    }

    fn display_pos_for_text_pos(&self, text_pos: usize) -> usize {
        self.display_units().try_fold(0usize, |raw, (_, unit, span, label)| {
            if label.is_some() { return Ok(raw); }
            let next_raw = raw + unit.len();
            if text_pos <= next_raw { Err(span.start + (text_pos - raw).min(unit.len())) } else { Ok(next_raw) }
        }).unwrap_err_or_else(|_| self.display_len())
    }

    pub fn text_pos_from_display_pos(&self, display_pos: usize) -> usize {
        self.display_units().try_fold(0usize, |raw, (_, unit, span, label)| {
            if label.is_some() {
                return if display_pos <= span.end { Err(raw) } else { Ok(raw) };
            }
            if display_pos <= span.end { Err(raw + (display_pos - span.start).min(unit.len())) } else { Ok(raw + unit.len()) }
        }).unwrap_err_or_else(|raw| raw)
    }

    pub fn display_span_for_text_span(&self, span: Range<usize>) -> Range<usize> { self.display_pos_for_text_pos(span.start)..self.display_pos_for_text_pos(span.end) }
    pub fn unit_range_from_display_range(&self, range: Range<usize>) -> Range<usize> { self.unit_index_for_display_pos(range.start)..self.unit_index_for_display_pos(range.end) }

    pub fn tag_span_at_display_pos(&self, pos: usize) -> Option<(usize, Range<usize>)> {
        self.display_units().find_map(|(index, _, span, label)| (label.is_some() && pos >= span.start && pos < span.end).then_some((index, span)))
    }
}

trait TryFoldResultExt<T, E> {
    fn unwrap_err_or_else(self, fallback: impl FnOnce(T) -> E) -> E;
}

impl<T, E> TryFoldResultExt<T, E> for Result<T, E> {
    fn unwrap_err_or_else(self, fallback: impl FnOnce(T) -> E) -> E { match self { Ok(value) => fallback(value), Err(error) => error } }
}

fn escape_xml_into(output: &mut String, text: &str) {
    text.chars().for_each(|character| match character {
        '&' => output.push_str("&amp;"),
        '<' => output.push_str("&lt;"),
        '>' => output.push_str("&gt;"),
        '"' => output.push_str("&quot;"),
        '\'' => output.push_str("&apos;"),
        _ => output.push(character),
    });
}
