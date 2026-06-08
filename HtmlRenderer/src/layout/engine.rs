use crate::document::{BorderStyle, BoxType, Children, DecorationFragment, Document, GlyphOffsetRun, InlineKind, Line, StyleView};
use crate::text::glyph_cache::GlyphCache;
use floem::kurbo::{Point, Rect};
use std::mem;
use std::ops::Range;

/// Get computed style for a box using split style vecs
/// Returns a StyleView with references into the document's style storage
pub(crate) fn get_style<'a>(doc: &'a Document, box_idx: usize) -> StyleView<'a> {
    let indices = doc.box_tree.boxes[box_idx].style.unwrap_or_else(|| {
        // Fallback for boxes without split styles (shouldn't happen after migration)
        // Use default indices (index 0 guaranteed to exist from ensure_default_styles)
        crate::document::StyleIndices { font_idx: 0, text_idx: 0, box_idx: 0, border_idx: 0, bg_idx: 0 }
    });

    doc.styles.view(indices)
}

fn push_border_rects(doc: &mut Document, rect: Rect, border_width: f64, color: u32, is_inline: bool) {
    let bw = border_width.min(rect.width().min(rect.height()));
    let x0 = rect.x0;
    let x1 = rect.x1;
    let y0 = rect.y0;
    let y1 = rect.y1;
    let top = Rect::new(x0, y0, x1, y0 + bw);
    let bottom = Rect::new(x0, y1 - bw, x1, y1);
    let left = Rect::new(x0, y0, x0 + bw, y1);
    let right = Rect::new(x1 - bw, y0, x1, y1);
    for rect in [top, right, bottom, left] {
        doc.layout.decorations.push(DecorationFragment { rect, color, is_inline });
    }
}

fn uniform_solid_border_width(style: &StyleView) -> f64 {
    matches!(style.border_top_style(), BorderStyle::Solid).then_some(style.border_top_width() as f64).unwrap_or(0.0)
}

fn paint_block_borders(doc: &mut Document, rect: Rect, top: Option<(f64, u32)>, right: Option<(f64, u32)>, bottom: Option<(f64, u32)>, left: Option<(f64, u32)>) {
    if let Some((w, c)) = top {
        let h = w.min(rect.height());
        if h > 0.0 {
            doc.layout.decorations.push(DecorationFragment { rect: Rect::new(rect.x0, rect.y0, rect.x1, rect.y0 + h), color: c, is_inline: false });
        }
    }
    if let Some((w, c)) = right {
        let bw = w.min(rect.width());
        if bw > 0.0 {
            doc.layout.decorations.push(DecorationFragment { rect: Rect::new(rect.x1 - bw, rect.y0, rect.x1, rect.y1), color: c, is_inline: false });
        }
    }
    if let Some((w, c)) = bottom {
        let h = w.min(rect.height());
        if h > 0.0 {
            doc.layout.decorations.push(DecorationFragment { rect: Rect::new(rect.x0, rect.y1 - h, rect.x1, rect.y1), color: c, is_inline: false });
        }
    }
    if let Some((w, c)) = left {
        let bw = w.min(rect.width());
        if bw > 0.0 {
            doc.layout.decorations.push(DecorationFragment { rect: Rect::new(rect.x0, rect.y0, rect.x0 + bw, rect.y1), color: c, is_inline: false });
        }
    }
}

pub(crate) struct ListMarkerHelper<'a> {
    doc: &'a Document,
    cache: &'a GlyphCache,
}

impl<'a> ListMarkerHelper<'a> {
    pub(crate) fn new(doc: &'a Document, cache: &'a GlyphCache) -> Self {
        Self { doc, cache }
    }

