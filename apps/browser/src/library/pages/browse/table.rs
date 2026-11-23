//! The shared browse listing presented through gpui-component's virtual table.
use std::{
    collections::{HashMap, HashSet},
    ops::Range,
};

use gpui::prelude::*;
use gpui::{App, Context, Entity, EntityId, Focusable as _, IntoElement, MouseButton, Pixels, Render, SharedString, Subscription, WeakEntity, Window, div, px, rems};
use gpui_component::table::{Column, ColumnSort, DataTable, TableDelegate, TableEvent, TableState};
use gpui_component::{Icon, Sizable, Size};
use library_model::{BrowseBookSort, BrowseChipSort};
use sync_common::DirId;
use ui_components as components;
use ui_paginator::CursorMove;
use ui_text::text;

use super::{
    controls::BrowseOptionsEvent,
    listing::{BrowseItem, BrowseKind, chip_id, resolve_items},
    navigator::{BrowseNavigator, Loaded},
    page::BrowsePage,
};
use crate::library::services::{LibraryFileActionsChanged, LibraryUpdateBridge};
use crate::{library::widgets::BookCardView, services::format_storage_bytes};

const MIN_TITLE_TEXT_WIDTH_REM: f32 = 16.0;
const TABLE_SCROLLBAR_GUTTER: f32 = 16.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Select,
    Title,
    Author,
    Format,
    Size,
    Progress,
    Downloaded,
}

impl Field {
    fn width(self, available_width: f32, rem: f32) -> f32 {
        match self {
            Self::Select => rem * components::topbar_action_height_rem() + components::SPACE_MD,
            Self::Title => 0.0,
            Self::Author => (available_width * 0.16).clamp(rem * 8.0, rem * 12.0),
            Self::Format => rem * 4.5,
            Self::Size => rem * 6.0,
            Self::Progress => rem * 4.5,
            Self::Downloaded => rem * 6.5,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Select => "",
            Self::Title => text!(browse::book_title),
            Self::Author => text!(browse::author),
            Self::Format => text!(browse::format),
            Self::Size => text!(browse::file_size),
            Self::Progress => text!(browse::read),
            Self::Downloaded => text!(browse::downloaded),
        }
    }
    fn sorts(self) -> Option<(BrowseBookSort, BrowseBookSort)> {
        use BrowseBookSort::*;
        Some(match self {
            Self::Select => return None,
            Self::Title => (Alphabetical, TitleDescending),
            Self::Author => (Author, AuthorDescending),
            Self::Format => (Format, FormatDescending),
            Self::Size => (Size, SizeDescending),
            Self::Progress => (Progress, ProgressDescending),
            Self::Downloaded => (Availability, AvailabilityDescending),
        })
    }
}

pub(super) struct BookTableView {
    table: Entity<TableState<BookTable>>,
    navigator: Entity<BrowseNavigator>,
    width: Pixels,
    book_subscriptions: HashMap<EntityId, Subscription>,
    _subscriptions: Vec<Subscription>,
}

impl BookTableView {
    pub(super) fn new(navigator: Entity<BrowseNavigator>, page: WeakEntity<BrowsePage>, updates: Entity<LibraryUpdateBridge>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let width = window.viewport_size().width;
        let table = cx.new(|cx| {
            TableState::new(BookTable { navigator: navigator.clone(), page, fields: Vec::new(), columns: Vec::new(), selected: HashSet::new(), visible: 0..24, compact: false, narrow: false, size_inline: false, cursor: None }, window, cx)
                .col_selectable(false)
                .row_selectable(false)
                .col_resizable(false)
                .col_movable(false)
        });
        let subscriptions = vec![
            cx.observe(&table, |_, _, cx| cx.notify()),
            cx.subscribe(&updates, |view, _, _: &LibraryFileActionsChanged, cx| view.table.update(cx, |_, cx| cx.notify())),
            cx.subscribe(&table, |view, _, event: &TableEvent, cx| {
                if matches!(event, TableEvent::ClearSelection) {
                    view.clear_selection(cx);
                }
            }),
            cx.subscribe(&navigator, |view, _, loaded: &Loaded, cx| {
                view.table.update(cx, |table, cx| {
                    if loaded.left.is_some() || !loaded.refresh {
                        table.delegate_mut().selected.clear();
                        table.delegate_mut().cursor = None;
                        table.clear_selection(cx);
                        table.scroll_to_row(0, cx);
                    }
                    let listed = &table.delegate().navigator.read(cx).listing.items;
                    let selected = table.delegate().selected.iter().filter(|id| listed.contains_key(*id)).cloned().collect();
                    table.delegate_mut().selected = selected;
                    table.refresh(cx);
                    cx.notify();
                });
                cx.notify();
            }),
        ];
        Self { table, navigator, width, book_subscriptions: HashMap::new(), _subscriptions: subscriptions }
    }

