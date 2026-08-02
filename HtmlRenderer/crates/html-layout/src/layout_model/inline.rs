use html_dom::MemoryUsageReport;
use std::ops::Range;

/// Width of a single cell in the inline glyph buffer.
///
/// Each cell holds a Unicode scalar value before shaping and a glyph-cache
/// index after shaping (see [`crate::text::glyph_cache`]). Both roles share
/// this one type so the buffer width can be changed in a single place.
///
/// `u32` so the buffer holds full Unicode scalar values (no BMP truncation of
/// emoji / supplementary-plane CJK) and the glyph-cache index has no 65 536-entry
/// ceiling.
pub type GlyphId = u32;

#[derive(Default, Clone)]
pub struct InlineContent {
    pub(super) glyphs: Vec<GlyphId>,
    pub(super) inline_runs: Vec<InlineRun>,
}

impl InlineContent {
    pub fn memory_usage_report(&self) -> MemoryUsageReport {
        let mut report = MemoryUsageReport::new();
        report.add_slice_storage::<GlyphId>("InlineContent.glyphs.storage", self.glyphs.capacity(), self.glyphs.len());
        report.add_slice_storage::<InlineRun>("InlineContent.inline_runs.storage", self.inline_runs.capacity(), self.inline_runs.len());
        report
    }
}

impl InlineContent {
    pub fn glyphs(&self) -> &[GlyphId] {
        self.glyphs.as_slice()
    }

    pub fn glyphs_mut(&mut self) -> &mut [GlyphId] {
        self.glyphs.as_mut_slice()
    }

    pub fn glyph_at(&self, glyph_idx: usize) -> Option<GlyphId> {
        self.glyphs.get(glyph_idx).copied()
    }

    pub fn glyph_slice(&self, range: Range<u32>) -> &[GlyphId] {
        &self.glyphs[range.start as usize..range.end as usize]
    }

    pub fn replace_glyphs(&mut self, glyphs: Vec<GlyphId>) {
        self.glyphs = glyphs;
    }

    pub fn inline_runs(&self) -> &[InlineRun] {
        self.inline_runs.as_slice()
    }

    pub fn inline_runs_mut(&mut self) -> &mut [InlineRun] {
        self.inline_runs.as_mut_slice()
    }

    pub fn inline_run(&self, run_idx: usize) -> Option<&InlineRun> {
        self.inline_runs.get(run_idx)
    }

    pub fn inline_runs_empty(&self) -> bool {
        self.inline_runs.is_empty()
    }

    pub fn push_inline_run(&mut self, run: InlineRun) {
        self.inline_runs.push(run);
    }

    pub fn push_glyph(&mut self, glyph: GlyphId) {
        self.glyphs.push(glyph);
    }
}

#[derive(Clone)]
pub struct InlineRun {
    pub kind: InlineKind,
    pub box_idx: u32,
    pub dom_text_node: Option<u32>, // DOM text node that produced this run (None for image/break)
}

impl InlineRun {
    pub fn memory_usage_report(&self) -> MemoryUsageReport {
        MemoryUsageReport::new()
    }
}

#[derive(Clone)]
pub enum InlineKind {
    Text {
        glyphs: Range<u32>,
    },
    Image {
        image_idx: u32,
    },
    Break,
    FloatAnchor {
        box_idx: u32,
    },
    /// Generated list-item marker text (the `::marker` content). Holds glyphs
    /// like `Text`, but is synthetic: it is excluded from selection, search, and
    /// CFI so it never appears in the document's addressable text, matching how
    /// browsers keep marker content out of the DOM.
    Marker {
        glyphs: Range<u32>,
    },
}

impl InlineKind {
    pub fn memory_usage_report(&self) -> MemoryUsageReport {
        MemoryUsageReport::new()
    }
}
