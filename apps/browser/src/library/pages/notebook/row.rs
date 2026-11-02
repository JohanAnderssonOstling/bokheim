use super::super::pagination::{merge_page, page_request_for_slots};
use super::overlay;
use crate::library::services::LibraryContext;
use crate::library::widgets::{
    BookCardElement, BookCardEvent, BookCardInteraction, BookCardPresentation, BookCardView, Paginator, PaginatorAxis, PaginatorChild, PaginatorChildren, PaginatorGroup, PaginatorGroupPolicy, PaginatorSizing, PaginatorWidthPolicy,
};
use book_model::Annotation;
use gpui::prelude::*;
use gpui::{App, Context, Entity, EventEmitter, FontWeight, HighlightStyle, Modifiers, Pixels, Render, ScrollHandle, StyledText, Subscription, Task, Window, div, px, rems};
use gpui_component::IconName;
use library_model::{BookCardRow, BookTextMatch, BookTextSearchBook, NOTEBOOK_ANNOTATION_PAGE_SIZE, NotebookAnnotationPage, NotebookBook, NotebookQuery};
use std::{collections::BTreeMap, rc::Rc};
use ui_components as components;

const RESULT_CARD_WIDTH_REM: f32 = 18.0;

pub(super) enum RowData {
    Note(NotebookBook),
    Text(BookTextSearchBook),
}
impl RowData {
    pub(super) fn book(&self) -> &BookCardRow {
        match self {
            Self::Note(value) => &value.book,
            Self::Text(value) => &value.book,
        }
    }
}

struct TextMatches {
    total: usize,
    previews: BTreeMap<usize, Rc<BookTextMatch>>,
    stale: bool,
}

pub(super) struct Stale;

impl EventEmitter<Stale> for NotebookRow {}

pub(super) struct NotebookRow {
    text_matches: Option<TextMatches>,
    scroll: ScrollHandle,
    book_data: BookCardRow,
    has_more: bool,
    library: LibraryContext,
    query: NotebookQuery,
    offset: usize,
    revision: String,
    request: Option<Task<()>>,
    page_position: PagePosition,
    book: Entity<BookCardView>,
    book_element: Entity<BookCardElement>,
    flow: Entity<Paginator>,
    annotations: Vec<(Annotation, f32)>,
    layout: Option<(Pixels, Pixels, Pixels)>,
    _flow_updates: Subscription,
    _book_updates: Subscription,
    _cover_text_updates: Subscription,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum PagePosition {
    #[default]
    Keep,
    First,
    Last,
}

fn estimated_annotations(values: Vec<Annotation>) -> Vec<(Annotation, f32)> {
    values
        .into_iter()
        .map(|annotation| {
            let length = annotation.exact_text.chars().count() + annotation.note.chars().count();
            (annotation, (10.0 + length as f32 / 24.0).clamp(12.0, 20.0))
        })
        .collect()
}

impl NotebookRow {
    pub(super) fn from_data(data: RowData, query: NotebookQuery, revision: &str, library: LibraryContext, cx: &mut Context<Self>) -> Self {
        match data {
            RowData::Note(value) => Self::new(value, query, library, cx),
            RowData::Text(value) => {
                let mut row = Self::new(NotebookBook { book: value.book, page: Default::default() }, query, library, cx);
                row.revision = revision.to_owned();
                row.text_matches = Some(TextMatches { total: value.match_count, previews: BTreeMap::new(), stale: false });
                row
            }
        }
    }
    pub(super) fn replace_data(&mut self, data: RowData, query: NotebookQuery, revision: &str, cx: &mut Context<Self>) {
        match data {
            RowData::Note(value) => {
                self.text_matches = None;
                self.replace(value, query, cx);
            }
            RowData::Text(value) => {
                let first = self.query != query || self.revision != revision;
                self.replace(NotebookBook { book: value.book, page: NotebookAnnotationPage { revision: revision.to_owned(), ..Default::default() } }, query, cx);
                self.text_matches = Some(TextMatches { total: value.match_count, previews: BTreeMap::new(), stale: false });
                self.layout = None;
                if first {
                    self.page_position = PagePosition::First;
                }
            }
        }
    }

