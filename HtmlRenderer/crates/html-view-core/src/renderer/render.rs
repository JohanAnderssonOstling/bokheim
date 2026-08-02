use std::collections::HashSet;
use std::time::Duration;

use kurbo::{Point, Rect};
use peniko::Color;

use crate::{LineScreen, PagePositions, PaintDirection, Painter, RenderState, RendererCore};

impl RendererCore {
    fn render_start_line_at(&self, offset_y: f64, epsilon: f64) -> usize {
        let lines = self.document.render_view().lines();
        let mut lo = 0;
        let mut hi = lines.len();
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let line = lines.get(mid).expect("binary-search index must be within line count");
            if line.point().y + line.height() <= offset_y + epsilon {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo
    }

    fn render_end_line_at(&self, offset_y: f64, epsilon: f64) -> Option<usize> {
        let lines = self.document.render_view().lines();
        let Some(first_line) = lines.first() else {
            return None;
        };
        if first_line.point().y + first_line.height() > offset_y + epsilon {
            return None;
        }
        let mut lo = 0;
        let mut hi = lines.len() - 1;
        while lo < hi {
            let mid = lo + (hi - lo + 1) / 2;
            let line = lines.get(mid).expect("binary-search index must be within line count");
            if line.point().y + line.height() <= offset_y + epsilon {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        Some(lo)
    }

    #[inline]
    fn snap_to_device_px(&self, v: f64) -> f64 {
        (v * self.layout.scale).round() / self.layout.scale
    }

    /// Compute x-coordinate for a column given a local x offset
    #[inline]
    fn col_x(&self, col_index: f64, local_x: f64) -> f64 {
        self.layout.col_gap + col_index * (self.layout.col_width + self.layout.col_gap) + local_x
    }

    /// Compute scaled image dimensions to fit column width
    fn scaled_image_size(&self, frag: &html_layout::RenderImageFragment) -> (f64, f64) {
        let size = frag.size();
        (size.width, size.height)
    }

    fn resolve_point_at(&self, point: Point, elem_height: f64, mut render_state: RenderState, start_offset_y: f64) -> (Option<Point>, RenderState) {
        // map document point to screen columns
        let mut y = point.y + render_state.y_offset - start_offset_y;
        let mut col_index = (y / self.layout.size.height).floor(); // initial column based on vertical position
        y = y - col_index * self.layout.size.height;

        if y + elem_height > self.layout.size.height {
            // move to next column if element would overflow
            col_index += 1.0;
            render_state.y_offset += self.layout.size.height - y;
            y = 0.;
        }

        // Past all columns - stop rendering
        if col_index >= self.layout.col_count {
            // stop when past last column
            render_state.should_stop = true;
            return (None, render_state);
        }

        render_state.col_index = col_index;
        let x = self.col_x(col_index, point.x);
        let resolved = Point::new(x, y);
        (Some(resolved), render_state)
    }

    fn initial_render_state() -> RenderState {
        // canonical renderer state for a fresh pass
        RenderState { y_offset: 0.0, col_index: 0.0, should_stop: false }
    }

    fn draw_line_index(&self, cx: &mut impl Painter, line_idx: usize, line: &html_layout::RenderLine, line_point: Point) {
        // render gutter line numbers
        let digits_str = line_idx.to_string();
        let digits_bytes = digits_str.as_bytes();
        let mut width = 0.0;
        for &b in digits_bytes {
            let digit = (b - b'0') as usize;
            width += self.document.render_view().glyph_metric(self.line_numbers.digits[digit]).expect("line-number glyph is registered").advance() as f64;
        }
        width += self.document.render_view().glyph_metric(self.line_numbers.space).expect("line-number space is registered").advance() as f64;

        let mut x = line_point.x - width - self.line_numbers.gap;
        if x < 0.0 {
            x = 0.0;
        }
        let base_y = line_point.y + line.baseline();
        for &b in digits_bytes {
            let digit = (b - b'0') as usize;
            let glyph_idx = self.line_numbers.digits[digit];
            let metric = self.document.render_view().glyph_metric(glyph_idx).expect("line-number glyph is registered");
            let baseline_offset = metric.baseline_offset() as f64;
            let point = Point::new(x, self.snap_to_device_px(base_y - baseline_offset));
            cx.draw_glyph(glyph_idx, point);
            x += metric.advance() as f64;
        }
        let space_baseline_offset = self.document.render_view().glyph_metric(self.line_numbers.space).expect("line-number space is registered").baseline_offset() as f64;
        let point = Point::new(x, self.snap_to_device_px(base_y - space_baseline_offset));
        cx.draw_glyph(self.line_numbers.space, point);
    }

    fn paint_decorations(&self, cx: &mut impl Painter, start_offset_y: f64, breaks: Option<Vec<(f64, f64)>>, line_positions: Option<&[Option<LineScreen>]>) {
        // draw background decorations across columns
        let page_start = start_offset_y;
        let page_end = start_offset_y + self.layout.col_count * self.layout.size.height; // visible doc y end for this page
        let breaks = breaks.unwrap_or_else(|| self.build_offset_breaks(start_offset_y));
        let inline_metrics = self.inline_metrics();
        let lines = self.document.render_view().lines();
        for decoration in self.document.render_view().decorations().iter() {
            let rect = decoration.rect();
            if rect.y1 <= page_start || rect.y0 >= page_end {
                continue;
            }
            let color = color_from_u32(decoration.color());
            if decoration.is_inline() {
                if let Some(line_idx) = self.find_line_index_for_decoration(rect.y0, rect.y1) {
                    if let Some(screen) = line_positions.and_then(|p| p.get(line_idx).and_then(|v| *v)) {
                        let Some(line) = lines.get(line_idx) else {
                            continue;
                        };
                        self.paint_inline_decoration_at(cx, &rect, &line, screen.point, 0.0, &color);
                    } else if let Some(line) = self.document.render_view().line(line_idx) {
                        let render_state = Self::initial_render_state();
                        let (point, _) = self.resolve_point_at(line.point(), line.height(), render_state, start_offset_y);
                        if let Some(point) = point {
                            self.paint_inline_decoration_at(cx, &rect, &line, point, inline_metrics.offset, &color);
                        }
                    }
                }
                continue;
            }
            let mut seg_start = rect.y0;
            let mut break_idx = 0;
            let mut current_offset = 0.0;
            while break_idx < breaks.len() && breaks[break_idx].0 <= seg_start {
                current_offset = breaks[break_idx].1;
                break_idx += 1;
            }

            while seg_start < rect.y1 {
                let next_break = breaks.get(break_idx).map(|b| b.0).unwrap_or(rect.y1);
                let seg_end = rect.y1.min(next_break);
                self.paint_decoration_segment(cx, &rect, seg_start, seg_end, current_offset, &color, start_offset_y);
                seg_start = seg_end;
                if break_idx < breaks.len() && breaks[break_idx].0 <= seg_start {
                    current_offset = breaks[break_idx].1;
                    break_idx += 1;
                }
            }
        }
    }

    fn build_offset_breaks(&self, start_offset_y: f64) -> Vec<(f64, f64)> {
        // compute y-offset jumps at column breaks
        let mut breaks = Vec::new();
        let mut render_state = Self::initial_render_state();
        let start_line = self.render_start_line_at(start_offset_y, 0.1);
        let mut prev_offset = render_state.y_offset;
        let lines = self.document.render_view().lines();
        for idx in start_line..lines.len() {
            let Some(line) = lines.get(idx) else {
                break;
            };
            let (_point, next_state) = self.resolve_point_at(line.point(), line.height(), render_state, start_offset_y);
            if next_state.y_offset != prev_offset {
                breaks.push((line.point().y, next_state.y_offset));
                prev_offset = next_state.y_offset;
            }
            render_state = next_state;
            if render_state.should_stop {
                break;
            }
        }
        breaks
    }

    fn build_offset_breaks_from_positions(&self, first_line: usize, end_line: usize, positions: &[Option<LineScreen>], start_offset_y: f64) -> Vec<(f64, f64)> {
        // compute offset breaks from cached positions
        let mut breaks = Vec::new();
        let mut prev_offset: Option<f64> = None;
        for line_idx in first_line..=end_line {
            let Some(screen) = positions.get(line_idx).and_then(|v| *v) else {
                continue;
            };
            let doc_y = self.document.render_view().line(line_idx).map(|line| line.point().y).unwrap_or(0.0);
            let offset = screen.point.y // cached screen y converted back to doc y
                + (screen.col_index as f64) * self.layout.size.height
                - doc_y
                + start_offset_y;
            if prev_offset.map(|o| (o - offset).abs() > 1e-6).unwrap_or(true) {
                breaks.push((doc_y, offset));
                prev_offset = Some(offset);
            }
        }
        breaks
    }

    fn paint_decoration_segment(&self, cx: &mut impl Painter, rect: &kurbo::Rect, seg_start: f64, seg_end: f64, y_offset: f64, color: &peniko::Color, start_offset_y: f64) {
        // split decoration across columns and draw
        if seg_end <= seg_start {
            return;
        }

        let col_height = self.layout.size.height;
        let y0p = seg_start + y_offset - start_offset_y; // segment y in page space
        let y1p = seg_end + y_offset - start_offset_y;
        if y1p <= 0.0 {
            return;
        }
        let eps = 0.0001;
        let mut first_col = (y0p / col_height).floor() as i32;
        let mut last_col = ((y1p - eps) / col_height).floor() as i32;
        if first_col < 0 {
            first_col = 0;
        }
        let max_col = self.layout.col_count as i32 - 1;
        if last_col > max_col {
            last_col = max_col;
        }
        if last_col < first_col {
            return;
        }

        for col in first_col..=last_col {
            let col_top = (col as f64) * col_height;
            let col_bottom = col_top + col_height;
            let y0 = y0p.max(col_top);
            let y1 = y1p.min(col_bottom);
            if y1 <= y0 {
                continue;
            }
            let x = self.col_x(col as f64, rect.x0);
            let y = y0 - col_top;
            let height = y1 - y0;
            let split_rect = kurbo::Rect::new(x, y, x + rect.width(), y + height);
            cx.fill_rect(split_rect, *color);
        }
    }

    fn inline_metrics(&self) -> crate::InlineMetrics {
        // compute inline decoration offset within column gap
        const MIN_GAP: f64 = 40.0;
        let effective_gap = (self.layout.col_gap - MIN_GAP).max(0.0);
        let half_gap = effective_gap / 2.0;
        crate::InlineMetrics { offset: half_gap / 2.0 }
    }

    fn find_line_index_for_decoration(&self, y0: f64, y1: f64) -> Option<usize> {
        // find first line overlapping a decoration rect
        let lines = self.document.render_view().lines();
        if lines.is_empty() {
            return None;
        }
        let mut lo = 0usize;
        let mut hi = lines.len();
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let line = lines.get(mid).expect("binary-search index must be within line count");
            if line.point().y + line.height() <= y0 {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        for idx in lo..lines.len() {
            let line = lines.get(idx).expect("line index must be within line count");
            let ly0 = line.point().y;
            let ly1 = line.point().y + line.height();
            if ly0 >= y1 {
                break;
            }
            if ly1 > y0 && ly0 < y1 {
                return Some(idx);
            }
        }
        None
    }

    fn paint_inline_decoration_at(&self, cx: &mut impl Painter, rect: &kurbo::Rect, line: &html_layout::RenderLine, screen_point: Point, x_offset: f64, color: &peniko::Color) {
        let point = line.point();
        let x = screen_point.x + x_offset + (rect.x0 - point.x);
        let y = screen_point.y + (rect.y0 - point.y);
        let split_rect = kurbo::Rect::new(x, y, x + rect.width(), y + rect.height());
        cx.fill_rect(split_rect, *color);
    }

    fn draw_line_at(&self, cx: &mut impl Painter, line_idx: usize, line: &html_layout::RenderLine, point: Point, render_state: RenderState) -> RenderState {
        // render glyphs for one line at a screen point
        let point = Point::new(self.snap_to_device_px(point.x), self.snap_to_device_px(point.y));
        if self.show_line_numbers {
            self.draw_line_index(cx, line_idx, line, point);
        }
        self.draw_images_for_line(cx, line_idx, point);

        let mut curr_x = point.x;
        let base_y = point.y + line.baseline();
        let offsets = self.document.render_view().line_glyph_offsets(line_idx);
        let advances = self.document.render_view().line_glyph_advances(line_idx);
        let mut offset_idx = 0usize;
        let mut current = offsets.as_ref().and_then(|runs| runs.get(0));
        let mut advance_idx = 0usize;
        let mut current_advance = advances.as_ref().and_then(|runs| runs.get(0));
        let glyphs = line.glyphs();
        for index in glyphs.start..glyphs.end {
            let view = self.document.render_view();
            let glyph_index = view.glyph_at(index as usize).unwrap_or_default();
            let metric = view.glyph_metric(glyph_index).expect("layout glyph IDs are registered");
            let baseline_offset = metric.baseline_offset() as f64;
            while let Some(run) = current.as_ref() {
                if index < run.range().end {
                    break;
                }
                offset_idx += 1;
                current = offsets.as_ref().and_then(|runs| runs.get(offset_idx));
            }
            let offset = current.as_ref().and_then(|run| if index >= run.range().start { Some(run.offset()) } else { None }).unwrap_or(0.0);
            while let Some(run) = current_advance.as_ref() {
                if index < run.range().end {
                    break;
                }
                advance_idx += 1;
                current_advance = advances.as_ref().and_then(|runs| runs.get(advance_idx));
            }
            let advance_override = current_advance.as_ref().and_then(|run| (index >= run.range().start).then(|| run.advance()));
            let draw_point = Point::new(self.snap_to_device_px(curr_x), self.snap_to_device_px(base_y - baseline_offset - offset));

            if advance_override.is_none() {
                cx.draw_glyph(glyph_index, draw_point);
            }

            if let Some(advance) = advance_override {
                curr_x += advance;
                continue;
            }
            curr_x += metric.advance() as f64 + line.letter_spacing();

            // Extra space after word breaks (spaces) for justification
            if metric.ch() == ' ' {
                curr_x += line.word_spacing();
            }
        }
        render_state
    }

    fn draw_images_for_line(&self, cx: &mut impl Painter, line_idx: usize, point: Point) {
        let images = self.document.render_view().images_for_line(line_idx);
        if images.is_empty() {
            return;
        }
        let placeholder = Color::rgba8(230, 230, 230, 255);
        for frag in images.iter() {
            let (width, height) = self.scaled_image_size(&frag);
            let offset = frag.offset();
            let rect = Rect::new(point.x + offset.x, point.y + offset.y, point.x + offset.x + width, point.y + offset.y + height);
            if let Some(decoded) = self.image_pipeline.get_decoded(frag.image_idx()) {
                cx.draw_image(&decoded.image, &decoded.hash, rect);
            } else {
                cx.fill_rect(rect, placeholder);
            }
        }
    }

    fn effective_line_height(&self, line_idx: usize, fallback: f64) -> f64 {
        if line_idx >= self.document.render_view().line_count() {
            return fallback;
        }
        let mut max_height = 0.0;
        for frag in self.document.render_view().images_for_line(line_idx).iter() {
            let (_, height) = self.scaled_image_size(&frag);
            if height > max_height {
                max_height = height;
            }
        }
        if max_height > 0.0 { max_height } else { fallback }
    }

    fn fill_selection_rect(&self, cx: &mut impl Painter, x0: f64, x1: f64, y: f64, height: f64) {
        // paint selection highlight for a horizontal span
        if x1 <= x0 {
            return;
        }
        let color = self.interaction_style.selection;
        let highlight = Color::rgba8(color.r, color.g, color.b, color.a);
        let rect = Rect::new(x0, y, x1, y + height);
        cx.fill_rect(rect, highlight);
    }

    fn paint_selection_for_line(&self, cx: &mut impl Painter, line_idx: usize, line: &html_layout::RenderLine, screen_point: Point) {
        // draw selection highlight for a line
        let Some((sel_start, sel_end)) = self.selection_range() else {
            return;
        };
        let glyphs = line.glyphs();
        let start = sel_start.max(glyphs.start);
        let end = sel_end.min(glyphs.end);
        if start >= end && !(sel_start < glyphs.start && sel_end > glyphs.start) {
            return;
        }

        let point = line.point();
        let col_start_x = screen_point.x - point.x;
        let col_end_x = col_start_x + self.layout.col_width;
        let starts_before = sel_start < glyphs.start;
        let ends_after = sel_end > glyphs.end;
        if let Some((start_line, end_line)) = self.selection_line_bounds(sel_start, sel_end) {
            if start_line != end_line {
                if line_idx > start_line && line_idx < end_line {
                    self.fill_selection_rect(cx, col_start_x, col_end_x, screen_point.y, line.height());
                    return;
                }
                if line_idx == start_line {
                    let x0 = screen_point.x + self.glyph_x_in_line(line, sel_start);
                    self.fill_selection_rect(cx, x0, col_end_x, screen_point.y, line.height());
                    return;
                }
                if line_idx == end_line {
                    let x1 = screen_point.x + self.glyph_x_in_line(line, sel_end);
                    self.fill_selection_rect(cx, col_start_x, x1, screen_point.y, line.height());
                    return;
                }
            }
        }

        let mut x = 0.0;
        let mut x0: Option<f64> = None;
        let mut x1: Option<f64> = None;
        for i in glyphs.start..glyphs.end {
            if i == start {
                x0 = Some(x);
            }
            let view = self.document.render_view();
            let glyph_index = view.glyph_at(i as usize).unwrap_or_default();
            let metric = view.glyph_metric(glyph_index).expect("layout glyph IDs are registered");
            if let Some(advance) = self.document.render_view().line_glyph_advances(line_idx).and_then(|runs| runs.iter().find(|run| run.range().contains(&i)).map(|run| run.advance())) {
                x += advance;
            } else {
                x += metric.advance() as f64 + line.letter_spacing();
                if metric.ch() == ' ' {
                    x += line.word_spacing();
                }
            }
            if i + 1 == end {
                x1 = Some(x);
                break;
            }
        }
        let x0 = if starts_before { col_start_x } else { screen_point.x + x0.unwrap_or(0.0) };
        let x1 = if ends_after { col_end_x } else { screen_point.x + x1.unwrap_or(x) };
        self.fill_selection_rect(cx, x0, x1, screen_point.y, line.height());
    }

    fn build_forward_positions(&self, start_offset_y: f64) -> PagePositions {
        let lines = self.document.render_view().lines();
        let start_line = self.render_start_line_at(start_offset_y, 0.1);
        let mut positions: Vec<Option<LineScreen>> = vec![None; lines.len()];
        let mut render_state = Self::initial_render_state();
        let mut last_line: Option<usize> = None;
        let mut next_start_offset_y = start_offset_y;
        for line_idx in start_line..lines.len() {
            let line = lines.get(line_idx).expect("line index must stay within line_count");
            let effective_height = self.effective_line_height(line_idx, line.height());
            let (point, next_state) = self.resolve_point_at(line.point(), effective_height, render_state, start_offset_y);
            render_state = next_state;
            if let Some(point) = point {
                positions[line_idx] = Some(LineScreen { point, col_index: render_state.col_index as i32 });
                last_line = Some(line_idx);
            }
            if render_state.should_stop {
                next_start_offset_y = line.point().y;
                break;
            }
        }
        let reached_end = !render_state.should_stop;
        PagePositions { start_line, last_line, positions, reached_end, next_start_offset_y }
    }

    fn paint_lines_from_positions(&self, cx: &mut impl Painter, first_line: usize, end_line: usize, positions: &[Option<LineScreen>], mut render_state: RenderState) -> RenderState {
        let lines = self.document.render_view().lines();
        for line_idx in first_line..=end_line {
            if let (Some(screen), Some(line)) = (positions.get(line_idx).and_then(|v| *v), lines.get(line_idx)) {
                self.paint_highlights_for_line(cx, &line, screen.point);
                self.paint_selection_for_line(cx, line_idx, &line, screen.point);
                render_state = self.draw_line_at(cx, line_idx, &line, screen.point, render_state);
            }
        }
        render_state
    }

    fn paint_positioned_page(&mut self, cx: &mut impl Painter, page: PagePositions, start_offset_y: f64, paint_end: Option<usize>, use_cached_inline_positions: bool) -> RenderState {
        self.frame.last_line_positions = page.positions;
        let breaks = paint_end.map(|end_line| self.build_offset_breaks_from_positions(page.start_line, end_line, &self.frame.last_line_positions, start_offset_y));

        if use_cached_inline_positions {
            self.paint_decorations(cx, start_offset_y, breaks, Some(&self.frame.last_line_positions));
        } else {
            self.paint_decorations(cx, start_offset_y, breaks, None);
        }

        self.frame.current_page_start_line = Some(page.start_line);
        self.frame.current_page_end_line = paint_end;

        let mut render_state = Self::initial_render_state();
        if let Some(end_line) = paint_end {
            render_state = self.paint_lines_from_positions(cx, page.start_line, end_line, &self.frame.last_line_positions, render_state);
        }
        render_state
    }

    pub(crate) fn paint_forward(&mut self, cx: &mut impl Painter) -> RenderState {
        // render forward from current offset
        let page = self.build_forward_positions(self.viewport.start_offset_y);
        self.viewport.next_start_offset_y = page.next_start_offset_y;
        self.viewport.reached_end = page.reached_end;
        let paint_end = page.last_line;
        self.paint_positioned_page(cx, page, self.viewport.start_offset_y, paint_end, false)
    }

    pub(crate) fn prefetch_images_for_current_page(&mut self) {
        let view = self.document.render_view();
        if view.all_image_fragments().is_empty() || view.line_count() == 0 {
            return;
        }
        let Some(start_line) = self.frame.current_page_start_line else {
            return;
        };
        let Some(end_line) = self.frame.current_page_end_line else {
            return;
        };
        if start_line >= view.line_count() || end_line >= view.line_count() {
            return;
        }

        let page_len = end_line.saturating_sub(start_line) + 1;
        let next_start = end_line + 1;
        let next_end = if next_start >= view.line_count() { end_line } else { (next_start + page_len - 1).min(view.line_count() - 1) };

        let mut desired: HashSet<u32> = HashSet::new();
        for line_idx in start_line..=next_end {
            for frag in view.images_for_line(line_idx).iter() {
                desired.insert(frag.image_idx());
            }
        }

        self.image_pipeline.ensure_window(&desired);
        if self.image_pipeline.has_pending() {
            self.host.schedule_repaint(Duration::from_millis(16));
        }
    }

    pub(crate) fn paint_backward(&mut self, cx: &mut impl Painter) -> RenderState {
        // render backward from end offset
        self.viewport.reached_end = false;

        let lines = self.document.render_view().lines();
        let end_line = if let Some(idx) = self.viewport.back_anchor_end_line {
            idx.min(lines.len().saturating_sub(1))
        } else if let Some(idx) = self.render_end_line_at(self.viewport.end_offset_y, 1e-6) {
            idx
        } else {
            self.viewport.start_offset_y = 0.0;
            self.viewport.direction = PaintDirection::Forward;
            return self.paint_forward(cx);
        };
        if end_line == 0 {
            self.viewport.start_offset_y = 0.0;
            self.viewport.direction = PaintDirection::Forward;
            self.viewport.back_anchor_end_line = None;
            return self.paint_forward(cx);
        }

        let available_height = self.layout.col_count * self.layout.size.height;
        let end_line_data = lines.get(end_line).expect("end line must be within line count");
        let end_line_height = self.effective_line_height(end_line, end_line_data.height());
        // Keep a tiny safety margin so the anchor line doesn't land exactly on the
        // bottom edge and get treated as overflow by column fit logic.
        let end_line_target_y = (available_height - end_line_height - 0.1).max(0.0);
        self.viewport.back_start_offset_y = (end_line_data.point().y - end_line_target_y).max(0.0);

        // Use the same forward position resolver as normal rendering so lines that share
        // the same document Y (e.g. table rows across columns) keep consistent screen Y.
        let mut page = self.build_forward_positions(self.viewport.back_start_offset_y);

        // If anchor line is not visible, find the smallest start offset that includes it.
        // This keeps the anchor as low as possible (near page bottom) instead of pinning top.
        if page.positions.get(end_line).and_then(|p| *p).is_none() {
            let mut lo = self.viewport.back_start_offset_y;
            let mut hi = end_line_data.point().y.max(lo);
            let mut best: Option<(PagePositions, f64)> = None;

            for _ in 0..20 {
                if (hi - lo) <= 0.01 {
                    break;
                }
                let mid = (lo + hi) * 0.5;
                let candidate = self.build_forward_positions(mid);
                if candidate.positions.get(end_line).and_then(|p| *p).is_some() {
                    best = Some((candidate, mid));
                    hi = mid;
                } else {
                    lo = mid;
                }
            }

            if let Some((best_page, start)) = best {
                self.viewport.back_start_offset_y = start;
                page = best_page;
            }
        }
        let first_painted = page.start_line;
        let paint_end = page.last_line.unwrap_or(first_painted).min(end_line);
        let first_painted_y = lines.get(first_painted).map(|line| line.point().y).unwrap_or(0.0);
        let render_state = self.paint_positioned_page(cx, page, self.viewport.back_start_offset_y, Some(paint_end), true);

        self.viewport.start_offset_y = first_painted_y;
        self.viewport.next_start_offset_y = self.viewport.end_offset_y;

        if first_painted == 0 && self.viewport.start_offset_y <= 0.0 {
            self.viewport.direction = PaintDirection::Forward;
            self.viewport.start_offset_y = 0.0;
            self.viewport.end_offset_y = 0.0;
            self.viewport.back_anchor_end_line = None;
        }

        render_state
    }
}

fn color_from_u32(color: u32) -> Color {
    Color::rgba8((color >> 24) as u8, (color >> 16) as u8, (color >> 8) as u8, color as u8)
}
