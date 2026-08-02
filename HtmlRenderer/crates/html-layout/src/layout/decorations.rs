use crate::document::{BorderStyle, DecorationFragment, InlineKind, Line, StyleView};
use crate::layout::LayoutContext;
use kurbo::Rect;

fn push_border_rects(layout: &mut Vec<DecorationFragment>, rect: Rect, border_width: f64, color: u32, is_inline: bool) {
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
        layout.push(DecorationFragment { rect, color, is_inline });
    }
}

fn uniform_solid_border_width(style: &StyleView) -> f64 {
    matches!(style.border_top_style(), BorderStyle::Solid).then_some(style.border_top_width() as f64).unwrap_or(0.0)
}

fn paint_block_borders(layout: &mut Vec<DecorationFragment>, rect: Rect, top: Option<(f64, u32)>, right: Option<(f64, u32)>, bottom: Option<(f64, u32)>, left: Option<(f64, u32)>) {
    if let Some((w, c)) = top {
        let h = w.min(rect.height());
        if h > 0.0 {
            layout.push(DecorationFragment { rect: Rect::new(rect.x0, rect.y0, rect.x1, rect.y0 + h), color: c, is_inline: false });
        }
    }
    if let Some((w, c)) = right {
        let bw = w.min(rect.width());
        if bw > 0.0 {
            layout.push(DecorationFragment { rect: Rect::new(rect.x1 - bw, rect.y0, rect.x1, rect.y1), color: c, is_inline: false });
        }
    }
    if let Some((w, c)) = bottom {
        let h = w.min(rect.height());
        if h > 0.0 {
            layout.push(DecorationFragment { rect: Rect::new(rect.x0, rect.y1 - h, rect.x1, rect.y1), color: c, is_inline: false });
        }
    }
    if let Some((w, c)) = left {
        let bw = w.min(rect.width());
        if bw > 0.0 {
            layout.push(DecorationFragment { rect: Rect::new(rect.x0, rect.y0, rect.x0 + bw, rect.y1), color: c, is_inline: false });
        }
    }
}

impl<'a, 'out> LayoutContext<'a, 'out> {
    /// Content rect of a block: the border box minus margins, positioned at the
    /// box's layout point. Shared by background and border emission.
    fn block_content_rect(&self, box_idx: usize) -> Rect {
        let style = self.style(box_idx);
        let size = self.get_box_size(box_idx);
        // Decorations are emitted after layout; the containing-block width is no
        // longer threaded here, so resolve percentage margins against this box's
        // own border-box width (px margins resolve exactly regardless).
        let cb_width = size.width;
        let margin_left = style.margin_left().resolve(cb_width);
        let margin_right = style.margin_right().resolve(cb_width);
        let margin_top = style.margin_top().resolve(cb_width);
        let margin_bottom = style.margin_bottom().resolve(cb_width);

        let point = self.get_box_point(box_idx);
        let width = (size.width - margin_left - margin_right).max(0.0);
        let height = (size.height - margin_top - margin_bottom).max(0.0);
        Rect::new(point.x, point.y, point.x + width, point.y + height)
    }

    pub(crate) fn emit_block_background(&mut self, box_idx: usize) {
        let background_color = self.style(box_idx).background_color();
        if background_color & 0xFF == 0 {
            return;
        }
        let rect = self.block_content_rect(box_idx);
        self.layout_state_mut().decorations.push(DecorationFragment { rect, color: background_color, is_inline: false });
    }

    pub(crate) fn emit_block_decorations(&mut self, box_idx: usize) {
        // Extract owned border values before any mutable operations on the document.
        let style = self.style(box_idx);
        let top_border = matches!(style.border_top_style(), BorderStyle::Solid).then_some((style.border_top_width() as f64, style.border_top_color()));
        let right_border = matches!(style.border_right_style(), BorderStyle::Solid).then_some((style.border_right_width() as f64, style.border_right_color()));
        let bottom_border = matches!(style.border_bottom_style(), BorderStyle::Solid).then_some((style.border_bottom_width() as f64, style.border_bottom_color()));
        let left_border = matches!(style.border_left_style(), BorderStyle::Solid).then_some((style.border_left_width() as f64, style.border_left_color()));
        let has_any_border = top_border.is_some() || right_border.is_some() || bottom_border.is_some() || left_border.is_some();

        self.emit_block_background(box_idx);

        if has_any_border {
            let rect = self.block_content_rect(box_idx);
            let layout = self.layout_state_mut();
            paint_block_borders(&mut layout.decorations, rect, top_border, right_border, bottom_border, left_border);
        }
    }

