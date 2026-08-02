use crate::document::{Clear, ComputedStyles, Document, Float, GlyphAdvanceRun, GlyphId, GlyphMetrics, GlyphOffsetRun, InlineContent, InlineKind, InlineRun, LayoutBox, LayoutMode, LayoutState, LayoutTree, Line, StyleIndices, StyleView};
use crate::layout::FloatContext;
use kurbo::Point;
use std::ops::Range;
use std::time::{Duration, Instant};

fn get_style_indices(ctx: &LayoutContext<'_, '_>, box_idx: usize) -> StyleIndices {
    ctx.box_style_indices(box_idx).unwrap_or_else(|| ctx.inputs.styles.default_indices())
}

#[cfg(test)]
pub fn layout(document: &Document, styles: &ComputedStyles, layout_tree: &mut LayoutTree, inline_content: &InlineContent, glyph_metrics: &GlyphMetrics, viewport_width: f64, line_height: f64, out: &mut LayoutState) {
    layout_with_timings(document, styles, layout_tree, inline_content, glyph_metrics, &crate::ImageMetrics::default(), viewport_width, line_height, out, None)
}

pub(crate) fn layout_with_timings(
    document: &Document, styles: &ComputedStyles, layout_tree: &mut LayoutTree, inline_content: &InlineContent, glyph_metrics: &GlyphMetrics, image_metrics: &crate::ImageMetrics, viewport_width: f64, line_height: f64,
    out: &mut LayoutState, timings: Option<&mut LayoutTimings>,
) {
    let inputs = LayoutInputs { document, styles, inline_content, glyph_metrics, image_metrics };
    let outputs = LayoutOutputs { layout_tree, state: out };
    let timings = timings.map(|timings| timings as *mut LayoutTimings);
    let mut context = LayoutContext { viewport_width, line_height, force_justify: false, float_stack: Vec::new(), inputs, outputs, timings };
    context.run()
}

pub(crate) struct LayoutInputs<'a> {
    document: &'a Document,
    styles: &'a ComputedStyles,
    inline_content: &'a InlineContent,
    glyph_metrics: &'a GlyphMetrics,
    image_metrics: &'a crate::ImageMetrics,
}

pub(crate) struct LayoutOutputs<'out> {
    layout_tree: &'out mut LayoutTree,
    state: &'out mut LayoutState,
}

#[derive(Default, Debug, Clone)]
pub struct LayoutTimings {
    pub clear_layout_output: Duration,
    pub layout_tree_traversal: Duration,
    /// Inclusive wall time for the root box and every descendant it lays out.
    pub root_box_layout: Duration,
    /// Inclusive wall time for line sorting, decoration collection, and image indexing.
    pub finalize_layout: Duration,
    pub sort_lines_and_remap_images: Duration,
    pub collect_inline_decorations: Duration,
    pub rebuild_image_fragments_by_line: Duration,
    pub layout_box_total: Duration,
    pub layout_block_children: Duration,
    pub layout_table: Duration,
    pub layout_table_row: Duration,
    pub layout_runs: Duration,
    pub layout_runs_with_floats: Duration,
    pub build_inline_tokens: Duration,
    pub build_inline_tokens_from_runs: Duration,
    pub break_lines: Duration,
    pub break_lines_knuth: Duration,
    pub emit_lines: Duration,
    pub measure_line: Duration,
    pub write_line_fragments: Duration,
    pub place_float_anchor: Duration,
}

pub(crate) struct LayoutContext<'a, 'out> {
    pub viewport_width: f64,
    pub line_height: f64,
    pub force_justify: bool,
    pub(crate) float_stack: Vec<FloatContext>,
    inputs: LayoutInputs<'a>,
    outputs: LayoutOutputs<'out>,
    timings: Option<*mut LayoutTimings>,
}

