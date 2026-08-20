//! Shared single-scroll layout for browse chips followed by books.

use std::rc::Rc;

use epub_svelte_lib::BrowseRow;
use gpui::prelude::*;
use gpui::{AnyElement, App, Entity, Render, SharedString, container_query, px, size};
use gpui_component::v_virtual_list;
use ui_components as components;

use super::chip_grid;
use crate::virtual_grid;

pub(crate) type OpenLocation = Rc<dyn Fn(String, &mut App)>;
pub(crate) fn render<B, P>(list_id: SharedString, chips: Rc<Vec<BrowseRow>>, books: Rc<Vec<(Entity<B>, usize)>>, page: Entity<P>, open_location: OpenLocation) -> AnyElement
where
    B: Render + 'static,
    P: Render + 'static,
{
    let query = container_query(move |available, window, _cx| {
        let width = f32::from(available.width);
        let rem_size = f32::from(window.rem_size());
        let chip_layout = chip_grid::layout(width, rem_size, chips.len());

        let book_columns = virtual_grid::columns_for_width(width, components::BOOK_COVER_WIDTH, components::BOOK_GRID_GAP);
        let book_gap = components::BOOK_GRID_GAP;
        let book_width = ((width - book_gap * book_columns.saturating_sub(1) as f32) / book_columns as f32).max(1.0);
        let book_row_height = components::book_grid_row_height_for_cover_width(book_width, rem_size);
        let book_rows = Rc::new(pack_rows(&books, book_columns));
        let book_row_count = book_rows.len();

        let mut row_sizes = Vec::with_capacity(chip_layout.row_count + book_row_count);
        row_sizes.extend((0..chip_layout.row_count).map(|_| size(px(width), px(chip_layout.row_height))));
        row_sizes.extend((0..book_row_count).map(|_| size(px(width), px(book_row_height))));

        let render_chips = chips.clone();
        let render_book_rows = book_rows.clone();
        let render_open_location = open_location.clone();
        let list = v_virtual_list(page.clone(), list_id.clone(), Rc::new(row_sizes), move |_, range, _, cx| {
            range
                .map(|row| {
                    if row < chip_layout.row_count {
                        let chip_range = chip_grid::row_range(row, chip_layout, render_chips.len());

                        chip_grid::row(chip_layout)
                            .children(render_chips[chip_range].iter().map(|row| {
                                let id = row.id.clone();
                                let open_location = render_open_location.clone();
                                chip_grid::chip(format!("browse-chip-{id}").into(), row.name.clone().into(), row.book_count, move |_, _, cx| open_location(id.clone(), cx), cx)
                            }))
                            .into_any_element()
                    } else {
                        let book_row = row - chip_layout.row_count;
                        virtual_grid::spanned_entity_grid_row(&render_book_rows[book_row], book_columns, width, book_row_height, book_gap).into_any_element()
                    }
                })
                .collect::<Vec<_>>()
        });

        components::full_size_column().child(components::sized_virtual_list(list).flex_1().min_h_0())
    });

    components::remaining_space_column().child(query).into_any_element()
}

fn pack_rows<B: Clone>(books: &[(B, usize)], columns: usize) -> Vec<Vec<(B, usize)>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut occupied = 0;

    for (book, span) in books {
        let span = (*span).clamp(1, columns);
        if occupied > 0 && occupied + span > columns {
            rows.push(std::mem::take(&mut row));
            occupied = 0;
        }
        occupied += span;
        row.push((book.clone(), span));
    }
    if !row.is_empty() {
        rows.push(row);
    }

    rows
}
