use crate::document::{BoxType, Children, DocumentState, InlineKind};
use crate::text::glyph_cache::GlyphCache;
use std::ops::Range;

pub(crate) struct ListMarkerHelper<'a> {
    doc: &'a DocumentState,
    cache: &'a GlyphCache,
}

impl<'a> ListMarkerHelper<'a> {
    pub(crate) fn new(doc: &'a DocumentState, cache: &'a GlyphCache) -> Self {
        Self { doc, cache }
    }

    pub(crate) fn width_for_box(&self, box_idx: usize) -> f64 {
        if self.has_table_ancestor(box_idx) {
            return 0.0;
        }
        if !self.has_list_ancestor(box_idx) {
            return 0.0;
        }
        let run_range = match self.doc.box_layout_mode(box_idx) {
            Some(BoxType::Block(block)) => match &block.children {
                Children::Runs(range) => Some(range.clone()),
                _ => None,
            },
            Some(BoxType::TableCell(cell)) => match &cell.children {
                Children::Runs(range) => Some(range.clone()),
                _ => None,
            },
            Some(BoxType::Inline(range) | BoxType::Anonymous(range)) => Some(range.clone()),
            Some(BoxType::Table(_) | BoxType::TableRow(_)) | None => None,
        };
        let Some(run_range) = run_range else {
            return 0.0;
        };
        self.marker_width_from_runs(run_range)
    }

    fn marker_width_from_runs(&self, run_range: Range<u32>) -> f64 {
        let first_run = match self.doc.inline_run(run_range.start as usize) {
            Some(run) => run,
            None => return 0.0,
        };
        let InlineKind::Text { glyphs } = &first_run.kind else {
            return 0.0;
        };
        if glyphs.start >= glyphs.end {
            return 0.0;
        }
        let marker_chars: Vec<_> = glyphs.clone().filter_map(|i| self.doc.glyph(i)).map(|glyph| self.cache.get_char(glyph)).collect();
        if marker_chars.is_empty() {
            return 0.0;
        }
        if !marker_chars.last().map(|c| *c == ' ').unwrap_or(false) {
            return 0.0;
        }
        if marker_chars.iter().any(|c| c.is_alphabetic()) {
            return 0.0;
        }
        for c in &marker_chars {
            if !(c.is_ascii_digit() || *c == '-' || *c == '.' || *c == ' ') {
                return 0.0;
            }
        }
        glyphs.clone().filter_map(|i| self.doc.glyph(i)).map(|glyph| self.cache.get(glyph).size().width).sum()
    }

    fn has_list_ancestor(&self, box_idx: usize) -> bool {
        let mut current = Some(box_idx);
        while let Some(idx) = current {
            if self.doc.get_tag(idx).eq_ignore_ascii_case("li") {
                return true;
            }
            current = self.doc.get_parent(idx);
        }
        false
    }

    fn has_table_ancestor(&self, box_idx: usize) -> bool {
        let mut current = Some(box_idx);
        while let Some(idx) = current {
            let tag = self.doc.get_tag(idx);
            if ["table", "tr", "td", "th"].iter().any(|t| tag.eq_ignore_ascii_case(t)) {
                return true;
            }
            current = self.doc.get_parent(idx);
        }
        false
    }
}

pub(crate) fn list_marker_width(doc: &DocumentState, cache: &GlyphCache, box_idx: usize) -> f64 {
    ListMarkerHelper::new(doc, cache).width_for_box(box_idx)
}