impl<'a, 'out> LayoutContext<'a, 'out> {
    fn run(&mut self) {
        let start = Instant::now();
        self.clear_layout_output();
        self.record_timing(|t| t.clear_layout_output += start.elapsed());

        let start = Instant::now();
        self.float_stack.clear();
        self.float_stack.push(FloatContext::default());
        self.record_timing(|t| t.layout_tree_traversal += start.elapsed());

        if let Some(root_box) = self.root_box() {
            let start = Instant::now();
            self.set_box_point(root_box, Point::ZERO);
            self.layout_box(root_box, self.viewport_width, None, 0.0);
            let elapsed = start.elapsed();
            self.record_timing(|t| t.layout_tree_traversal += elapsed);
            self.record_timing(|t| t.root_box_layout += elapsed);
        }

        let finalization_started = Instant::now();
        let start = Instant::now();
        self.float_stack.clear();
        self.record_timing(|t| t.layout_tree_traversal += start.elapsed());

        let start = Instant::now();
        self.sort_lines_and_remap_images();
        self.record_timing(|t| t.sort_lines_and_remap_images += start.elapsed());

        let start = Instant::now();
        self.collect_inline_decorations();
        self.record_timing(|t| t.collect_inline_decorations += start.elapsed());

        let start = Instant::now();
        self.rebuild_image_fragments_by_line();
        self.record_timing(|t| t.rebuild_image_fragments_by_line += start.elapsed());
        self.record_timing(|t| t.finalize_layout += finalization_started.elapsed());
    }

    pub(crate) fn record_timing(&self, f: impl FnOnce(&mut LayoutTimings)) {
        if let Some(timings) = self.timings {
            unsafe {
                f(&mut *timings);
            }
        }
    }

    pub(crate) fn layout_cache(&self) -> &LayoutState {
        self.layout_state()
    }

    pub(crate) fn layout_cache_mut(&mut self) -> &mut LayoutState {
        self.layout_state_mut()
    }

    #[inline]
    pub(crate) fn glyph_metric(&self, glyph: GlyphId) -> crate::document::GlyphMetric {
        self.inputs.glyph_metrics.get(glyph)
    }

    #[inline]
    pub(crate) fn glyph_metrics(&self) -> &GlyphMetrics {
        self.inputs.glyph_metrics
    }

    pub(crate) fn image_display_size(&self, image_idx: u32) -> (f64, f64) {
        self.inputs.image_metrics.display_size(image_idx, self.document().image(image_idx).expect("image index should be valid"))
    }

    fn clear_layout_output(&mut self) {
        let layout = self.layout_cache_mut();
        layout.lines.clear();
        layout.line_glyph_offsets.clear();
        layout.line_glyph_advances.clear();
        layout.decorations.clear();
        layout.image_fragments.clear();
        for fragments in &mut layout.image_fragments_by_line {
            fragments.clear();
        }
        layout.anchor_positions.clear();
    }

    pub(crate) fn current_float_context(&self) -> Option<&FloatContext> {
        self.float_stack.last()
    }

    pub(crate) fn current_float_context_mut(&mut self) -> Option<&mut FloatContext> {
        self.float_stack.last_mut()
    }

    pub(crate) fn push_float_context(&mut self) {
        self.float_stack.push(FloatContext::default());
    }

    pub(crate) fn pop_float_context(&mut self) {
        self.float_stack.pop();
    }

