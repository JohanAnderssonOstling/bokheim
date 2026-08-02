use crate::document::{BorderCollapseMode, BoxSizing, BoxType, Children, Float, InlineKind, LayoutMode, ListItemMarker, ListStylePosition, PreferredSize, resolve_preffered_size};
use crate::layout::LayoutContext;
use kurbo::{Point, Size};
use std::time::Instant;

/// `border_box_inset` is the padding+border to subtract from a specified size
/// under `box-sizing: border-box` (0 for `content-box`); `auto` is unaffected.
fn resolve_vertical_size(size: PreferredSize, parent_content_height: Option<f64>, border_box_inset: f64) -> Option<f64> {
    match size {
        PreferredSize::Auto => None,
        PreferredSize::Px(px) => Some((px as f64 - border_box_inset).max(0.0)),
        PreferredSize::Percent(pct) => parent_content_height.map(|h| (h * pct as f64 - border_box_inset).max(0.0)),
    }
}

fn resolve_content_width(width: PreferredSize, min_width: PreferredSize, max_width: PreferredSize, horizontal_margin_padding: f64, horizontal_border: f64, available_width: f64, border_box_inset: f64) -> f64 {
    let available_for_auto = available_width - horizontal_margin_padding - horizontal_border;
    // Specified (non-auto) sizes are border-box sizes under border-box sizing,
    // so the padding+border inset converts them to content-box sizes.
    let resolve = |size: PreferredSize, auto: f64| match size {
        PreferredSize::Auto => auto,
        _ => (resolve_preffered_size(size, auto, available_width) - border_box_inset).max(0.0),
    };
    let initial_width = resolve(width, available_for_auto);
    let min_width = resolve(min_width, 0.);
    let max_width = resolve(max_width, f64::INFINITY);
    initial_width.clamp(min_width, max_width).max(0.0)
}