    pub(super) fn selected_items(&self, cx: &App) -> (Option<DirId>, Vec<BrowseItem>) {
        let navigator = self.navigator.read(cx);
        let table = self.table.read(cx);
        let ids = table.delegate().loaded_items(cx).into_iter().filter(|item| table.delegate().selected.contains(&item.id(cx))).map(|item| item.id(cx)).collect::<Vec<_>>();
        (navigator.destination.location.folder(), resolve_items(&ids, &navigator.listing.items))
    }

    pub(super) fn clear_selection(&mut self, cx: &mut Context<Self>) -> bool {
        self.table.update(cx, |table, cx| {
            let changed = !table.delegate().selected.is_empty();
            table.delegate_mut().selected.clear();
            if changed {
                cx.notify();
            }
            changed
        })
    }

    pub(super) fn select_loaded(&mut self, cx: &mut Context<Self>) {
        self.table.update(cx, |table, cx| {
            table.delegate_mut().select_loaded(cx);
            cx.notify();
        });
    }

    pub(super) fn cursor_item(&self, cx: &App) -> Option<BrowseItem> {
        let table = self.table.read(cx);
        table.selected_row().and_then(|index| table.delegate().item(index, cx))
    }

    pub(super) fn navigation_focused(&self, window: &Window, cx: &App) -> bool {
        self.table.read(cx).focus_handle(cx).is_focused(window)
    }

    pub(super) fn toggle_selection(&mut self, movement: CursorMove, cx: &mut Context<Self>) {
        self.table.update(cx, |table, cx| {
            let count = table.delegate().rows_count(cx);
            if count == 0 {
                return;
            }
            let current = table.selected_row().unwrap_or(0).min(count - 1);
            if table.delegate().selected.is_empty()
                && let Some(item) = table.delegate().item(current, cx)
            {
                table.delegate_mut().selected.insert(item.id(cx));
            }
            let target = match movement {
                CursorMove::Left | CursorMove::Up => current.saturating_sub(1),
                CursorMove::Right | CursorMove::Down => (current + 1).min(count - 1),
            };
            if target != current
                && let Some(item) = table.delegate().item(target, cx)
            {
                let id = item.id(cx);
                if !table.delegate_mut().selected.remove(&id) {
                    table.delegate_mut().selected.insert(id);
                }
            }
            table.delegate_mut().cursor = Some(target);
            table.set_selected_row(target, cx);
            cx.notify();
        });
    }

    pub(super) fn handle_key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let table = self.table.read(cx);
        let count = table.delegate().rows_count(cx);
        if count == 0 {
            return false;
        }
        let current = table.selected_row();
        if key == "enter" {
            if let Some(item) = current.and_then(|index| table.delegate().item(index, cx)) {
                let page = table.delegate().page.clone();
                activate(item, page, window, cx);
                return true;
            }
            return false;
        }
        let index = match key {
            "up" => current.unwrap_or(0).saturating_sub(1),
            "down" => current.map_or(0, |index| (index + 1).min(count - 1)),
            "home" => 0,
            "end" => count - 1,
            "pageup" => current.unwrap_or(0).saturating_sub(8),
            "pagedown" => (current.unwrap_or(0) + 8).min(count - 1),
            _ => return false,
        };
        self.table.update(cx, |table, cx| {
            table.delegate_mut().cursor = Some(index);
            table.set_selected_row(index, cx);
        });
        true
    }
}

