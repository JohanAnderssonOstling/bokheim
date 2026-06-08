use crate::document::{Document, DocumentState, GlyphOffsetRun, LayoutCache, Line, StyleView};
use crate::text::glyph_cache::GlyphCache;
use floem::kurbo::Point;
use std::mem;

/// Get computed style for a box using split style vecs
/// Returns a StyleView with references into the document's style storage
pub(crate) fn get_style<'a>(doc: &'a DocumentState, box_idx: usize) -> StyleView<'a> {
    let indices = doc.box_style_indices(box_idx).unwrap_or_else(|| {
        // Fallback for boxes without split styles (shouldn't happen after migration)
        // Use default indices (index 0 guaranteed to exist from ensure_default_styles)
        crate::document::StyleIndices { font_idx: 0, text_idx: 0, box_idx: 0, border_idx: 0, bg_idx: 0 }
    });

    doc.style_view(indices)
}

pub struct LayoutEngine {
    pub viewport_width: f64,
    pub line_height: f64,
    pub force_justify: bool,
    pub(crate) document: DocumentState,
    pub(crate) glyph_cache: GlyphCache,
}

impl LayoutEngine {
    pub fn new(viewport_width: f64, line_height: f64) -> Self {
        Self { viewport_width, line_height, force_justify: false, document: DocumentState::new(Document::new(), Default::default()), glyph_cache: GlyphCache::new() }
    }

    pub fn layout(&mut self, document: DocumentState, glyph_cache: GlyphCache) -> (DocumentState, GlyphCache) {
        self.document = document;
        self.glyph_cache = glyph_cache;
        self.clear_layout_output();

        if let Some(root_box) = self.document.root_box() {
            self.document.set_box_point(root_box, Point::ZERO);
            self.layout_box(root_box, self.viewport_width, None, 0.0);
        }
        self.sort_lines_and_remap_images();
        self.collect_inline_decorations();
        self.rebuild_image_fragments_by_line();
        (mem::take(&mut self.document), mem::take(&mut self.glyph_cache))
    }

    pub(crate) fn layout_cache(&self) -> &LayoutCache {
        self.document.layout()
    }

    pub(crate) fn layout_cache_mut(&mut self) -> &mut LayoutCache {
        self.document.layout_mut()
    }

    fn clear_layout_output(&mut self) {
        let layout = self.layout_cache_mut();
        layout.lines.clear();
        layout.line_glyph_offsets.clear();
        layout.decorations.clear();
        layout.image_fragments.clear();
        layout.image_fragments_by_line.clear();
    }

    fn sort_lines_and_remap_images(&mut self) {
        let layout = self.layout_cache_mut();
        let mut offsets_by_line: Vec<Vec<GlyphOffsetRun>> = layout.line_glyph_offsets.drain(..).collect();
        let mut line_pairs: Vec<(Line, Vec<GlyphOffsetRun>, usize)> = layout.lines.drain(..).enumerate().map(|(idx, line)| (line, offsets_by_line.get_mut(idx).map(mem::take).unwrap_or_default(), idx)).collect();

        line_pairs.sort_by(|(a, _, _), (b, _, _)| a.point.y.partial_cmp(&b.point.y).unwrap_or(std::cmp::Ordering::Equal).then_with(|| a.point.x.partial_cmp(&b.point.x).unwrap_or(std::cmp::Ordering::Equal)));

        let mut index_map = vec![0usize; line_pairs.len()];
        for (new_idx, (_, _, old_idx)) in line_pairs.iter().enumerate() {
            index_map[*old_idx] = new_idx;
        }
        layout.lines = line_pairs.iter().map(|(line, _, _)| line.clone()).collect();
        layout.line_glyph_offsets = line_pairs.into_iter().map(|(_, offsets, _)| offsets).collect();
        for frag in &mut layout.image_fragments {
            if let Some(&new_idx) = index_map.get(frag.line_idx) {
                frag.line_idx = new_idx;
            }
        }
    }

    fn rebuild_image_fragments_by_line(&mut self) {
        let layout = self.layout_cache_mut();
        layout.image_fragments_by_line = vec![Vec::new(); layout.lines.len()];
        for (idx, frag) in layout.image_fragments.iter().enumerate() {
            if let Some(line) = layout.image_fragments_by_line.get_mut(frag.line_idx) {
                line.push(idx);
            }
        }
    }
}