    pub(super) fn new(data: NotebookBook, query: NotebookQuery, library: LibraryContext, cx: &mut Context<Self>) -> Self {
        let open = library.clone();
        let book = BookCardView::create_in_grid(
            data.book.clone(),
            library.clone(),
            Rc::new(move |event, window, cx| {
                if let BookCardEvent::OpenDetail(book, _) = event {
                    if let Some(id) = book.library_file() {
                        open.open_book_at(id, book.title, None, window, cx);
                    }
                }
            }),
            false,
            false,
            cx,
        );
        let book_element = BookCardView::presentation(book.clone(), BookCardPresentation::Grid, cx);
        let flow = cx.new(|_| Paginator::new("notebook-annotations", []).flow_axis(PaginatorAxis::Horizontal).horizontal_only().next_page_preview(false));
        let flow_updates = cx.observe(&flow, |row: &mut Self, _, cx| {
            row.load_visible_matches(cx);
            cx.notify();
        });
        let book_updates = cx.observe(&book, |_, _, cx| cx.notify());
        let cover_text_updates = cx.observe_global::<crate::stores::CoverTextSetting>(|_: &mut Self, cx| cx.notify());
        let annotations = estimated_annotations(data.page.annotations);
        Self {
            text_matches: None,
            scroll: ScrollHandle::new(),
            book_data: data.book,
            has_more: data.page.has_more,
            library,
            query,
            offset: data.page.offset,
            revision: data.page.revision,
            request: None,
            page_position: PagePosition::Keep,
            book,
            book_element,
            flow,
            annotations,
            layout: None,
            _flow_updates: flow_updates,
            _book_updates: book_updates,
            _cover_text_updates: cover_text_updates,
        }
    }

    fn book_geometry(&self, window: &Window, cx: &App) -> (Pixels, Pixels) {
        let compact = components::WindowWidthClass::for_window(window).is_compact();
        let width = rems(if compact { 6.75 } else { 8.75 }).to_pixels(window.rem_size());
        (width, self.book.read(cx).height_at_width(width, window, cx))
    }

    pub(super) fn height(&self, window: &Window, cx: &App) -> Pixels {
        self.book_geometry(window, cx).1 + rems(components::topbar_action_height_rem() + 1.5).to_pixels(window.rem_size()) + px(1.0)
    }

    pub(super) fn book_card(&self) -> &Entity<BookCardView> {
        &self.book
    }

    pub(super) fn annotation_window(&self) -> Option<(sync_common::BookId, usize)> {
        if self.text_matches.is_some() { None } else { self.book_data.library_file().map(|book| (book, self.offset)) }
    }

    pub(super) fn replace(&mut self, data: NotebookBook, query: NotebookQuery, cx: &mut Context<Self>) {
        self.book.update(cx, |book, cx| book.replace_row(data.book.clone(), cx));
        self.request = None;
        let position = if self.query != query { PagePosition::First } else { PagePosition::Keep };
        self.query = query;
        self.book_data = data.book;
        self.install_page(data.page, position, cx);
    }

    fn install_page(&mut self, page: NotebookAnnotationPage, position: PagePosition, cx: &mut Context<Self>) {
        if position != PagePosition::Keep {
            self.page_position = position;
        }
        let changed = !self.annotations.iter().map(|(annotation, _)| annotation).eq(page.annotations.iter());
        if changed {
            self.annotations = estimated_annotations(page.annotations);
        }
        if changed || self.page_position != PagePosition::Keep {
            self.layout = None;
        }
        self.offset = page.offset;
        self.revision = page.revision;
        self.has_more = page.has_more;
        cx.notify();
    }

    fn load_annotations(&mut self, offset: usize, position: PagePosition, cx: &mut Context<Self>) {
        let Some(book) = self.book_data.library_file() else { return };
        let backend = self.library.backend().clone();
        let query = self.query.clone();
        let revision = self.revision.clone();
        self.request = Some(cx.spawn(async move |row, cx| {
            let result = backend.notebook_annotations(query, revision, book, offset).await;
            let _ = row.update(cx, |row, cx| {
                row.request = None;
                match result {
                    Ok(Some(page)) => row.install_page(page, position, cx),
                    Ok(None) => cx.emit(Stale),
                    Err(error) => {
                        log::error!("failed to load notebook annotations: {error}");
                        cx.notify();
                    }
                }
            });
        }));
        cx.notify();
    }