impl<'a, 'out> LayoutContext<'a, 'out> {
    pub fn layout_box(&mut self, box_idx: usize, available_width: f64, parent_content_height: Option<f64>, first_line_indent: f64) -> Size {
        let timing_started = Instant::now();
        let style = self.style(box_idx);
        let is_image_box = self.get_box_image_idx(box_idx).is_some();
        let is_table_cell = self.is_table_cell_box(box_idx);
        let is_float = matches!(style.float(), Float::Left | Float::Right);
        let replaced_or_cell = is_image_box || is_table_cell;
        let width = if replaced_or_cell { PreferredSize::Auto } else { style.width() };
        let min_width = if replaced_or_cell { PreferredSize::Auto } else { style.min_width() };
        let max_width = if replaced_or_cell { PreferredSize::Auto } else { style.max_width() };
        let border_left = style.border_left_width() as f64;
        let border_top = style.border_top_width() as f64;
        let horizontal_border = border_left + style.border_right_width() as f64;
        let vertical_border = style.border_top_width() as f64 + style.border_bottom_width() as f64;
        let horizontal_padding = style.get_horizontal_padding(available_width);
        let vertical_padding = style.get_vertical_padding(available_width);
        let (horizontal_border_box_inset, vertical_border_box_inset) = match style.box_sizing() {
            BoxSizing::ContentBox => (0.0, 0.0),
            BoxSizing::BorderBox => (horizontal_padding + horizontal_border, vertical_padding + vertical_border),
        };

        // Generated list marker (::marker). `inside` markers take inline advance
        // on the first line (added to its indent); `outside` markers hang in the
        // start padding and are positioned after the content is laid out.
        let marker = self.list_marker(box_idx);
        let marker_width = marker.map(|m| self.measure_marker_width(m.marker_box as usize)).unwrap_or(0.0);
        // Gap between the marker and the item's text, sized to the font like a
        // browser's default marker spacing.
        let marker_gap = if marker.is_some() { 0.5 * style.font_size() as f64 } else { 0.0 };
        let inside_marker_indent = marker.filter(|m| matches!(m.position, ListStylePosition::Inside)).map(|_| marker_width + marker_gap).unwrap_or(0.0);
        let first_content_line_idx = self.lines().len();
        let explicit_height = resolve_vertical_size(style.height(), parent_content_height, vertical_border_box_inset);
        let content_width = resolve_content_width(width, min_width, max_width, style.get_horizontal_margin_padding(available_width), horizontal_border, available_width, horizontal_border_box_inset);

        // Calculate content position from box position (which parent already set).
        // Padding percentages resolve against this box's containing-block width.
        let box_point = self.get_box_point(box_idx);
        let content_pos = Point::new(box_point.x + style.padding_left().resolve(available_width) + border_left, box_point.y + style.padding_top().resolve(available_width) + border_top);

        // Float-owned boxes are measured in the parent float context. Only the
        // surrounding flow containers need a fresh nested context here.
        let pushed_float_context = self.box_uses_float_context(box_idx) && !is_float;
        if pushed_float_context {
            self.push_float_context();
        }

        let content_size = match self.box_layout_mode(box_idx).expect("layout box should exist") {
            LayoutMode::Block(block) => match &block.children {
                Children::Blocks(indices) => {
                    let indices = indices.clone();
                    self.layout_block_children(box_idx, &indices, content_width, available_width, explicit_height, style.text_indent().resolve(content_width) + inside_marker_indent)
                }
                Children::Runs(range) => self.layout_runs(
                    range.clone(),
                    content_pos,
                    content_width,
                    style.text_indent().resolve(content_width) + inside_marker_indent,
                    style.text_indent_hanging(),
                    style.text_indent_each_line(),
                    style.text_align(),
                    style.text_align_last(),
                    is_float,
                ),
                Children::Empty => Size::ZERO,
            },
            LayoutMode::Table(table) => self.layout_table(box_idx, table.rows.clone(), content_pos, content_width, parent_content_height),
            LayoutMode::TableRow(row) => self.layout_table_row(row.cells.clone(), content_pos, content_width, parent_content_height),
            LayoutMode::TableCell(cell) => match &cell.children {
                Children::Blocks(indices) => {
                    let indices = indices.clone();
                    self.layout_block_children(box_idx, &indices, content_width, available_width, explicit_height, style.text_indent().resolve(content_width))
                }
                Children::Runs(range) => {
                    self.layout_runs(range.clone(), content_pos, content_width, style.text_indent().resolve(content_width), style.text_indent_hanging(), style.text_indent_each_line(), style.text_align(), style.text_align_last(), is_float)
                }
                Children::Empty => Size::ZERO,
            },
            BoxType::Inline(range) => self.layout_runs(range.clone(), content_pos, content_width, first_line_indent, false, false, style.text_align(), style.text_align_last(), is_float),
            BoxType::Anonymous(range) => self.layout_runs(range.clone(), content_pos, content_width, first_line_indent, false, false, style.text_align(), style.text_align_last(), is_float),
        };

        // Position and lay out the generated marker now that the item's content
        // (and thus its first line) exists.
        if let Some(marker) = marker {
            self.place_list_marker(marker, marker_width, marker_gap, content_pos, first_content_line_idx);
        }

        if pushed_float_context {
            self.pop_float_context();
        }

        let mut content_height = match explicit_height {
            Some(explicit) => content_size.height.max(explicit),
            None => content_size.height,
        };
        // Apply min-height constraint
        let min_height_constraint = resolve_vertical_size(style.min_height(), parent_content_height, vertical_border_box_inset);
        if let Some(min_h) = min_height_constraint {
            if content_height < min_h {
                content_height = min_h;
            }
        }
        // Apply max-height constraint
        let max_height_constraint = resolve_vertical_size(style.max_height(), parent_content_height, vertical_border_box_inset);
        if let Some(max_h) = max_height_constraint {
            if content_height > max_h {
                content_height = max_h;
            }
        }

        // A normal-flow block-level box with `width: auto` fills its containing
        // block (CSS 2.1 §10.3.3) rather than shrinking to content. Replaced
        // images keep their intrinsic/scaled width, and shrink-to-fit contexts
        // (tables, inline boxes) keep the measured content width.
        let fills_available_width = !is_image_box && !is_float && (self.is_block_box(box_idx) || self.is_anonymous_box(box_idx));
        let used_content_width = match width {
            PreferredSize::Auto if fills_available_width => content_width,
            PreferredSize::Auto => content_size.width,
            _ => content_width,
        };

        let size = Size::new(used_content_width + horizontal_padding + horizontal_border, content_height + vertical_padding + vertical_border);
        self.set_box_size(box_idx, size);

        if self.is_block_box(box_idx) {
            self.emit_block_decorations(box_idx);
        } else if self.is_table_box(box_idx) {
            {
                if matches!(style.border_collapse(), BorderCollapseMode::Collapse) {
                    self.emit_block_background(box_idx);
                } else {
                    self.emit_block_decorations(box_idx);
                }
            }
        }
        self.record_timing(|t| t.layout_box_total += timing_started.elapsed());
        size
    }

