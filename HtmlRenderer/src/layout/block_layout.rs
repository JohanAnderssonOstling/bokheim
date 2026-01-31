use std::ops::Range;
use floem::kurbo::{Point, Size, Vec2};
use crate::document::{BoxType, ComputedStyle};
use crate::layout::{get_style, LayoutEngine};

impl LayoutEngine {
    pub(crate) fn layout_block_children(&mut self, parent_idx: usize, range: Range<u32>, available_width: f64, parent_content_height: Option<f64>, first_line_indent: f64) -> Size {
        let mut y_offset = 0.0;
        let mut max_width = 0.0f64;
        let mut prev_margin_bottom = 0.0f64;
        let mut indent_remaining = first_line_indent;
        let mut is_first_child = true;

        let parent_style = get_style(&self.document, parent_idx);
        let position = self.document.get_box_point(parent_idx) + parent_style.get_top_left_padding();

        for i in range {
            let child_idx = i as usize;
            let child = &self.document.boxes[child_idx];

            match &child.box_type {
                BoxType::Inline(_) => {
                    // Inline boxes are laid out via their anonymous inline run container.
                    continue;
                }
                _ => {}
            }
            let style = get_style(&self.document, child_idx);

            let child_is_anonymous = matches!(&child.box_type, BoxType::Anonymous(_));
            let collapsed_margin = collapse_margins(prev_margin_bottom, style.margin_top as f64, is_first_child, child_is_anonymous, &parent_style);
            y_offset += collapsed_margin;

            let child_indent = calculate_child_indent(&child.box_type, &mut indent_remaining);
            let child_pos = position + Vec2::new(style.margin_left as f64, y_offset);
            self.document.boxes[child_idx].point = child_pos;
            let child_size = self.layout_box(child_idx, available_width, parent_content_height, child_indent);
            y_offset += child_size.height;

            max_width = max_width.max(child_size.width + style.get_horizontal_margin());
            prev_margin_bottom = style.margin_bottom as f64;
            is_first_child = false;
        }

        Size::new(max_width, y_offset)
    }
}

fn collapse_margins(prev_margin: f64, curr_margin: f64, is_first: bool, child_is_anonymous: bool, parent_style: &ComputedStyle) -> f64 {
    if is_first {
        // Check if parent-child margin collapse should occur
        let should_collapse_with_parent = !child_is_anonymous
            && parent_style.should_parent_sibling_collapse();

        if should_collapse_with_parent {
            // Parent-child margins collapse: the collapsed margin was already applied
            // to the parent's position, so don't add additional spacing here.
            // The actual collapsed value (max/min/sum of parent.margin_top and curr_margin)
            // should have been applied when positioning the parent.
            0.0
        } else {
            // No collapse with parent: use child's top margin
            curr_margin
        }
    } else if prev_margin >= 0.0 && curr_margin >= 0.0 {
        // Both positive: take maximum
        prev_margin.max(curr_margin)
    } else if prev_margin <= 0.0 && curr_margin <= 0.0 {
        // Both negative: take minimum (most negative)
        prev_margin.min(curr_margin)
    } else {
        // Mixed positive/negative: sum them
        prev_margin + curr_margin
    }
}

fn calculate_child_indent(box_type: &BoxType, indent_remaining: &mut f64) -> f64 {
    match box_type {
        BoxType::Anonymous(_) => {
            let indent = *indent_remaining;
            *indent_remaining = 0.0;
            indent
        }
        _ => {
            *indent_remaining = 0.0;
            0.0
        }
    }
}