    fn load_visible_matches(&mut self, cx: &mut Context<Self>) {
        let Some(matches) = &self.text_matches else { return };
        if self.request.is_some() || matches.stale {
            return;
        }
        let Some(range) = self.flow.read(cx).page_child_range() else { return };
        let Some(request) = page_request_for_slots(range.clone(), 0, 0, matches.total, |_| true, |index| matches.previews.contains_key(&index)) else { return };
        let offset = request.book_offset;
        let end = (offset..range.end).find(|index| matches.previews.contains_key(index)).unwrap_or(range.end);
        let Some(book) = self.book_data.library_file() else { return };
        let backend = self.library.backend().clone();
        let query = self.query.clone();
        let revision = self.revision.clone();
        self.request = Some(cx.spawn(async move |row, cx| {
            let result = backend.text_search_matches(query, revision, book, offset, end - offset).await;
            let _ = row.update(cx, |row, cx| {
                row.request = None;
                match result {
                    Ok(Some(page)) => {
                        if let Some(matches) = &mut row.text_matches {
                            matches.total = page.total;
                            merge_page(&mut matches.previews, page.offset, page.matches, Rc::new);
                            row.layout = None;
                        }
                    }
                    Ok(None) => {
                        if let Some(matches) = &mut row.text_matches {
                            matches.stale = true;
                        }
                        row.layout = None;
                        cx.emit(Stale);
                    }
                    Err(error) => {
                        if let Some(matches) = &mut row.text_matches {
                            merge_page(&mut matches.previews, offset, (offset..end).map(|_| BookTextMatch::default()), Rc::new);
                        }
                        row.layout = None;
                        log::error!("failed to load book text matches: {error}");
                    }
                }
                if let (Some(matches), Some(range)) = (&mut row.text_matches, row.flow.read(cx).page_child_range()) {
                    // Retain the visible page and two pages on either side.
                    // Building descriptors stays bounded as the reader pages.
                    let margin = range.len().saturating_mul(2);
                    let keep = range.start.saturating_sub(margin)..range.end.saturating_add(margin);
                    matches.previews.retain(|index, _| keep.contains(index));
                }
                cx.notify();
                row.load_visible_matches(cx);
            });
        }));
    }

    fn turn(&mut self, next: bool, cx: &mut Context<Self>) {
        if self.request.is_some() || !self.flow.read(cx).page_turn_ready() {
            return;
        }
        let local_page = if next { self.flow.read(cx).has_next() } else { self.flow.read(cx).has_previous() };
        if local_page {
            self.flow.update(cx, |flow, cx| {
                if next {
                    flow.next_with_axis(PaginatorAxis::Horizontal, cx);
                } else {
                    flow.previous_with_axis(PaginatorAxis::Horizontal, cx);
                }
            });
        } else if self.text_matches.is_some() {
            return;
        } else if next && self.has_more {
            self.load_annotations(self.offset + self.annotations.len(), PagePosition::First, cx);
        } else if !next && self.offset > 0 {
            self.load_annotations(self.offset.saturating_sub(NOTEBOOK_ANNOTATION_PAGE_SIZE), PagePosition::Last, cx);
        }
    }

    fn activate(&self, annotation: Annotation, window: &mut Window, cx: &mut Context<Self>) {
        overlay::show(self.library.clone(), self.book_data.title.clone(), annotation, window, cx);
    }