impl Render for BookTableView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rem = f32::from(window.rem_size());
        let width = self.width;
        self.table.update(cx, |table, cx| {
            if table.delegate_mut().fit(width, rem, cx) {
                table.refresh(cx);
            }
        });
        let table = self.table.read(cx);
        let visible = table.delegate().visible.clone();
        let compact = table.delegate().compact;
        let size_inline = table.delegate().size_inline;
        let books = visible.clone().filter_map(|index| table.delegate().item(index, cx)).filter_map(|item| item.book().cloned()).collect::<Vec<_>>();
        self.book_subscriptions.retain(|id, _| books.iter().any(|book| book.entity_id() == *id));
        for book in books {
            self.book_subscriptions.entry(book.entity_id()).or_insert_with(|| cx.observe(&book, |view, _, cx| view.table.update(cx, |_, cx| cx.notify())));
        }
        let navigator = self.navigator.clone();
        cx.defer(move |cx| {
            let compact_slots = navigator.read(cx).listing.compact_slot_count();
            navigator.update(cx, |navigator, cx| navigator.load_page_range(visible, compact_slots, cx));
        });
        let height = window.rem_size() * (components::BOOK_CARD_TITLE_HEIGHT_REM + components::TEXT_XS * 1.25 * (u8::from(compact) + u8::from(size_inline)) as f32) + px(16.0);
        let this = cx.weak_entity();
        div()
            .relative()
            .size_full()
            .min_h_0()
            .px(px(components::CONTENT_INSET))
            .child(
                gpui::canvas(
                    move |bounds, _, cx| {
                        let _ = this.update(cx, |view, cx| {
                            if (view.width - bounds.size.width).abs() > px(0.5) {
                                view.width = bounds.size.width;
                                cx.notify();
                            }
                        });
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .left(px(components::CONTENT_INSET))
                .right(px(components::CONTENT_INSET))
                .top_0()
                .h_full(),
            )
            .child(DataTable::new(&self.table).bordered(false).stripe(false).scrollbar_visible(true, false).small().row_height(height).header_height(window.rem_size() * components::topbar_action_height_rem()))
    }
}

pub(super) struct BookTable {
    navigator: Entity<BrowseNavigator>,
    page: WeakEntity<BrowsePage>,
    fields: Vec<Field>,
    columns: Vec<Column>,
    selected: HashSet<SharedString>,
    visible: Range<usize>,
    compact: bool,
    narrow: bool,
    size_inline: bool,
    cursor: Option<usize>,
}

impl BookTable {
    fn child_count(&self, cx: &App) -> usize {
        let listing = &self.navigator.read(cx).listing;
        listing.page.as_ref().filter(|page| page.headings.is_some()).map_or(listing.children.len(), |page| page.heading_count() + listing.compact_slot_count())
    }

    fn item(&self, index: usize, cx: &App) -> Option<BrowseItem> {
        let listing = &self.navigator.read(cx).listing;
        let children = self.child_count(cx);
        if index < children {
            let row = if listing.page.as_ref().is_some_and(|page| page.headings.is_some()) {
                let compact = listing.compact_slot_count();
                if index < compact { listing.children.iter().filter(|row| !super::listing::is_author_heading(row.id)).nth(index) } else { listing.heading_rows.get(&(index - compact)) }
            } else {
                listing.children.get(index)
            }?;
            listing.item(&chip_id(row.id)).cloned()
        } else {
            let book = if listing.page.is_some() { listing.book_indices.get(&(index - children)) } else { listing.books.get(index - children) }?;
            Some(BrowseItem::Book(book.clone()))
        }
    }

    fn loaded_items(&self, cx: &App) -> Vec<BrowseItem> {
        let listing = &self.navigator.read(cx).listing;
        listing.children.iter().filter_map(|row| listing.item(&chip_id(row.id)).cloned()).chain(listing.books.iter().cloned().map(BrowseItem::Book)).collect()
    }

    fn select_loaded(&mut self, cx: &App) {
        self.selected.extend(self.loaded_items(cx).into_iter().map(|item| item.id(cx)));
    }

