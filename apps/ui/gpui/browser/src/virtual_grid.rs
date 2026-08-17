//! Shared geometry and row rendering for fixed-height virtual card grids.

use std::ops::Range;

use gpui::prelude::*;
use gpui::{Context, Div, ElementId, Entity, Render, Window, div, px};
use gpui_component::ElementExt as _;

/// Converts the resolved width of a grid into its logical column count.
///
/// The policy is evaluated after GPUI has laid out the grid's parent. This
/// keeps responsive structure dependent on the real content bounds instead of
/// guessing them from the window viewport during `Render`.
#[derive(Clone, Copy)]
pub(crate) enum ColumnPolicy {
    MinimumItemWidth { width: f32, gap: f32 },
}

impl ColumnPolicy {
    fn columns(self, width: f32) -> usize {
        match self {
            Self::MinimumItemWidth { width: item_width, gap } => columns_for_width(width, item_width, gap),
        }
    }
}

struct ResponsiveColumnsState {
    columns: usize,
    width: f32,
}

/// Retained grid geometry for an element whose actual width is only known after
/// parent layout. Cards and their row height consume the same measurement, so
/// neither can resize a frame before the other.
pub(crate) struct ResponsiveColumns {
    state: Entity<ResponsiveColumnsState>,
    policy: ColumnPolicy,
}

impl ResponsiveColumns {
    pub(crate) fn for_element<P: Render + 'static>(id: impl Into<ElementId>, policy: ColumnPolicy, window: &mut Window, cx: &mut Context<P>) -> Self {
        let state = window.use_keyed_state((id.into(), "responsive-columns"), cx, |_, _| ResponsiveColumnsState { columns: 1, width: 0.0 });
        Self { state, policy }
    }

    pub(crate) fn columns(&self, cx: &Context<impl Render>) -> usize {
        self.state.read(cx).columns
    }

    pub(crate) fn width(&self, cx: &Context<impl Render>) -> f32 {
        self.state.read(cx).width
    }

    pub(crate) fn geometry(&self, item_count: usize, maximum_single_row_width: f32, cx: &Context<impl Render>) -> (usize, f32) {
        let state = self.state.read(cx);
        match self.policy {
            ColumnPolicy::MinimumItemWidth { width: minimum, gap } => grid_geometry(state.width, state.columns, item_count, minimum, maximum_single_row_width, gap),
        }
    }

    pub(crate) fn track(&self, element: Div) -> Div {
        let state = self.state.clone();
        let policy = self.policy;
        element.on_prepaint(move |bounds, _, cx| {
            let width = f32::from(bounds.size.width);
            let columns = policy.columns(width);
            let current = state.read(cx);
            if current.columns != columns || (current.width - width).abs() >= 1.0 {
                state.update(cx, |state, cx| {
                    state.columns = columns;
                    state.width = width;
                    cx.notify();
                });
            }
        })
    }

    /// Hosts a viewport-filling child whose height is supplied by flex growth.
    ///
    /// Keeping this wrapper here prevents responsive virtual grids from
    /// accidentally placing a `flex_1` list under a non-flex parent, which
    /// resolves the list to zero height even when it contains rows.
    pub(crate) fn track_virtual_viewport(&self, content: impl IntoElement) -> Div {
        self.track(div().size_full().min_h_0().min_w_0().flex().flex_col().child(content))
    }
}

pub(crate) fn columns_for_width(width: f32, minimum_item_width: f32, gap: f32) -> usize {
    (((width + gap) / (minimum_item_width + gap)).floor() as usize).max(1)
}

/// Resolves a fixed-width grid. Viewport width changes the number of columns,
/// never the card geometry; an incomplete row simply retains empty slots.
pub(crate) fn grid_geometry(_width: f32, capacity: usize, _item_count: usize, minimum_item_width: f32, _maximum_single_row_width: f32, _gap: f32) -> (usize, f32) {
    (capacity.max(1), minimum_item_width)
}

pub(crate) fn row_count(item_count: usize, columns: usize) -> usize {
    item_count.div_ceil(columns.max(1))
}

pub(crate) fn row_range(row: usize, columns: usize, item_count: usize) -> Range<usize> {
    let start = row.saturating_mul(columns);
    start.min(item_count)..(start + columns).min(item_count)
}

/// A row whose children consume the width assigned by their parent. Empty
/// slots keep the final partial row aligned with preceding rows without any
/// viewport-derived pixel widths.
fn spanned_item_width(span: usize, unit_width: f32, gap: f32) -> f32 {
    unit_width * span as f32 + gap * span.saturating_sub(1) as f32
}

/// A fixed-width row for heterogeneous cards. Multi-unit cards include the
/// gaps of the units they replace, while unused units are retained as an
/// invisible spacer. Consequently viewport remainder changes only the outer
/// margins, never card width or row alignment.
pub(crate) fn fixed_spanned_entity_row<T: Render + 'static>(cards: &[(Entity<T>, usize)], units: usize, unit_width: f32, height: f32, gap: f32) -> Div {
    let occupied = cards.iter().map(|(_, span)| *span).sum::<usize>().min(units);
    div()
        .w_full()
        .h(px(height))
        .flex_none()
        .flex()
        .flex_row()
        .justify_center()
        .gap(px(gap))
        .children(cards.iter().map(|(card, span)| div().w(px(spanned_item_width(*span, unit_width, gap))).h(px(height)).flex_none().child(card.clone())))
        .when(occupied < units, |row| row.child(div().w(px(spanned_item_width(units - occupied, unit_width, gap))).h(px(height)).flex_none()))
}

pub(crate) fn flexible_entity_row<T: Render + 'static>(cards: &[Entity<T>], range: Range<usize>, columns: usize, item_width: f32, height: f32, gap: f32) -> Div {
    let item_count = range.len();
    div()
        .w_full()
        .h(px(height))
        .flex_none()
        .flex()
        .flex_row()
        // Page padding and the inner gap remain stable. Every column consumes
        // an equal share of the remaining width, like hierarchy cards.
        .gap(px(gap))
        .children(cards[range].iter().cloned().map(move |card| div().w(px(item_width)).h(px(height)).flex_none().child(card)))
        .children((item_count..columns).map(move |_| div().w(px(item_width)).h(px(height)).flex_none()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_geometry_chunks_items_without_dropping_the_last_partial_row() {
        assert_eq!(columns_for_width(668.0, 204.0, 28.0), 3);
        assert_eq!(row_count(8, 3), 3);
        assert_eq!(row_range(0, 3, 8), 0..3);
        assert_eq!(row_range(2, 3, 8), 6..8);
    }

    #[test]
    fn responsive_item_width_accounts_for_inter_column_gaps() {
        assert_eq!(columns_for_width(1232.0, 400.0, 16.0), 3);
    }

    #[test]
    fn responsive_columns_have_no_artificial_upper_limit() {
        assert_eq!(columns_for_width(10_000.0, 202.0, 16.0), 45);
    }

    #[test]
    fn book_cards_remain_fixed_width_in_partial_rows() {
        assert_eq!(grid_geometry(1200.0, 5, 2, 200.0, 428.0, 28.0), (5, 200.0));
        assert_eq!(grid_geometry(656.0, 3, 3, 200.0, 428.0, 28.0), (3, 200.0));
    }

    #[test]
    fn spanned_cards_replace_units_without_changing_unit_width() {
        assert_eq!(spanned_item_width(1, 202.0, 16.0), 202.0);
        assert_eq!(spanned_item_width(2, 202.0, 16.0), 420.0);
    }
}