    fn text_groups(&self, matches: &TextMatches, height: Pixels, window: &Window, cx: &Context<Self>) -> Vec<PaginatorGroup> {
        let target = cx.weak_entity();
        let loaded = matches
            .previews
            .iter()
            .map(|(&index, hit)| {
                let target = target.clone();
                let hit = hit.clone();
                let child = PaginatorChild::new(move |_, _, cx| {
                    let theme = components::browser_theme(cx);
                    let click = target.clone();
                    let key = target.clone();
                    let click_target = hit.target.clone();
                    let key_target = hit.target.clone();
                    let excerpt = StyledText::new(if hit.excerpt.is_empty() { "Preview unavailable.".to_owned() } else { hit.excerpt.clone() })
                        .with_highlights(hit.excerpt_matches.iter().cloned().map(|range| (range, HighlightStyle { font_weight: Some(FontWeight::BOLD), ..Default::default() })));
                    components::reader_state_card(("book-text-match", index), false, theme)
                        .border_b_0()
                        .pt(px(4.0))
                        .h_full()
                        .overflow_hidden()
                        .when(hit.target.is_some(), |card| card.cursor_pointer())
                        .focusable()
                        .tab_stop(true)
                        .on_click(move |_, window, cx| {
                            cx.stop_propagation();
                            let _ = click.update(cx, |row, cx| row.open_text_match(click_target.clone(), window, cx));
                        })
                        .on_key_down(move |event, window, cx| {
                            if event.keystroke.modifiers == Modifiers::default() && matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                cx.stop_propagation();
                                let _ = key.update(cx, |row, cx| row.open_text_match(key_target.clone(), window, cx));
                            }
                        })
                        .child(components::reader_state_note(excerpt, theme).text_color(theme.text).overflow_hidden())
                        .into_any_element()
                })
                .with_id(format!("book-text-match-{index}"))
                .with_own_cell();
                (index, child)
            })
            .collect();
        let stale = matches.stale;
        let placeholder = PaginatorChild::new(move |_, _, cx| {
            let theme = components::browser_theme(cx);
            div().size_full().p(rems(0.75)).bg(theme.page_bg).child(components::reader_state_note(if stale { "Search again to refresh these results." } else { "Loading…" }, theme)).into_any_element()
        })
        .with_own_cell();
        let policy = PaginatorGroupPolicy::new(PaginatorWidthPolicy::new(rems(RESULT_CARD_WIDTH_REM)), PaginatorSizing::pixels(height), px(0.0))
            .with_maximum_width(PaginatorWidthPolicy::new(rems(RESULT_CARD_WIDTH_REM)))
            .with_gap(rems(0.5).to_pixels(window.rem_size()))
            .flush_edges()
            .pack_start();
        vec![PaginatorGroup::from_slots(policy, PaginatorChildren::sparse(matches.total, placeholder, loaded))]
    }

    fn open_text_match(&self, target: Option<String>, window: &mut Window, cx: &mut App) {
        if let (Some(book), Some(target)) = (self.book_data.library_file(), target) {
            self.library.open_book_at(book, self.book_data.title.clone(), Some(target), window, cx);
        }
    }

    fn groups(&self, height: Pixels, window: &Window, cx: &Context<Self>) -> Vec<PaginatorGroup> {
        if let Some(matches) = &self.text_matches {
            return self.text_groups(matches, height, window, cx);
        }
        let target = cx.weak_entity();
        let children = self.annotations.iter().map(|(annotation, width)| {
            let annotation = annotation.clone();
            let id = annotation.id.clone();
            let target = target.clone();
            PaginatorChild::new(move |_, _, cx| {
                let theme = components::browser_theme(cx);
                let click = target.clone();
                let key = target.clone();
                let click_annotation = annotation.clone();
                let key_annotation = annotation.clone();
                components::reader_state_card(format!("notebook-annotation-{}", annotation.id), false, theme)
                    .border_b_0()
                    .pt(px(4.0))
                    .debug_selector({
                        let id = annotation.id.clone();
                        move || format!("notebook-annotation-{id}")
                    })
                    .h_full()
                    .overflow_hidden()
                    .cursor_pointer()
                    .focusable()
                    .tab_stop(true)
                    .on_click(move |_, window, cx| {
                        cx.stop_propagation();
                        let _ = click.update(cx, |row, cx| row.activate(click_annotation.clone(), window, cx));
                    })
                    .on_key_down(move |event, window, cx| {
                        if event.keystroke.modifiers == Modifiers::default() && matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            cx.stop_propagation();
                            let _ = key.update(cx, |row, cx| {
                                row.activate(key_annotation.clone(), window, cx);
                            });
                        }
                    })
                    .child(components::reader_annotation_color_bar(annotation.color).top(px(6.0)))
                    .when(!annotation.exact_text.is_empty(), |card| card.child(components::reader_state_quote(format!("“{}”", annotation.exact_text), theme)))
                    .when(!annotation.note.is_empty(), |card| card.child(components::reader_state_note(annotation.note.clone(), theme).text_color(theme.text).min_h_0().overflow_hidden()))
                    .into_any_element()
            })
            .with_id(id)
            .with_estimated_width(rems(*width))
            .with_own_cell()
        });
        let policy = PaginatorGroupPolicy::new(PaginatorWidthPolicy::new(rems(RESULT_CARD_WIDTH_REM)), PaginatorSizing::pixels(height), px(0.0))
            .intrinsic()
            .with_maximum_width(PaginatorWidthPolicy::new(rems(RESULT_CARD_WIDTH_REM)))
            .with_gap(rems(0.5).to_pixels(window.rem_size()))
            .flush_edges()
            .pack_start();
        vec![PaginatorGroup::new(policy, children)]
    }
}

