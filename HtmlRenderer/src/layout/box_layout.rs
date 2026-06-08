use crate::document::{BorderCollapseMode, BoxType, Children, LayoutMode, PreferredSize, resolve_preffered_size};
use crate::layout::{LayoutEngine, get_style, list_marker_width};
use floem::kurbo::{Point, Size};

impl LayoutEngine {
    pub fn layout_box(&mut self, box_idx: usize, available_width: f64, parent_content_height: Option<f64>, first_line_indent: f64) -> Size {
        // Extract all style values we need before any mutable operations
        let style = get_style(&self.document, box_idx);
        let is_image_box = self.document.get_box_image_idx(box_idx).is_some();
        let is_table_cell = self.document.is_table_cell_box(box_idx);

        let height_val = style.height();
        let width_val = if is_image_box || is_table_cell { PreferredSize::Auto } else { style.width() };
        let min_width_val = if is_image_box || is_table_cell { PreferredSize::Auto } else { style.min_width() };
        let max_width_val = if is_image_box || is_table_cell { PreferredSize::Auto } else { style.max_width() };
        let min_height_val = style.min_height();
        let max_height_val = style.max_height();
        let padding_left = style.padding_left() as f64;
        let padding_top = style.padding_top() as f64;
        let text_indent = style.text_indent() as f64;
        let text_indent_hanging = style.text_indent_hanging();
        let text_indent_each_line = style.text_indent_each_line();
        let text_align = style.text_align();
        let text_align_last = style.text_align_last();
        let horizontal_margin_padding = style.get_horizontal_margin_padding();
        let horizontal_padding = style.get_horizontal_padding();
        let vertical_padding = style.get_vertical_padding();
        let border_left = style.border_left_width() as f64;
        let border_top = style.border_top_width() as f64;
        let horizontal_border = border_left + style.border_right_width() as f64;
        let vertical_border = border_top + style.border_bottom_width() as f64;
        let border_collapse = style.border_collapse();

        let list_marker_width = list_marker_width(&self.document, &self.glyph_cache, box_idx);

        let explicit_height = match height_val {
            PreferredSize::Auto => None,
            PreferredSize::Px(px) => Some(px as f64),
            PreferredSize::Percent(pct) => parent_content_height.map(|h| h * pct as f64),
        };

        let available_for_auto = available_width - horizontal_margin_padding - horizontal_border;
        let initial_width = resolve_preffered_size(width_val, available_for_auto, available_width);
        let min_width = resolve_preffered_size(min_width_val, 0., available_width);
        let max_width = resolve_preffered_size(max_width_val, f64::INFINITY, available_width);
        let content_width = initial_width.clamp(min_width, max_width).max(0.0);

        // Calculate content position from box position (which parent already set)
        let box_point = self.document.get_box_point(box_idx);
        let content_pos = Point::new(box_point.x + padding_left + border_left, box_point.y + padding_top + border_top);

        let content_size = match self.document.box_layout_mode(box_idx).expect("layout box should exist") {
            LayoutMode::Block(block) => match &block.children {
                Children::Blocks(indices) => {
                    let indices = indices.clone();
                    self.layout_block_children(box_idx, &indices, content_width, explicit_height, text_indent)
                }
                Children::Runs(range) => self.layout_runs(range.clone(), content_pos, content_width, text_indent - list_marker_width, text_indent_hanging, text_indent_each_line, text_align, text_align_last),
                Children::Empty => Size::ZERO,
            },
            LayoutMode::Table(table) => self.layout_table(box_idx, table.rows.clone(), content_pos, content_width, parent_content_height),
            LayoutMode::TableRow(row) => self.layout_table_row(row.cells.clone(), content_pos, content_width, parent_content_height),
            LayoutMode::TableCell(cell) => match &cell.children {
                Children::Blocks(indices) => {
                    let indices = indices.clone();
                    self.layout_block_children(box_idx, &indices, content_width, explicit_height, text_indent)
                }
                Children::Runs(range) => self.layout_runs(range.clone(), content_pos, content_width, text_indent - list_marker_width, text_indent_hanging, text_indent_each_line, text_align, text_align_last),
                Children::Empty => Size::ZERO,
            },
            BoxType::Inline(range) => self.layout_runs(range.clone(), content_pos, content_width, first_line_indent - list_marker_width, false, false, text_align, text_align_last),
            BoxType::Anonymous(range) => self.layout_runs(range.clone(), content_pos, content_width, first_line_indent - list_marker_width, false, false, text_align, text_align_last),
        };

        let mut content_height = match explicit_height {
            Some(explicit) => content_size.height.max(explicit),
            None => content_size.height,
        };
        // Apply min-height constraint
        let min_height_constraint = match min_height_val {
            PreferredSize::Auto => None,
            PreferredSize::Px(px) => Some(px as f64),
            PreferredSize::Percent(pct) => parent_content_height.map(|h| h * pct as f64),
        };
        if let Some(min_h) = min_height_constraint {
            if content_height < min_h {
                content_height = min_h;
            }
        }
        // Apply max-height constraint
        let max_height_constraint = match max_height_val {
            PreferredSize::Auto => None,
            PreferredSize::Px(px) => Some(px as f64),
            PreferredSize::Percent(pct) => parent_content_height.map(|h| h * pct as f64),
        };
        if let Some(max_h) = max_height_constraint {
            if content_height > max_h {
                content_height = max_h;
            }
        }

        // A normal-flow block-level box with `width: auto` fills its containing
        // block (CSS 2.1 §10.3.3) rather than shrinking to content. Replaced
        // images keep their intrinsic/scaled width, and shrink-to-fit contexts
        // (tables, inline boxes) keep the measured content width.
        let fills_available_width = !is_image_box && (self.document.is_block_box(box_idx) || self.document.is_anonymous_box(box_idx));
        let used_content_width = match width_val {
            PreferredSize::Auto if fills_available_width => content_width,
            PreferredSize::Auto => content_size.width,
            _ => content_width,
        };

        let size = Size::new(used_content_width + horizontal_padding + horizontal_border, content_height + vertical_padding + vertical_border);
        self.document.set_box_size(box_idx, size);

        if self.document.is_block_box(box_idx) {
            self.emit_block_decorations(box_idx);
        } else if self.document.is_table_box(box_idx) {
            {
                if matches!(border_collapse, BorderCollapseMode::Collapse) {
                    self.emit_block_background(box_idx);
                } else {
                    self.emit_block_decorations(box_idx);
                }
            }
        }
        size
    }
}