    fn fit(&mut self, width: Pixels, rem: f32, cx: &App) -> bool {
        let width = (f32::from(width) - TABLE_SCROLLBAR_GUTTER).max(0.0);
        let padding = Size::Small.table_cell_padding();
        let min_title_width = rem * MIN_TITLE_TEXT_WIDTH_REM + f32::from(padding.left + padding.right);
        let catalog = self.navigator.read(cx).kind == BrowseKind::Catalog;
        let mut fields = vec![Field::Select, Field::Title, Field::Author, Field::Format, Field::Size];
        if !catalog {
            fields.extend([Field::Progress, Field::Downloaded]);
        }
        // Use the same widths for fitting and rendering, including only the
        // columns this page offers. Keep a readable title before collapsing.
        let required_width = |fields: &[Field]| min_title_width + fields.iter().map(|field| field.width(width, rem)).sum::<f32>();
        self.compact = required_width(&fields) > width;
        if self.compact {
            fields.retain(|field| !matches!(field, Field::Author | Field::Progress));
        }
        self.narrow = required_width(&fields) > width;
        if self.narrow {
            fields.retain(|field| *field != Field::Format);
        }
        self.size_inline = required_width(&fields) > width;
        if self.size_inline {
            fields.retain(|field| *field != Field::Size);
        }
        let widths = fields.iter().map(|field| field.width(width, rem)).collect::<Vec<_>>();
        let title_width = (width - widths.iter().sum::<f32>()).max(1.0);
        let selected_sort = self.navigator.read(cx).selection.book_sort;
        let selected_chip_sort = self.navigator.read(cx).selection.chip_sort;
        let has_headings = self.child_count(cx) > 0;
        let chip_sorts = &self.navigator.read(cx).listing.controls.collection_sorts;
        let active_sort = |sort| selected_sort == sort && (!has_headings || BrowseChipSort::for_book_sort(sort).filter(|sort| chip_sorts.contains(sort)).is_none_or(|sort| sort == selected_chip_sort));
        let offered = &self.navigator.read(cx).listing.controls.book_sorts;
        let columns = fields
            .iter()
            .zip(widths)
            .map(|(&field, width)| {
                let mut column = Column::new(format!("book-{}", field as usize), field.label()).width(px(if field == Field::Title { title_width } else { width })).resizable(false).movable(false).selectable(false);
                if field == Field::Size {
                    column = column.text_right();
                }
                if let Some((ascending, descending)) = field.sorts().filter(|(a, _)| offered.contains(a)) {
                    column = column.sort(if active_sort(ascending) {
                        ColumnSort::Ascending
                    } else if active_sort(descending) {
                        ColumnSort::Descending
                    } else {
                        ColumnSort::Default
                    });
                }
                column
            })
            .collect::<Vec<_>>();
        let changed = self.fields != fields || self.columns.iter().zip(&columns).any(|(a, b)| (a.width - b.width).abs() > px(0.5) || a.sort != b.sort);
        self.fields = fields;
        self.columns = columns;
        changed
    }
}

impl TableDelegate for BookTable {
    fn columns_count(&self, _: &App) -> usize {
        self.columns.len()
    }
    fn rows_count(&self, cx: &App) -> usize {
        let listing = &self.navigator.read(cx).listing;
        self.child_count(cx) + listing.page.as_ref().map_or(listing.books.len(), |page| page.book_total)
    }
    fn column(&self, index: usize, _: &App) -> Column {
        self.columns[index].clone()
    }

    fn perform_sort(&mut self, index: usize, direction: ColumnSort, _: &mut Window, cx: &mut Context<TableState<Self>>) {
        let Some((ascending, descending)) = self.fields[index].sorts() else { return };
        let sort = if direction == ColumnSort::Descending { descending } else { ascending };
        let page = self.page.clone();
        // The page can refresh this table when the new result arrives.
        cx.defer(move |cx| {
            let _ = page.update(cx, |page, cx| page.select_option(BrowseOptionsEvent::SelectTableSort(sort), cx));
        });
    }

    fn visible_rows_changed(&mut self, range: Range<usize>, _: &mut Window, cx: &mut Context<TableState<Self>>) {
        self.visible = range.clone();
        let navigator = self.navigator.clone();
        cx.defer(move |cx| {
            let compact = navigator.read(cx).listing.compact_slot_count();
            navigator.update(cx, |navigator, cx| navigator.load_page_range(range, compact, cx));
        });
    }

