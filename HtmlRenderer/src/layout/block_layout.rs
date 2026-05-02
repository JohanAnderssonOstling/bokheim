use crate::document::{BoxType, Children, PreferredSize};
use crate::layout::{LayoutEngine, get_style};
use floem::kurbo::{Size, Vec2};

#[derive(Clone, Copy, Default)]
struct CollapsedMargin {
    max_positive: f64,
    min_negative: f64,
}

impl CollapsedMargin {
    fn add(&mut self, margin: f64) {
        if margin >= 0.0 {
            self.max_positive = self.max_positive.max(margin);
        } else {
            self.min_negative = self.min_negative.min(margin);
        }
    }

    fn merge(&mut self, other: CollapsedMargin) {
        self.max_positive = self.max_positive.max(other.max_positive);
        self.min_negative = self.min_negative.min(other.min_negative);
    }

    fn resolve(self) -> f64 {
        self.max_positive + self.min_negative
    }

    fn is_empty(self) -> bool {
        self.max_positive == 0.0 && self.min_negative == 0.0
    }
}

struct ParentCollapseContext {
    top_open: bool,
    bottom_open: bool,
}

struct BlockFlowItem {
    margin_top: CollapsedMargin,
    margin_left: f64,
    margin_bottom: CollapsedMargin,
    horizontal_margin: f64,
    is_anonymous: bool,
    top_open: bool,
    self_collapsing_empty: bool,
}

impl LayoutEngine {
    pub(crate) fn layout_block_children(&mut self, parent_idx: usize, children: &[u32], available_width: f64, parent_content_height: Option<f64>, first_line_indent: f64) -> Size {
        let mut y_offset = 0.0;
        let mut max_width = 0.0f64;
        let mut pending_margin = CollapsedMargin::default();
        let mut indent_remaining = first_line_indent;
        let mut started_flow = false;

        let parent_style = get_style(&self.document, parent_idx);
        let parent_padding = parent_style.get_top_left_padding();
        let parent_ctx = ParentCollapseContext {
            top_open: parent_style.padding_top() == 0.0 && parent_style.border_top_width() == 0.0,
            bottom_open: parent_style.padding_bottom() == 0.0 && parent_style.border_bottom_width() == 0.0 && matches!(parent_style.height(), PreferredSize::Auto) && matches!(parent_style.min_height(), PreferredSize::Auto),
        };
        let position = self.document.get_box_point(parent_idx) + parent_padding;

        for &child_handle in children {
            let child_idx = child_handle as usize;
            if matches!(&self.document.boxes[child_idx].layout_mode, BoxType::Inline(_)) {
                continue;
            }

            let item = self.block_flow_item(child_idx);
            let top_collapses_with_parent = !started_flow && parent_ctx.top_open && !item.is_anonymous && item.top_open;

            // When collapsing with parent top, don't accumulate margins into pending —
            // they are already encoded in effective_top_margin visible to the grandparent.
            if !top_collapses_with_parent {
                pending_margin.merge(item.margin_top);
            }
            if !item.self_collapsing_empty && !top_collapses_with_parent {
                y_offset += pending_margin.resolve();
                pending_margin = CollapsedMargin::default();
            } else if !item.self_collapsing_empty {
                // top collapses with parent: child sits at y=0, pending stays clean
                pending_margin = CollapsedMargin::default();
            }

            let child_indent = calculate_child_indent(&self.document.boxes[child_idx].layout_mode, &mut indent_remaining);
            let child_pos = position + Vec2::new(item.margin_left, y_offset);
            self.document.boxes[child_idx].point = child_pos;
            let child_size = self.layout_box(child_idx, available_width, parent_content_height, child_indent);

            max_width = max_width.max(child_size.width + item.horizontal_margin);

            if item.self_collapsing_empty {
                if !top_collapses_with_parent {
                    pending_margin.merge(item.margin_bottom);
                }
                continue;
            }

            y_offset += child_size.height;
            pending_margin.merge(item.margin_bottom);
            started_flow = true;
        }

        if !pending_margin.is_empty() && !parent_ctx.bottom_open {
            y_offset += pending_margin.resolve();
        }

        Size::new(max_width, y_offset)
    }

    fn block_flow_item(&self, box_idx: usize) -> BlockFlowItem {
        let style = get_style(&self.document, box_idx);
        let layout_mode = &self.document.boxes[box_idx].layout_mode;
        let is_anonymous = matches!(layout_mode, BoxType::Anonymous(_));
        let has_children = match layout_mode {
            BoxType::Block(block) => !matches!(block.children, Children::Empty),
            BoxType::Table(table) => !table.rows.is_empty(),
            BoxType::TableRow(row) => !row.cells.is_empty(),
            BoxType::TableCell(cell) => !matches!(cell.children, Children::Empty),
            BoxType::Inline(range) | BoxType::Anonymous(range) => range.start < range.end,
        };
        let top_open = style.padding_top() == 0.0 && style.border_top_width() == 0.0;
        let bottom_open = style.padding_bottom() == 0.0 && style.border_bottom_width() == 0.0 && matches!(style.height(), PreferredSize::Auto) && matches!(style.min_height(), PreferredSize::Auto);
        let self_collapsing_empty = matches!(layout_mode, BoxType::Block(block) if matches!(block.children, Children::Empty)) && !has_children && top_open && bottom_open;

        let (margin_top, margin_bottom) = if is_anonymous { (CollapsedMargin::default(), CollapsedMargin::default()) } else { (self.effective_top_margin(box_idx), self.effective_bottom_margin(box_idx)) };

        BlockFlowItem { margin_top, margin_left: style.margin_left() as f64, margin_bottom, horizontal_margin: style.get_horizontal_margin(), is_anonymous, top_open, self_collapsing_empty }
    }