    pub(crate) fn width_for_box(&self, box_idx: usize) -> f64 {
        if self.has_table_ancestor(box_idx) {
            return 0.0;
        }
        if !self.has_list_ancestor(box_idx) {
            return 0.0;
        }
        let run_range = match &self.doc.box_tree.boxes[box_idx].layout_mode {
            BoxType::Block(block) => match &block.children {
                Children::Runs(range) => Some(range.clone()),
                _ => None,
            },
            BoxType::TableCell(cell) => match &cell.children {
                Children::Runs(range) => Some(range.clone()),
                _ => None,
            },
            BoxType::Inline(range) | BoxType::Anonymous(range) => Some(range.clone()),
            BoxType::Table(_) | BoxType::TableRow(_) => None,
        };
        let Some(run_range) = run_range else {
            return 0.0;
        };
        self.marker_width_from_runs(run_range)
    }

    fn marker_width_from_runs(&self, run_range: Range<u32>) -> f64 {
        let first_run = match self.doc.box_tree.inline_runs.get(run_range.start as usize) {
            Some(run) => run,
            None => return 0.0,
        };
        let InlineKind::Text { glyphs } = &first_run.kind else {
            return 0.0;
        };
        if glyphs.start >= glyphs.end {
            return 0.0;
        }
        let marker_chars: Vec<_> = glyphs.clone().map(|i| self.cache.get_char(self.doc.box_tree.glyphs[i as usize])).collect();
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
        glyphs.clone().map(|i| self.cache.get(self.doc.box_tree.glyphs[i as usize]).size().width).sum()
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
            if self.doc.get_tag(idx).eq_ignore_ascii_case("table") || self.doc.get_tag(idx).eq_ignore_ascii_case("tr") || self.doc.get_tag(idx).eq_ignore_ascii_case("td") || self.doc.get_tag(idx).eq_ignore_ascii_case("th") {
                return true;
            }
            current = self.doc.get_parent(idx);
        }
        false
    }
}

pub(crate) fn list_marker_width(doc: &Document, cache: &GlyphCache, box_idx: usize) -> f64 {
    ListMarkerHelper::new(doc, cache).width_for_box(box_idx)
}

pub struct LayoutEngine {
    pub viewport_width: f64,
    pub line_height: f64,
    pub force_justify: bool,
    pub(crate) document: Document,
    pub(crate) glyph_cache: GlyphCache,
}

impl LayoutEngine {
    pub fn new(viewport_width: f64, line_height: f64) -> Self {
        Self { viewport_width, line_height, force_justify: false, document: Document::new(), glyph_cache: GlyphCache::new() }
    }

    pub fn layout(&mut self, document: Document, glyph_cache: GlyphCache) -> (Document, GlyphCache) {
        self.document = document;
        self.glyph_cache = glyph_cache;
        self.clear_layout_output();

        if let Some(root_box) = self.document.box_tree.root_box {
            self.document.box_tree.boxes[root_box as usize].point = Point::ZERO;
            self.layout_box(root_box as usize, self.viewport_width, None, 0.0);
        }
        self.sort_lines_and_remap_images();
        self.collect_inline_decorations();
        self.rebuild_image_fragments_by_line();
        (mem::take(&mut self.document), mem::take(&mut self.glyph_cache))
    }

    fn clear_layout_output(&mut self) {
        self.document.layout.lines.clear();
        self.document.layout.line_glyph_offsets.clear();
        self.document.layout.decorations.clear();
        self.document.layout.image_fragments.clear();
        self.document.layout.image_fragments_by_line.clear();
    }