impl Render for NotebookRow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let (width, height) = self.book_geometry(window, cx);
        let layout = (width, height, window.rem_size());
        if self.layout != Some(layout) {
            let groups = self.groups(height, window, cx);
            let position = std::mem::take(&mut self.page_position);
            self.flow.update(cx, |flow, cx| match position {
                PagePosition::Keep => flow.refresh_groups(groups, cx),
                PagePosition::First => flow.set_groups(groups, cx),
                PagePosition::Last => {
                    flow.set_groups(groups, cx);
                    flow.show_last_page(cx);
                }
            });
            self.layout = Some(layout);
        }
        let flow = self.flow.read(cx);
        let can_previous = self.request.is_none() && (flow.has_previous() || self.offset > 0);
        let can_next = self.request.is_none() && (flow.has_next() || self.has_more);
        let arrows = div()
            .w_full()
            .flex()
            .justify_end()
            .gap(rems(0.25))
            .child(components::outlined_icon_button("notebook-previous", if self.text_matches.is_some() { "Previous matches" } else { "Previous annotations" }, IconName::ChevronLeft).opacity(if can_previous { 1.0 } else { 0.35 }).on_click(
                cx.listener(move |row, _, _, cx| {
                    if can_previous {
                        row.turn(false, cx);
                    }
                }),
            ))
            .child(components::outlined_icon_button("notebook-next", if self.text_matches.is_some() { "Next matches" } else { "Next annotations" }, IconName::ChevronRight).opacity(if can_next { 1.0 } else { 0.35 }).on_click(cx.listener(
                move |row, _, _, cx| {
                    if can_next {
                        row.turn(true, cx);
                    }
                },
            )));
        div()
            .id(("notebook-row", self.book.entity_id()))
            .debug_selector(|| "notebook-row-viewport".into())
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .on_scroll_wheel(cx.listener(|row, event: &gpui::ScrollWheelEvent, window, cx| {
                let delta = event.delta.pixel_delta_for_viewport(window.line_height(), row.scroll.bounds().size);
                // An oversized row owns vertical scrolling while the pointer
                // is over it; the outer paginator still handles normal rows.
                if row.scroll.max_offset().y > px(0.0) && delta.y.abs() > delta.x.abs() {
                    cx.stop_propagation();
                }
            }))
            .child(
                div().w_full().h(self.height(window, cx)).flex_none().min_w_0().pt(rems(0.25)).pb(rems(0.75)).border_b_1().border_color(theme.rule).flex().flex_col().gap(rems(0.5)).child(arrows).child(
                    div()
                        .w_full()
                        .h(height)
                        .flex_none()
                        .flex()
                        .gap(rems(0.5))
                        .child(div().w(width).h_full().flex_none().debug_selector(|| "notebook-book-card".into()).child(BookCardView::present(self.book_element.clone(), BookCardInteraction::default(), window, cx)))
                        .child(div().flex_1().min_w_0().h_full().child(self.flow.clone())),
                ),
            )
            .into_any_element()
    }
}

#[cfg(all(test, not(target_arch = "wasm32"), not(feature = "kobo")))]
mod tests;