    fn render_empty(&mut self, _: &mut Window, _: &mut Context<TableState<Self>>) -> impl IntoElement {
        // The browse page owns its loading, error, and empty states.
        div()
    }

    fn render_th(&mut self, index: usize, _: &mut Window, cx: &mut Context<TableState<Self>>) -> impl IntoElement {
        if self.fields[index] == Field::Select {
            let loaded = self.loaded_items(cx);
            let checked = !loaded.is_empty() && loaded.iter().all(|item| self.selected.contains(&item.id(cx)));
            components::selection_checkbox("table-select-loaded", text!(browse::select_loaded_items), checked)
                .indeterminate(!checked && loaded.iter().any(|item| self.selected.contains(&item.id(cx))))
                .on_click(cx.listener(|table, checked, _, cx| {
                    cx.stop_propagation();
                    if *checked {
                        table.delegate_mut().select_loaded(cx);
                    } else {
                        table.delegate_mut().selected.clear();
                    }
                    cx.notify();
                }))
                .into_any_element()
        } else {
            div().text_size(rems(components::TEXT_XS)).child(self.fields[index].label()).into_any_element()
        }
    }

    fn render_tr(&mut self, index: usize, _: &mut Window, cx: &mut Context<TableState<Self>>) -> gpui::Stateful<gpui::Div> {
        let theme = components::browser_theme(cx);
        let item = self.item(index, cx);
        let selected = item.as_ref().is_some_and(|item| self.selected.contains(&item.id(cx)));
        let page = self.page.clone();
        let menu_item = item.clone();
        let hold_page = page.clone();
        let hold_item = item.clone();
        let open_page = page.clone();
        div()
            .id(("book-table-row", index))
            .relative()
            .when(selected, |row| row.bg(theme.accent_wash))
            .when(self.cursor == Some(index), |row| row.child(div().absolute().inset_0().border_1().border_color(theme.accent)))
            .on_mouse_down(MouseButton::Right, move |event, window, cx| {
                if let Some(item) = &menu_item {
                    let _ = page.update(cx, |page, cx| page.open_table_menu(item.clone(), event.position, window, cx));
                    cx.stop_propagation();
                }
            })
            .on_long_press(move |event, window, cx| {
                if let Some(item) = &hold_item {
                    let _ = hold_page.update(cx, |page, cx| page.open_table_menu(item.clone(), event.position, window, cx));
                    cx.stop_propagation();
                }
            })
            .on_click(move |_, window, cx| {
                if let Some(item) = &item {
                    activate(item.clone(), open_page.clone(), window, cx);
                    cx.stop_propagation();
                }
            })
    }

