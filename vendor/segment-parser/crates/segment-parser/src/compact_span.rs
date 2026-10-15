use std::ops::Range;

use structure::{SegmentTree, TokenUnitId};

/// A text span stored as two `u16` offsets so that facts stay small.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompactSpan {
    start: u16,
    end: u16,
}

impl CompactSpan {
    pub fn new(span: Range<usize>, description: &str) -> Self { Self { start: compact_offset(span.start, description), end: compact_offset(span.end, description) } }
    pub fn range(self) -> Range<usize> { self.start as usize..self.end as usize }
    pub fn text(self, tree: &SegmentTree) -> &str { tree.text().get(self.range()).unwrap_or("") }
    pub fn token_unit_ids(self, tree: &SegmentTree) -> Vec<TokenUnitId> { tree.token_unit_ids_for_span(self.range()) }
}

pub fn compact_offset(offset: usize, description: &str) -> u16 { u16::try_from(offset).unwrap_or_else(|_| panic!("{description} exceeds u16 range")) }