    /// The effective top margin of a box as seen by its parent's flow, collapsing through
    /// any open top edge and leading self-collapsing children per CSS §8.3.1.
    fn effective_top_margin(&self, box_idx: usize) -> CollapsedMargin {
        let style = get_style(&self.document, box_idx);
        let mut result = CollapsedMargin::default();
        result.add(style.margin_top() as f64);

        if style.padding_top() != 0.0 || style.border_top_width() != 0.0 {
            return result;
        }

        let indices = match &self.document.boxes[box_idx].layout_mode {
            BoxType::Block(block) => match &block.children {
                Children::Blocks(v) => v.clone(),
                _ => return result,
            },
            BoxType::Table(table) => table.rows.clone(),
            _ => return result,
        };

        for child_handle in indices {
            let child_idx = child_handle as usize;
            let cs = get_style(&self.document, child_idx);
            let child_top_open = cs.padding_top() == 0.0 && cs.border_top_width() == 0.0;
            let child_bottom_open = cs.padding_bottom() == 0.0 && cs.border_bottom_width() == 0.0 && matches!(cs.height(), PreferredSize::Auto) && matches!(cs.min_height(), PreferredSize::Auto);
            let mt = cs.margin_top() as f64;
            let mb = cs.margin_bottom() as f64;
            let (is_inline, is_anon, is_self_collapsing) = {
                let mode = &self.document.boxes[child_idx].layout_mode;
                (matches!(mode, BoxType::Inline(_)), matches!(mode, BoxType::Anonymous(_)), matches!(mode, BoxType::Block(b) if matches!(b.children, Children::Empty)) && child_top_open && child_bottom_open)
            };

            if is_inline {
                continue;
            }
            if is_anon {
                break;
            }
            if is_self_collapsing {
                result.add(mt);
                result.add(mb);
                // continue: next sibling may also collapse through to the parent top
            } else if child_top_open {
                result.merge(self.effective_top_margin(child_idx));
                break;
            } else {
                break;
            }
        }

        result
    }

    /// The effective bottom margin of a box as seen by its parent's flow, collapsing through
    /// any open bottom edge and trailing self-collapsing children per CSS §8.3.1.
    fn effective_bottom_margin(&self, box_idx: usize) -> CollapsedMargin {
        let style = get_style(&self.document, box_idx);
        let bottom_open = style.padding_bottom() == 0.0 && style.border_bottom_width() == 0.0 && matches!(style.height(), PreferredSize::Auto) && matches!(style.min_height(), PreferredSize::Auto);

        let mut result = CollapsedMargin::default();
        result.add(style.margin_bottom() as f64);

        if !bottom_open {
            return result;
        }

        let indices = match &self.document.boxes[box_idx].layout_mode {
            BoxType::Block(block) => match &block.children {
                Children::Blocks(v) => v.clone(),
                _ => return result,
            },
            BoxType::Table(table) => table.rows.clone(),
            _ => return result,
        };

        for &child_handle in indices.iter().rev() {
            let child_idx = child_handle as usize;
            let cs = get_style(&self.document, child_idx);
            let child_top_open = cs.padding_top() == 0.0 && cs.border_top_width() == 0.0;
            let child_bottom_open = cs.padding_bottom() == 0.0 && cs.border_bottom_width() == 0.0 && matches!(cs.height(), PreferredSize::Auto) && matches!(cs.min_height(), PreferredSize::Auto);
            let mt = cs.margin_top() as f64;
            let mb = cs.margin_bottom() as f64;
            let (is_inline, is_anon, is_self_collapsing) = {
                let mode = &self.document.boxes[child_idx].layout_mode;
                (matches!(mode, BoxType::Inline(_)), matches!(mode, BoxType::Anonymous(_)), matches!(mode, BoxType::Block(b) if matches!(b.children, Children::Empty)) && child_top_open && child_bottom_open)
            };

            if is_inline {
                continue;
            }
            if is_anon {
                break;
            }
            if is_self_collapsing {
                result.add(mb);
                result.add(mt);
                // continue: previous sibling may also collapse through to the parent bottom
            } else if child_bottom_open {
                result.merge(self.effective_bottom_margin(child_idx));
                break;
            } else {
                break;
            }
        }

        result
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