    fn sort_lines_and_remap_images(&mut self) {
        let mut offsets_by_line: Vec<Vec<GlyphOffsetRun>> = self.document.layout.line_glyph_offsets.drain(..).collect();
        let mut line_pairs: Vec<(Line, Vec<GlyphOffsetRun>, usize)> = self.document.layout.lines.drain(..).enumerate().map(|(idx, line)| (line, offsets_by_line.get_mut(idx).map(mem::take).unwrap_or_default(), idx)).collect();

        line_pairs.sort_by(|(a, _, _), (b, _, _)| a.point.y.partial_cmp(&b.point.y).unwrap_or(std::cmp::Ordering::Equal).then_with(|| a.point.x.partial_cmp(&b.point.x).unwrap_or(std::cmp::Ordering::Equal)));

        let mut index_map = vec![0usize; line_pairs.len()];
        for (new_idx, (_, _, old_idx)) in line_pairs.iter().enumerate() {
            index_map[*old_idx] = new_idx;
        }
        self.document.layout.lines = line_pairs.iter().map(|(line, _, _)| line.clone()).collect();
        self.document.layout.line_glyph_offsets = line_pairs.into_iter().map(|(_, offsets, _)| offsets).collect();
        for frag in &mut self.document.layout.image_fragments {
            if let Some(&new_idx) = index_map.get(frag.line_idx) {
                frag.line_idx = new_idx;
            }
        }
    }

    fn rebuild_image_fragments_by_line(&mut self) {
        self.document.layout.image_fragments_by_line = vec![Vec::new(); self.document.layout.lines.len()];
        for (idx, frag) in self.document.layout.image_fragments.iter().enumerate() {
            if let Some(line) = self.document.layout.image_fragments_by_line.get_mut(frag.line_idx) {
                line.push(idx);
            }
        }
    }

    pub(crate) fn emit_block_background(&mut self, box_idx: usize) {
        let style = get_style(&self.document, box_idx);
        let background_color = style.background_color();
        if background_color & 0xFF == 0 {
            return;
        }

        let margin_left = style.margin_left() as f64;
        let margin_right = style.margin_right() as f64;
        let margin_top = style.margin_top() as f64;
        let margin_bottom = style.margin_bottom() as f64;

        let b = &self.document.box_tree.boxes[box_idx];
        let width = (b.size.width - margin_left - margin_right).max(0.0);
        let height = (b.size.height - margin_top - margin_bottom).max(0.0);
        let rect = Rect::new(b.point.x, b.point.y, b.point.x + width, b.point.y + height);
        self.document.layout.decorations.push(DecorationFragment { rect, color: background_color, is_inline: false });
    }

    pub(crate) fn emit_block_decorations(&mut self, box_idx: usize) {
        // Extract all style values before mutable operations
        let style = get_style(&self.document, box_idx);
        let border_width = uniform_solid_border_width(&style);
        let top_border = matches!(style.border_top_style(), BorderStyle::Solid).then_some((style.border_top_width() as f64, style.border_top_color()));
        let right_border = matches!(style.border_right_style(), BorderStyle::Solid).then_some((style.border_right_width() as f64, style.border_right_color()));
        let bottom_border = matches!(style.border_bottom_style(), BorderStyle::Solid).then_some((style.border_bottom_width() as f64, style.border_bottom_color()));
        let left_border = matches!(style.border_left_style(), BorderStyle::Solid).then_some((style.border_left_width() as f64, style.border_left_color()));
        let has_any_border = top_border.is_some() || right_border.is_some() || bottom_border.is_some() || left_border.is_some();
        let margin_left = style.margin_left() as f64;
        let margin_right = style.margin_right() as f64;
        let margin_top = style.margin_top() as f64;
        let margin_bottom = style.margin_bottom() as f64;

        let b = &self.document.box_tree.boxes[box_idx];
        let width = (b.size.width - margin_left - margin_right).max(0.0);
        let height = (b.size.height - margin_top - margin_bottom).max(0.0);

        let x0 = b.point.x;
        let y0 = b.point.y;
        let x1 = x0 + width;
        let y1 = y0 + height;
        let rect = Rect::new(x0, y0, x1, y1);

        self.emit_block_background(box_idx);

        if border_width > 0.0 || has_any_border {
            paint_block_borders(&mut self.document, rect, top_border, right_border, bottom_border, left_border);
        }
    }

