//! Shared geometry and row rendering for fixed-height virtual card grids.

use std::ops::Range;

use gpui::prelude::*;
use gpui::{Div, Entity, Render, div, px};

pub(crate) fn columns_for_width(width: f32, minimum_item_width: f32, gap: f32) -> usize {
    (((width + gap) / (minimum_item_width + gap)).floor() as usize).max(1)
}

pub(crate) fn item_width(width: f32, columns: usize, gap: f32) -> f32 {
    ((width - gap * columns.saturating_sub(1) as f32) / columns as f32).max(0.0)
}

/// Returns the spacing which divides all unused horizontal room evenly: once
/// between each item and once at either outer edge of the page.
///
/// `width` is the space inside the page's existing padding, so that padding is
/// added back before the remainder is divided. The returned inset is only the
/// extra padding the row needs on top of the page padding it already has.
pub(crate) fn even_spacing(width: f32, columns: usize, item_width: f32, page_padding: f32) -> (f32, f32) {
    let columns = columns.max(1);
    let spacing = ((width + page_padding * 2.0 - item_width * columns as f32) / (columns + 1) as f32).max(0.0);
    (spacing, (spacing - page_padding).max(0.0))
}

pub(crate) fn row_count(item_count: usize, columns: usize) -> usize {
    item_count.div_ceil(columns.max(1))
}

pub(crate) fn row_range(row: usize, columns: usize, item_count: usize) -> Range<usize> {
    let start = row.saturating_mul(columns);
    start.min(item_count)..(start + columns).min(item_count)
}

pub(crate) fn entity_row<T: Render + 'static>(cards: &[Entity<T>], range: Range<usize>, width: f32, height: f32, gap: f32) -> Div {
    div().w_full().h(px(height)).flex_none().flex().flex_row().gap(px(gap)).children(cards[range].iter().cloned().map(|card| div().w(px(width)).h(px(height)).flex_none().child(card)))
}

pub(crate) fn evenly_spaced_entity_row<T: Render + 'static>(cards: &[Entity<T>], range: Range<usize>, width: f32, height: f32, gap: f32, inset: f32) -> Div {
    entity_row(cards, range, width, height, gap).px(px(inset))
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
        assert_eq!(item_width(1232.0, 3, 16.0), 400.0);
        assert_eq!(columns_for_width(1232.0, 400.0, 16.0), 3);
    }

    #[test]
    fn even_spacing_includes_both_existing_page_edges() {
        // 668px of content plus the existing 18px padding on either side is
        // 704px. Three 204px covers leave 92px for four equal spaces.
        assert_eq!(even_spacing(668.0, 3, 204.0, 18.0), (23.0, 5.0));
    }

    #[test]
    fn even_spacing_keeps_a_partial_last_row_on_the_same_columns() {
        let (gap, inset) = even_spacing(768.0, 3, 204.0, 18.0);
        assert_eq!(gap, 48.0);
        assert_eq!(inset, 30.0);
    }
}