    fn sort_lines_and_remap_images(&mut self) {
        let layout = self.layout_cache_mut();
        let mut lines = std::mem::take(&mut layout.lines);
        let mut offsets_by_line = std::mem::take(&mut layout.line_glyph_offsets);
        let mut advances_by_line = std::mem::take(&mut layout.line_glyph_advances);
        let mut line_pairs: Vec<(Line, Vec<GlyphOffsetRun>, Vec<GlyphAdvanceRun>, usize)> =
            lines.drain(..).enumerate().map(|(idx, line)| (line, offsets_by_line.get_mut(idx).map(std::mem::take).unwrap_or_default(), advances_by_line.get_mut(idx).map(std::mem::take).unwrap_or_default(), idx)).collect();
        offsets_by_line.clear();
        advances_by_line.clear();

        line_pairs.sort_by(|(a, _, _, _), (b, _, _, _)| a.point.y.partial_cmp(&b.point.y).unwrap_or(std::cmp::Ordering::Equal).then_with(|| a.point.x.partial_cmp(&b.point.x).unwrap_or(std::cmp::Ordering::Equal)));

        let mut index_map = vec![0usize; line_pairs.len()];
        for (new_idx, (_, _, _, old_idx)) in line_pairs.iter().enumerate() {
            index_map[*old_idx] = new_idx;
        }

        for (line, offsets, advances, _) in line_pairs {
            lines.push(line);
            offsets_by_line.push(offsets);
            advances_by_line.push(advances);
        }
        layout.lines = lines;
        layout.line_glyph_offsets = offsets_by_line;
        layout.line_glyph_advances = advances_by_line;
        for frag in &mut layout.image_fragments {
            if let Some(&new_idx) = index_map.get(frag.line_idx) {
                frag.line_idx = new_idx;
            }
        }
    }

    fn rebuild_image_fragments_by_line(&mut self) {
        let layout = self.layout_cache_mut();
        layout.image_fragments_by_line.resize_with(layout.lines.len(), Vec::new);
        layout.image_fragments_by_line.truncate(layout.lines.len());
        for (idx, frag) in layout.image_fragments.iter().enumerate() {
            if let Some(line) = layout.image_fragments_by_line.get_mut(frag.line_idx) {
                line.push(idx);
            }
        }
    }
}

