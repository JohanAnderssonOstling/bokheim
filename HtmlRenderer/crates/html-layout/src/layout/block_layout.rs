use crate::document::{BoxType, Children, Clear, Float, PreferredSize};
use crate::layout::{ClearSide, FloatBand, FloatSide, LayoutContext};
use kurbo::{Size, Vec2};
use std::time::Instant;

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

impl<'a, 'out> LayoutContext<'a, 'out> {
    /// `available_width` is the content width of `parent_idx` — i.e. the containing
    /// block inline-size for the children, against which their margin percentages
    /// resolve. `parent_cb_width` is the parent's *own* containing-block width, used
    /// only to resolve the parent's percentage padding when placing the content origin.
    pub(crate) fn layout_block_children(&mut self, parent_idx: usize, children: &[u32], available_width: f64, parent_cb_width: f64, parent_content_height: Option<f64>, first_line_indent: f64) -> Size {
        let timing_started = Instant::now();
        let mut y_offset = 0.0;
        let mut max_width = 0.0f64;
        let mut pending_margin = CollapsedMargin::default();
        let mut indent_remaining = first_line_indent;
        let mut started_flow = false;

        let parent_style = self.style(parent_idx);
        // Children start inside the parent's border and padding (the parent's box
        // point is its border-box origin).
        let parent_padding = parent_style.get_top_left_padding(parent_cb_width) + Vec2::new(parent_style.border_left_width() as f64, parent_style.border_top_width() as f64);
        let parent_ctx = ParentCollapseContext {
            top_open: parent_style.padding_top().is_zero() && parent_style.border_top_width() == 0.0,
            bottom_open: parent_style.padding_bottom().is_zero() && parent_style.border_bottom_width() == 0.0 && matches!(parent_style.height(), PreferredSize::Auto) && matches!(parent_style.min_height(), PreferredSize::Auto),
        };
        let position = self.get_box_point(parent_idx) + parent_padding;

        for &child_handle in children {
            let child_idx = child_handle as usize;
            if self.is_inline_box(child_idx) {
                continue;
            }

            let item = self.block_flow_item(child_idx, available_width);
            let child_float = self.box_float(child_idx);
            let child_clear = self.box_clear(child_idx);
            let top_collapses_with_parent = !started_flow && parent_ctx.top_open && !item.is_anonymous && item.top_open;

            if matches!(child_clear, Clear::Left | Clear::Right | Clear::Both) {
                let side = match child_clear {
                    Clear::Left => ClearSide::Left,
                    Clear::Right => ClearSide::Right,
                    Clear::Both => ClearSide::Both,
                    Clear::None => unreachable!(),
                };
                if let Some(ctx) = self.current_float_context() {
                    let cleared = ctx.clear_to(position.y + y_offset, side);
                    if cleared > position.y + y_offset {
                        y_offset = cleared - position.y;
                    }
                }
            }

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

            let child_indent = self.box_layout_mode(child_idx).map(|mode| calculate_child_indent(mode, &mut indent_remaining)).unwrap_or(0.0);
            let float_band = self.current_float_context().map(|ctx| ctx.available(position.y + y_offset, self.line_height.max(1.0), position.x, position.x + available_width)).unwrap_or((position.x, position.x + available_width));
            let mut child_pos = position + Vec2::new(item.margin_left, y_offset);
            if matches!(child_float, Float::Left | Float::Right) {
                child_pos.x = float_band.0;
            }
            self.set_box_point(child_idx, child_pos);
            let child_size = self.layout_box(child_idx, available_width, parent_content_height, child_indent);

            max_width = max_width.max(child_size.width + item.horizontal_margin);

            if matches!(child_float, Float::Left | Float::Right) {
                let final_x = if matches!(child_float, Float::Right) { (position.x + available_width - child_size.width).max(float_band.0) } else { child_pos.x };
                if final_x != child_pos.x {
                    self.set_box_point(child_idx, kurbo::Point::new(final_x, child_pos.y));
                }
                if let Some(ctx) = self.current_float_context_mut() {
                    ctx.add_band(FloatBand {
                        left: final_x,
                        right: final_x + child_size.width,
                        top: child_pos.y,
                        bottom: child_pos.y + child_size.height,
                        side: if matches!(child_float, Float::Right) { FloatSide::Right } else { FloatSide::Left },
                    });
                }
                continue;
            }

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

        let size = Size::new(max_width, y_offset);
        self.record_timing(|t| t.layout_block_children += timing_started.elapsed());
        size
    }

    /// `cb_width` is the containing-block inline-size for `box_idx` (its parent's
    /// content width), against which this box's margin percentages resolve.
    fn block_flow_item(&self, box_idx: usize, cb_width: f64) -> BlockFlowItem {
        let style = self.style(box_idx);
        let layout_mode = self.box_layout_mode(box_idx).expect("layout box should exist");
        let is_anonymous = matches!(layout_mode, BoxType::Anonymous(_));
        let has_children = match layout_mode {
            BoxType::Block(block) => !matches!(block.children, Children::Empty),
            BoxType::Table(table) => !table.rows.is_empty(),
            BoxType::TableRow(row) => !row.cells.is_empty(),
            BoxType::TableCell(cell) => !matches!(cell.children, Children::Empty),
            BoxType::Inline(range) | BoxType::Anonymous(range) => range.start < range.end,
        };
        let top_open = style.padding_top().is_zero() && style.border_top_width() == 0.0;
        let bottom_open = style.padding_bottom().is_zero() && style.border_bottom_width() == 0.0 && matches!(style.height(), PreferredSize::Auto) && matches!(style.min_height(), PreferredSize::Auto);
        let self_collapsing_empty = matches!(layout_mode, BoxType::Block(block) if matches!(block.children, Children::Empty)) && !has_children && top_open && bottom_open;

        let (margin_top, margin_bottom) = if is_anonymous { (CollapsedMargin::default(), CollapsedMargin::default()) } else { (self.effective_top_margin(box_idx, cb_width), self.effective_bottom_margin(box_idx, cb_width)) };

        BlockFlowItem { margin_top, margin_left: style.margin_left().resolve(cb_width), margin_bottom, horizontal_margin: style.get_horizontal_margin(cb_width), is_anonymous, top_open, self_collapsing_empty }
    }

    /// The effective top margin of a box as seen by its parent's flow, collapsing through
    /// any open top edge and leading self-collapsing children per CSS §8.3.1.
    fn effective_top_margin(&self, box_idx: usize, cb_width: f64) -> CollapsedMargin {
        let style = self.style(box_idx);
        let mut result = CollapsedMargin::default();
        result.add(style.margin_top().resolve(cb_width));

        if !style.padding_top().is_zero() || style.border_top_width() != 0.0 {
            return result;
        }

        let indices = match self.box_layout_mode(box_idx) {
            Some(BoxType::Block(block)) => match &block.children {
                Children::Blocks(v) => v.clone(),
                _ => return result,
            },
            Some(BoxType::Table(table)) => table.rows.clone(),
            _ => return result,
        };

        for child_handle in indices {
            let child_idx = child_handle as usize;
            let cs = self.style(child_idx);
            let child_top_open = cs.padding_top().is_zero() && cs.border_top_width() == 0.0;
            let child_bottom_open = cs.padding_bottom().is_zero() && cs.border_bottom_width() == 0.0 && matches!(cs.height(), PreferredSize::Auto) && matches!(cs.min_height(), PreferredSize::Auto);
            let mt = cs.margin_top().resolve(cb_width);
            let mb = cs.margin_bottom().resolve(cb_width);
            let (is_inline, is_anon, is_self_collapsing) = {
                let mode = self.box_layout_mode(child_idx).expect("layout box should exist");
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
                result.merge(self.effective_top_margin(child_idx, cb_width));
                break;
            } else {
                break;
            }
        }

        result
    }

    /// The effective bottom margin of a box as seen by its parent's flow, collapsing through
    /// any open bottom edge and trailing self-collapsing children per CSS §8.3.1.
    fn effective_bottom_margin(&self, box_idx: usize, cb_width: f64) -> CollapsedMargin {
        let style = self.style(box_idx);
        let bottom_open = style.padding_bottom().is_zero() && style.border_bottom_width() == 0.0 && matches!(style.height(), PreferredSize::Auto) && matches!(style.min_height(), PreferredSize::Auto);

        let mut result = CollapsedMargin::default();
        result.add(style.margin_bottom().resolve(cb_width));

        if !bottom_open {
            return result;
        }

        let indices = match self.box_layout_mode(box_idx) {
            Some(BoxType::Block(block)) => match &block.children {
                Children::Blocks(v) => v.clone(),
                _ => return result,
            },
            Some(BoxType::Table(table)) => table.rows.clone(),
            _ => return result,
        };

        for &child_handle in indices.iter().rev() {
            let child_idx = child_handle as usize;
            let cs = self.style(child_idx);
            let child_top_open = cs.padding_top().is_zero() && cs.border_top_width() == 0.0;
            let child_bottom_open = cs.padding_bottom().is_zero() && cs.border_bottom_width() == 0.0 && matches!(cs.height(), PreferredSize::Auto) && matches!(cs.min_height(), PreferredSize::Auto);
            let mt = cs.margin_top().resolve(cb_width);
            let mb = cs.margin_bottom().resolve(cb_width);
            let (is_inline, is_anon, is_self_collapsing) = {
                let mode = self.box_layout_mode(child_idx).expect("layout box should exist");
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
                result.merge(self.effective_bottom_margin(child_idx, cb_width));
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