    pub(crate) fn collect_inline_decorations(&mut self) {
        if self.lines().is_empty() || self.inline_runs_empty() {
            return;
        }

        // Inline text runs in ascending, contiguous glyph order. A glyph's owning
        // box is derived from these runs (one entry per run) instead of a
        // per-glyph map.
        let text_runs = self.inline_runs().to_vec();

        let inline_ancestors = self.build_inline_ancestor_map();
        let mut active = vec![false; self.box_count()];
        let mut active_start = vec![0.0f64; self.box_count()];
        let mut active_indices: Vec<u32> = Vec::new();
        let mut marked = vec![false; self.box_count()];
        let mut run_cursor = 0usize;

        let lines = self.lines().to_vec();

        for (line_idx, line) in lines.into_iter().enumerate() {
            for &b in &active_indices {
                active[b as usize] = false;
            }
            active_indices.clear();

            // Run containing this line's first glyph; advanced monotonically as
            // we walk the line left to right.
            while run_cursor < text_runs.len() {
                match &text_runs[run_cursor].kind {
                    InlineKind::Text { glyphs } | InlineKind::Marker { glyphs } if glyphs.end <= line.glyphs.start => {
                        run_cursor += 1;
                        continue;
                    }
                    InlineKind::Text { glyphs } | InlineKind::Marker { glyphs } => {
                        if glyphs.start > line.glyphs.start {
                            break;
                        }
                        break;
                    }
                    _ => {
                        run_cursor += 1;
                    }
                }
            }

            let mut x = line.point.x;
            for i in line.glyphs.clone() {
                while run_cursor < text_runs.len() {
                    match &text_runs[run_cursor].kind {
                        InlineKind::Text { glyphs } | InlineKind::Marker { glyphs } if glyphs.end <= i => {
                            run_cursor += 1;
                            continue;
                        }
                        InlineKind::Text { glyphs } | InlineKind::Marker { glyphs } => {
                            if glyphs.start <= i {
                                break;
                            }
                            break;
                        }
                        _ => {
                            run_cursor += 1;
                            continue;
                        }
                    }
                }

                // Glyphs outside any text run contribute no inline ancestors,
                // matching the old map's behavior for uncovered glyphs.
                let boxes: &[u32] = match text_runs.get(run_cursor) {
                    Some(run) => match &run.kind {
                        InlineKind::Text { glyphs } | InlineKind::Marker { glyphs } if glyphs.start <= i => &inline_ancestors[run.box_idx as usize],
                        _ => &[],
                    },
                    _ => &[],
                };

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

                let Some(glyph_index) = self.glyph(i) else {
                    continue;
                };
                x += self.layout_cache().line_glyph_advances.get(line_idx).and_then(|runs| runs.iter().find(|run| run.range.contains(&i))).map_or_else(|| self.glyph_metric(glyph_index).advance() as f64, |run| run.advance as f64);
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
        let mut result = vec![Vec::new(); self.box_count()];
        for i in 0..self.box_count() {
            let mut current = Some(i);
            let mut ancestors = Vec::new();
            while let Some(idx) = current {
                if self.is_inline_box(idx) {
                    ancestors.push(idx as u32);
                }
                current = self.get_parent(idx);
            }
            result[i] = ancestors;
        }
        result
    }

    fn emit_inline_segment(&mut self, box_idx: usize, line: &Line, start_x: f64, end_x: f64) {
        if start_x >= end_x {
            return;
        }
        if !self.is_inline_box(box_idx) {
            return;
        }

        // Extract all style values before mutable operations
        let style = self.style(box_idx);
        let background_color = style.background_color();
        let color_alpha = background_color & 0xFF;
        let border_width = uniform_solid_border_width(&style);
        // Resolve percentage padding against the inline box's own laid-out width
        // (the containing-block width is not threaded into the post-layout
        // decoration pass; px padding resolves exactly regardless).
        let cb_width = self.get_box_size(box_idx).width;
        let pad_left = style.padding_left().resolve(cb_width);
        let pad_right = style.padding_right().resolve(cb_width);
        let pad_top = style.padding_top().resolve(cb_width);
        let pad_bottom = style.padding_bottom().resolve(cb_width);
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
            self.layout_state_mut().decorations.push(DecorationFragment { rect, color: background_color, is_inline: true });
        }
        if border_width > 0.0 {
            let layout = self.layout_state_mut();
            push_border_rects(&mut layout.decorations, rect, border_width, border_top_color, true);
        }
    }
}