impl<'a, 'out> LayoutContext<'a, 'out> {
    fn document_ref(&self) -> &'a Document {
        self.inputs.document
    }

    fn layout_tree_ref(&self) -> &LayoutTree {
        self.outputs.layout_tree
    }

    fn layout_tree_mut(&mut self) -> &mut LayoutTree {
        self.outputs.layout_tree
    }

    fn inline_content_ref(&self) -> &'a InlineContent {
        self.inputs.inline_content
    }

    pub(crate) fn document(&self) -> &'a Document {
        self.document_ref()
    }

    pub(crate) fn layout_state(&self) -> &LayoutState {
        self.outputs.state
    }

    pub(crate) fn layout_state_mut(&mut self) -> &mut LayoutState {
        self.outputs.state
    }

    pub(crate) fn style(&self, box_idx: usize) -> StyleView<'a> {
        self.inputs.styles.view(get_style_indices(self, box_idx)).expect("validated style handle")
    }

    pub(crate) fn box_at(&self, box_idx: usize) -> Option<&LayoutBox> {
        self.layout_tree_ref().box_at(box_idx)
    }

    pub(crate) fn box_at_mut(&mut self, box_idx: usize) -> Option<&mut LayoutBox> {
        self.layout_tree_mut().box_at_mut(box_idx)
    }

    pub(crate) fn box_dom_element_idx(&self, box_idx: usize) -> Option<u32> {
        self.box_at(box_idx)?.dom_element()
    }

    pub(crate) fn get_box_image_idx(&self, box_idx: usize) -> Option<u32> {
        let node_idx = self.box_at(box_idx)?.dom_element()?;
        let dom_node = self.document_ref().node_id_from_raw(node_idx)?;
        self.document_ref().element_ref(dom_node).and_then(|element| element.image_idx())
    }

    pub(crate) fn box_layout_mode(&self, box_idx: usize) -> Option<&LayoutMode> {
        self.box_at(box_idx).map(|layout_box| layout_box.layout_mode())
    }

    pub(crate) fn box_style_indices(&self, box_idx: usize) -> Option<crate::document::StyleIndices> {
        self.box_at(box_idx)?.style()
    }

    pub(crate) fn root_box(&self) -> Option<usize> {
        self.layout_tree_ref().root_box()
    }

    pub(crate) fn box_count(&self) -> usize {
        self.layout_tree_ref().box_count()
    }

    pub(crate) fn is_table_cell_box(&self, box_idx: usize) -> bool {
        matches!(self.box_layout_mode(box_idx), Some(LayoutMode::TableCell(_)))
    }

    pub(crate) fn is_block_box(&self, box_idx: usize) -> bool {
        matches!(self.box_layout_mode(box_idx), Some(LayoutMode::Block(_)))
    }

    pub(crate) fn is_anonymous_box(&self, box_idx: usize) -> bool {
        matches!(self.box_layout_mode(box_idx), Some(LayoutMode::Anonymous(_)))
    }

    pub(crate) fn is_table_box(&self, box_idx: usize) -> bool {
        matches!(self.box_layout_mode(box_idx), Some(LayoutMode::Table(_)))
    }

    pub(crate) fn is_inline_box(&self, box_idx: usize) -> bool {
        matches!(self.box_layout_mode(box_idx), Some(LayoutMode::Inline(_)))
    }

    pub(crate) fn get_box_point(&self, box_idx: usize) -> Point {
        self.layout_tree_ref().get_box_point(box_idx)
    }

    pub(crate) fn get_box_size(&self, box_idx: usize) -> kurbo::Size {
        self.layout_tree_ref().get_box_size(box_idx)
    }

    pub(crate) fn set_box_point(&mut self, box_idx: usize, point: Point) {
        self.layout_tree_mut().set_box_point(box_idx, point);
    }

    pub(crate) fn set_box_size(&mut self, box_idx: usize, size: kurbo::Size) {
        self.layout_tree_mut().set_box_size(box_idx, size);
    }

    pub(crate) fn inline_runs_empty(&self) -> bool {
        self.inline_content_ref().inline_runs_empty()
    }

    pub(crate) fn inline_runs(&self) -> &[InlineRun] {
        self.inline_content_ref().inline_runs()
    }

    pub(crate) fn inline_run(&self, run_idx: usize) -> Option<&InlineRun> {
        self.inline_content_ref().inline_run(run_idx)
    }

    pub(crate) fn glyphs(&self) -> &[GlyphId] {
        self.inline_content_ref().glyphs()
    }

    pub(crate) fn glyph_at(&self, glyph_idx: usize) -> Option<GlyphId> {
        self.inline_content_ref().glyph_at(glyph_idx)
    }

    pub(crate) fn glyph(&self, glyph_idx: u32) -> Option<GlyphId> {
        self.inline_content_ref().glyph_at(glyph_idx as usize)
    }

    pub(crate) fn text_runs(&self) -> Vec<(usize, Range<u32>)> {
        self.inline_content_ref()
            .inline_runs()
            .iter()
            .filter_map(|run| match &run.kind {
                InlineKind::Text { glyphs } if glyphs.start < glyphs.end => Some((run.box_idx as usize, glyphs.clone())),
                _ => None,
            })
            .collect()
    }

    pub(crate) fn get_parent(&self, box_idx: usize) -> Option<usize> {
        self.box_at(box_idx)?.parent().map(|p| p as usize)
    }

    pub(crate) fn list_marker(&self, box_idx: usize) -> Option<crate::document::ListItemMarker> {
        self.layout_tree_ref().list_marker(box_idx)
    }

    pub(crate) fn lines(&self) -> &[Line] {
        self.layout_state().lines.as_slice()
    }

    pub(crate) fn box_float(&self, box_idx: usize) -> Float {
        self.style(box_idx).float()
    }

    pub(crate) fn box_clear(&self, box_idx: usize) -> Clear {
        self.style(box_idx).clear()
    }

    pub(crate) fn box_uses_float_context(&self, box_idx: usize) -> bool {
        self.root_box() == Some(box_idx) || self.is_table_cell_box(box_idx) || matches!(self.box_float(box_idx), Float::Left | Float::Right)
    }
}