    /// Sum of the marker box's shaped glyph advances. Used both to reserve
    /// inline space for `inside` markers and to hang `outside` markers to the
    /// left of the content edge.
    fn measure_marker_width(&self, marker_box: usize) -> f64 {
        let Some(LayoutMode::Inline(range)) = self.box_layout_mode(marker_box) else {
            return 0.0;
        };
        let range = range.clone();
        let mut width = 0.0;
        for run_idx in range.start..range.end {
            let Some(run) = self.inline_run(run_idx as usize) else { continue };
            if let InlineKind::Marker { glyphs } = &run.kind {
                for g in glyphs.clone() {
                    if let Some(glyph_id) = self.glyph(g) {
                        width += self.glyph_metric(glyph_id).advance() as f64;
                    }
                }
            }
        }
        width
    }

    /// Position and lay out a list item's marker box. It is anchored to the top
    /// of the item's first content line so their baselines align (the marker
    /// inherits the item's font). `outside` markers hang to the left of the
    /// content edge; `inside` markers sit at the content edge, on the space the
    /// content's first-line indent reserved for them.
    fn place_list_marker(&mut self, marker: ListItemMarker, marker_width: f64, marker_gap: f64, content_pos: Point, first_content_line_idx: usize) {
        let line_y = self.lines().get(first_content_line_idx).map(|line| line.point.y).unwrap_or(content_pos.y);
        let marker_x = match marker.position {
            // Outside: hang left of the content edge, leaving the gap between the
            // marker and the text. Inside: sit at the content edge; the content's
            // first-line indent already reserved marker_width + gap after it.
            ListStylePosition::Outside => content_pos.x - marker_width - marker_gap,
            ListStylePosition::Inside => content_pos.x,
        };
        self.set_box_point(marker.marker_box as usize, Point::new(marker_x, line_y));
        let available = self.viewport_width.max(marker_width + 1.0);
        self.layout_box(marker.marker_box as usize, available, None, 0.0);
    }
}

#[cfg(test)]
mod tests {
    use crate::LaidOutDocument;
    use crate::parser::DocumentFactory;
    use crate::test_support::TestGlyphShaper as GlyphCache;
    use kurbo::Size;

    fn layout_html(html: &str, width: f64) -> LaidOutDocument {
        let mut factory = DocumentFactory::new();
        let mut glyph_cache = GlyphCache::new();
        factory.parse_with_new_pipeline(html, None).shape(&mut glyph_cache).expect("test shaper must register every glyph").layout(crate::LayoutConstraints::new(width, 16.0).unwrap())
    }

    fn div_size(document: &LaidOutDocument) -> Size {
        let div_idx = (0..document.box_count()).find(|&idx| document.get_tag(idx).eq_ignore_ascii_case("div")).expect("div box should exist");
        document.get_box_size(div_idx)
    }

    #[test]
    fn content_box_width_adds_padding_and_border() {
        let html = "<html><body><div style=\"width:100px; height:50px; padding:10px; border:5px solid black;\">x</div></body></html>";
        let size = div_size(&layout_html(html, 600.0));

        assert_eq!(size.width, 130.0, "content-box: border box = width + padding + border");
        assert_eq!(size.height, 80.0);
    }

    #[test]
    fn border_box_width_includes_padding_and_border() {
        let html = "<html><body><div style=\"width:100px; height:50px; padding:10px; border:5px solid black; box-sizing:border-box;\">x</div></body></html>";
        let size = div_size(&layout_html(html, 600.0));

        assert_eq!(size.width, 100.0, "border-box: specified width is the border-box width");
        assert_eq!(size.height, 50.0);
    }

    #[test]
    fn border_box_width_never_goes_negative() {
        // Padding+border exceed the specified width; content clamps to 0 and the
        // border box becomes just padding + border.
        let html = "<html><body><div style=\"width:10px; padding:10px; border:5px solid black; box-sizing:border-box;\"></div></body></html>";
        let size = div_size(&layout_html(html, 600.0));

        assert_eq!(size.width, 30.0, "content width clamps at 0; padding and border remain");
    }
}