    fn collect_inline_decorations(&mut self) {
        if self.document.layout.lines.is_empty() || self.document.box_tree.inline_runs.is_empty() {
            return;
        }

        // Ensure the shared glyph→box map is current (rebuilds only if stale).
        self.document.box_tree.glyph_box_map();

        let inline_ancestors = self.build_inline_ancestor_map();
        let mut active = vec![false; self.document.box_tree.boxes.len()];
        let mut active_start = vec![0.0f64; self.document.box_tree.boxes.len()];
        let mut active_indices: Vec<u32> = Vec::new();
        let mut marked = vec![false; self.document.box_tree.boxes.len()];

        for line in self.document.layout.lines.clone() {
            for &b in &active_indices {
                active[b as usize] = false;
            }
            active_indices.clear();

            let mut x = line.point.x;
            for i in line.glyphs.clone() {
                let box_idx = self.document.box_tree.glyph_box[i as usize] as usize;
                let boxes = &inline_ancestors[box_idx];

                for &b in boxes {
                    let b_idx = b as usize;
                    marked[b_idx] = true;
                    if !active[b_idx] {
                        active[b_idx] = true;
                        active_start[b_idx] = x;
                        active_indices.push(b);
                    }
                }

                if !active_indices.is_empty() {
                    let mut next_active: Vec<u32> = Vec::with_capacity(active_indices.len());
                    for &b in &active_indices {
                        let b_idx = b as usize;
                        if marked[b_idx] {
                            next_active.push(b);
                        } else {
                            self.emit_inline_segment(b_idx, &line, active_start[b_idx], x);
                            active[b_idx] = false;
                        }
                    }
                    active_indices = next_active;
                }

                for &b in boxes {
                    marked[b as usize] = false;
                }

                let glyph_index = self.document.box_tree.glyphs[i as usize];
                let layout = self.glyph_cache.get(glyph_index);
                x += layout.size().width;
            }

            for &b in &active_indices {
                let b_idx = b as usize;
                self.emit_inline_segment(b_idx, &line, active_start[b_idx], x);
                active[b_idx] = false;
            }
            active_indices.clear();
        }
    }

    fn build_inline_ancestor_map(&self) -> Vec<Vec<u32>> {
        let mut result = vec![Vec::new(); self.document.box_tree.boxes.len()];
        for i in 0..self.document.box_tree.boxes.len() {
            let mut current = Some(i);
            let mut ancestors = Vec::new();
            while let Some(idx) = current {
                if matches!(&self.document.box_tree.boxes[idx].layout_mode, BoxType::Inline(_)) {
                    ancestors.push(idx as u32);
                }
                current = self.document.box_tree.boxes[idx].parent.map(|p| p as usize);
            }
            result[i] = ancestors;
        }
        result
    }

    fn emit_inline_segment(&mut self, box_idx: usize, line: &Line, start_x: f64, end_x: f64) {
        if start_x >= end_x {
            return;
        }
        if !matches!(&self.document.box_tree.boxes[box_idx].layout_mode, BoxType::Inline(_)) {
            return;
        }

        // Extract all style values before mutable operations
        let style = get_style(&self.document, box_idx);
        let background_color = style.background_color();
        let color_alpha = background_color & 0xFF;
        let border_width = uniform_solid_border_width(&style);
        let pad_left = style.padding_left() as f64;
        let pad_right = style.padding_right() as f64;
        let pad_top = style.padding_top() as f64;
        let pad_bottom = style.padding_bottom() as f64;
        let border_top_color = style.border_top_color();

        if color_alpha == 0 && border_width <= 0.0 {
            return;
        }

        let x0 = start_x - pad_left - border_width;
        let x1 = end_x + pad_right + border_width;
        let y0 = line.point.y - pad_top - border_width;
        let y1 = line.point.y + line.height + pad_bottom + border_width;
        if x1 <= x0 || y1 <= y0 {
            return;
        }

        let rect = Rect::new(x0, y0, x1, y1);
        if color_alpha != 0 {
            self.document.layout.decorations.push(DecorationFragment { rect, color: background_color, is_inline: true });
        }
        if border_width > 0.0 {
            push_border_rects(&mut self.document, rect, border_width, border_top_color, true);
        }
    }
}
