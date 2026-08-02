use html_dom::MemoryUsageReport;
use kurbo::{Point, Rect, Size};
use rustc_data_structures::fx::FxHashMap;
use std::ops::Range;

/// Output of the layout phase: line boxes, glyph offsets, and paint fragments.
/// Produced by the layout phase and consumed read-only by the renderer.
#[derive(Default, Clone)]
pub struct LayoutState {
    pub lines: Vec<Line>,
    pub line_glyph_offsets: Vec<Vec<GlyphOffsetRun>>,
    pub line_glyph_advances: Vec<Vec<GlyphAdvanceRun>>,
    pub decorations: Vec<DecorationFragment>,
    pub image_fragments: Vec<ImageFragment>,
    pub image_fragments_by_line: Vec<Vec<usize>>,
    pub anchor_positions: FxHashMap<u16, AnchorPosition>,
}

impl LayoutState {
    pub fn memory_usage_report(&self) -> MemoryUsageReport {
        let mut report = MemoryUsageReport::new();
        report.add_slice_storage::<Line>("LayoutState.lines.storage", self.lines.capacity(), self.lines.len());
        report.add_slice_storage::<Vec<GlyphOffsetRun>>("LayoutState.line_glyph_offsets.storage", self.line_glyph_offsets.capacity(), self.line_glyph_offsets.len());
        for offsets in &self.line_glyph_offsets {
            report.add_slice_storage::<GlyphOffsetRun>("LayoutState.line_glyph_offsets.items.storage", offsets.capacity(), offsets.len());
        }
        report.add_slice_storage::<Vec<GlyphAdvanceRun>>("LayoutState.line_glyph_advances.storage", self.line_glyph_advances.capacity(), self.line_glyph_advances.len());
        for advances in &self.line_glyph_advances {
            report.add_slice_storage::<GlyphAdvanceRun>("LayoutState.line_glyph_advances.items.storage", advances.capacity(), advances.len());
        }
        report.add_slice_storage::<DecorationFragment>("LayoutState.decorations.storage", self.decorations.capacity(), self.decorations.len());
        report.add_slice_storage::<ImageFragment>("LayoutState.image_fragments.storage", self.image_fragments.capacity(), self.image_fragments.len());
        report.add_slice_storage::<Vec<usize>>("LayoutState.image_fragments_by_line.storage", self.image_fragments_by_line.capacity(), self.image_fragments_by_line.len());
        for fragments in &self.image_fragments_by_line {
            report.add_slice_storage::<usize>("LayoutState.image_fragments_by_line.items.storage", fragments.capacity(), fragments.len());
        }
        report.add_slice_storage::<(u16, AnchorPosition)>("LayoutState.anchor_positions.storage", self.anchor_positions.capacity(), self.anchor_positions.len());
        report
    }
}

#[derive(Clone, Copy)]
pub struct AnchorPosition {
    pub y: f64,
    pub order: u32,
}

impl AnchorPosition {
    pub fn memory_usage_report(&self) -> MemoryUsageReport {
        MemoryUsageReport::new()
    }
}

#[derive(Clone)]
pub struct Line {
    pub glyphs: Range<u32>,
    pub point: Point,
    pub height: f64,
    pub baseline: f64,
    pub word_spacing: f64,   // extra space to add after each space (for justification)
    pub letter_spacing: f64, // extra space to add after each letter (for justification)
}

impl Line {
    pub fn memory_usage_report(&self) -> MemoryUsageReport {
        MemoryUsageReport::new()
    }
}

#[derive(Clone)]
pub struct GlyphOffsetRun {
    pub range: Range<u32>,
    pub offset: f32,
}

/// Sparse horizontal advance overrides produced for position-dependent glyphs
/// such as preserved tabs. Ordinary glyphs remain on the metric fast path.
#[derive(Clone)]
pub struct GlyphAdvanceRun {
    pub range: Range<u32>,
    pub advance: f32,
}

impl GlyphAdvanceRun {
    pub fn memory_usage_report(&self) -> MemoryUsageReport {
        MemoryUsageReport::new()
    }
}

impl GlyphOffsetRun {
    pub fn memory_usage_report(&self) -> MemoryUsageReport {
        MemoryUsageReport::new()
    }
}

#[derive(Clone)]
pub struct DecorationFragment {
    pub rect: Rect,
    pub color: u32,
    pub is_inline: bool,
}

impl DecorationFragment {
    pub fn memory_usage_report(&self) -> MemoryUsageReport {
        MemoryUsageReport::new()
    }
}

#[derive(Clone)]
pub struct ImageFragment {
    pub line_idx: usize,
    pub image_idx: u32,
    pub offset: Point,
    pub size: Size,
}

impl ImageFragment {
    pub fn memory_usage_report(&self) -> MemoryUsageReport {
        MemoryUsageReport::new()
    }
}