    fn render_td(&mut self, row: usize, column: usize, window: &mut Window, cx: &mut Context<TableState<Self>>) -> impl IntoElement {
        let Some(item) = self.item(row, cx) else { return div().into_any_element() };
        let field = self.fields[column];
        let theme = components::browser_theme(cx);
        let label = if matches!(field, Field::Select | Field::Title) {
            match &item {
                BrowseItem::Book(card) => card.read(cx).title().to_owned(),
                BrowseItem::Child { row, .. } => row.name.clone(),
            }
        } else {
            String::new()
        };
        if field == Field::Select {
            let id = item.id(cx);
            return components::selection_checkbox(("table-select", row), format!(text!(browse::select_item), name = label), self.selected.contains(&id))
                .on_click(cx.listener(move |table, checked, _, cx| {
                    cx.stop_propagation();
                    if *checked {
                        table.delegate_mut().selected.insert(id.clone());
                    } else {
                        table.delegate_mut().selected.remove(&id);
                    }
                    cx.notify();
                }))
                .into_any_element();
        }
        if field == Field::Downloaded {
            return match &item {
                BrowseItem::Book(card) => BookCardView::download_checkbox(card.clone(), cx),
                BrowseItem::Child { row, .. } => BrowsePage::folder_download_checkbox(self.page.clone(), row, cx),
            };
        }
        if field == Field::Title {
            let padding = Size::Small.table_cell_padding();
            let width = (self.columns[column].width - padding.left - padding.right).max(px(1.0));
            let content = match &item {
                BrowseItem::Book(card) => {
                    let book = card.read(cx).detail_source().0;
                    components::ensure_text_fonts(&book.title, cx);
                    components::ensure_text_fonts(&book.author, cx);
                    if let Some(subtitle) = &book.subtitle {
                        components::ensure_text_fonts(subtitle, cx);
                    }
                    div()
                        .w_full()
                        .min_w_0()
                        .whitespace_normal()
                        .child(components::book_card_intrinsic_title(book.title, book.subtitle.as_deref(), width, window, cx))
                        .when(self.compact, |row| {
                            row.child(div().text_size(rems(components::TEXT_XS)).line_height(gpui::relative(1.25)).line_clamp(1).text_ellipsis().text_color(theme.text_muted).child(if self.narrow {
                                let format = book.format.map(|format| format.canonical_extension().to_uppercase()).unwrap_or_else(|| book.format_category.label().into());
                                if book.author.trim().is_empty() { format } else { format!("{} · {format}", book.author) }
                            } else {
                                book.author
                            }))
                        })
                        .when(self.size_inline, |row| {
                            row.child(div().text_size(rems(components::TEXT_XS)).line_height(gpui::relative(1.25)).text_color(theme.text_muted).child(book.size_bytes.map(format_storage_bytes).unwrap_or_else(|| "—".into())))
                        })
                }
                BrowseItem::Child { row, .. } => {
                    let subtitle = if self.size_inline && row.id.folder().is_some() {
                        format!(text!(browse::book_count_with_size), row.size_bytes.map(format_storage_bytes).unwrap_or_else(|| "—".into()), count = row.book_count)
                    } else {
                        format!(text!(browse::book_count), row.book_count)
                    };
                    div()
                        .w_full()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap(px(components::SPACE_SM))
                        .child(Icon::new(components::navigation_icon(self.navigator.read(cx).kind.child_icon())).size(window.rem_size()))
                        .whitespace_normal()
                        .child(components::book_card_intrinsic_title(row.name.clone(), Some(&subtitle), (width - window.rem_size() - px(components::SPACE_SM)).max(px(1.0)), window, cx))
                }
            };
            let page = self.page.clone();
            return components::base_button(("table-title", row))
                .ghost()
                .no_fill()
                .p_0()
                .w_full()
                .min_w_0()
                .h_auto()
                .aria_label(label)
                .child(content)
                .on_click(move |_, window, cx| {
                    activate(item.clone(), page.clone(), window, cx);
                    cx.stop_propagation();
                })
                .into_any_element();
        }
        let value = match &item {
            BrowseItem::Book(card) => {
                let book = card.read(cx).detail_source().0;
                match field {
                    Field::Author => book.author,
                    Field::Format => book.format.map(|format| format.canonical_extension().to_uppercase()).unwrap_or_else(|| book.format_category.label().into()).into(),
                    Field::Size => book.size_bytes.map(format_storage_bytes).unwrap_or_else(|| "—".into()),
                    Field::Progress => {
                        if book.progress >= library_model::FINISHED_PROGRESS_THRESHOLD {
                            "100%".into()
                        } else {
                            format!("{}%", book.progress.round() as u32)
                        }
                    }
                    _ => String::new(),
                }
            }
            BrowseItem::Child { row, .. } => match field {
                Field::Size if row.id.folder().is_some() => row.size_bytes.map(format_storage_bytes).unwrap_or_else(|| "—".into()),
                _ => String::new(),
            },
        };
        div()
            .w_full()
            .min_w_0()
            .text_size(rems(components::TEXT_SM))
            .text_color(theme.text_muted)
            .whitespace_normal()
            .when(field == Field::Size, |cell| cell.text_right().whitespace_nowrap())
            .line_clamp(2)
            .text_ellipsis()
            .child(value)
            .into_any_element()
    }
}

fn activate(item: BrowseItem, page: WeakEntity<BrowsePage>, window: &mut Window, cx: &mut App) {
    window.defer(cx, move |window, cx| match item {
        BrowseItem::Book(card) => BookCardView::activate(&card, cx),
        BrowseItem::Child { row, .. } => {
            let _ = page.update(cx, |page, cx| page.open_table_child(row.id, window, cx));
        }
    });
}
