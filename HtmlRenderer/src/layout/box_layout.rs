use floem::kurbo::{Point, Size};
use crate::document::{resolve_preffered_size, BoxType, Children, ComputedStyle, Display, PreferredSize};
use crate::layout::{get_style, list_marker_width, LayoutEngine};

impl LayoutEngine {
    pub fn layout_box(&mut self, box_idx: usize, available_width: f64, parent_content_height: Option<f64>, first_line_indent: f64, ) -> Size {
        let style = get_style(&self.document, box_idx);
        let list_marker_width = list_marker_width(&self.document, &self.glyph_cache, box_idx);

        let explicit_height = match style.height {
            PreferredSize::Auto => None,
            PreferredSize::Px(px) => Some(px as f64),
            PreferredSize::Percent(pct) => parent_content_height.map(|h| h * pct as f64),
        };


        let available_for_auto = available_width - style.get_horizontal_margin_padding();
        let initial_width = resolve_preffered_size(style.width, available_for_auto, available_width);
        let min_width = resolve_preffered_size(style.min_width, 0., available_width);
        let max_width = resolve_preffered_size(style.max_width,f64::INFINITY, available_width);
        let content_width = initial_width.clamp(min_width, max_width).max(0.0);

        // Calculate content position from box position (which parent already set)
        let box_point = self.document.boxes[box_idx].point;
        let content_pos = Point::new(
            box_point.x + style.padding_left as f64,
            box_point.y + style.padding_top as f64,
        );

        let content_size = match &self.document.boxes[box_idx].box_type {
            BoxType::Block(block) => match &block.children {
                Children::Blocks(range) => {
                    self.layout_block_children(box_idx, range.clone(), content_width, explicit_height, style.text_indent as f64, )
                },
                Children::Runs(range) => self.layout_runs(
                    range.clone(),
                    content_pos,
                    content_width,
                    style.text_indent as f64 - list_marker_width,
                    style.text_indent_hanging,
                    style.text_indent_each_line,
                    style.text_align,
                    style.text_align_last,
                ),
                Children::Empty => Size::ZERO,
            },
            BoxType::Inline(range) => self.layout_runs(
                range.clone(),
                content_pos,
                content_width,
                first_line_indent - list_marker_width,
                false,
                false,
                style.text_align,
                style.text_align_last,
            ),
            BoxType::Anonymous(range) => self.layout_runs(
                range.clone(),
                content_pos,
                content_width,
                first_line_indent - list_marker_width,
                false,
                false,
                style.text_align,
                style.text_align_last,
            ),
        };

        let mut content_height = match explicit_height {
            Some(explicit) => content_size.height.max(explicit),
            None => content_size.height,
        };
        // Apply min-height constraint
        let min_height_constraint = match style.min_height {
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
        let max_height_constraint = match style.max_height {
            PreferredSize::Auto => None,
            PreferredSize::Px(px) => Some(px as f64),
            PreferredSize::Percent(pct) => parent_content_height.map(|h| h * pct as f64),
        };
        if let Some(max_h) = max_height_constraint {
            if content_height > max_h {
                content_height = max_h;
            }
        }

        let used_content_width = match style.width {
            PreferredSize::Auto => content_size.width,
            _ => content_width,
        };
        

        let size = Size::new(used_content_width + style.get_horizontal_padding(), content_height + style.get_vertical_padding());
        self.document.boxes[box_idx].size = size;

        if matches!(&self.document.boxes[box_idx].box_type, BoxType::Block(_)) {
            self.emit_block_decorations(box_idx, &style);
        }
        size
    }
}
