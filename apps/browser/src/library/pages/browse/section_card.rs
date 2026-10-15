//! One card of a sectioned listing. Child-entry columns and book covers share
//! one horizontal paginator, with entries first. Tab reaches each child and
//! book; Enter opens it and Ctrl+Enter opens its context menu.

use crate::library::widgets::PaginatorState;
use std::{collections::BTreeMap, ops::Range, rc::Rc};

use app_preferences::CoverText;
use gpui::prelude::*;
use gpui::{AnyElement, App, Context, Entity, IntoElement, Pixels, Render, SharedString, Subscription, Task, WeakEntity, Window, div, px};
use gpui_component::IconName;
use ui_components as components;

use super::cards::{CardEvent, CardEvents};
use super::drag_drop::{BookDrag, drag_payload};
use super::layout::{child_frame, download_mark, keyboard_item};
use super::listing::{BrowseItem, BrowseKind, BrowseSection, shown_count};
use super::navigator::BrowseNavigator;
use super::profile::Stage;
use crate::library::widgets::BookCardView;
use crate::library::widgets::{BookCardInteraction, BookCardPresentation, Paginator, PaginatorAxis, PaginatorChild, PaginatorGroup, browse_section_item_policy, browse_section_subject_rows};

pub(super) struct SectionCard {
    events: CardEvents,
    library_id: uuid::Uuid,
    id: String,
    child_icon: components::NavigationIcon,
    band_index: usize,
    section: BrowseSection,
    flow: Entity<Paginator>,
    layout: (Pixels, CoverText),
    _flow_subscription: Subscription,
    navigator: WeakEntity<BrowseNavigator>,
    revision: u64,
    loaded_books: BTreeMap<usize, Entity<BookCardView>>,
    request: Option<Task<()>>,
    request_failed: bool,
    pending_turn: Option<bool>,
}

impl SectionCard {
    pub(super) fn new(events: CardEvents, library_id: uuid::Uuid, id: String, kind: BrowseKind, section: &BrowseSection, navigator: Entity<BrowseNavigator>, revision: u64, cx: &mut Context<Self>) -> Self {
        let child_icon = kind.child_icon();
        let layout = (px(16.0), crate::stores::current_cover_text(cx));
        let loaded_books = section.books.iter().cloned().enumerate().collect();
        let groups = mixed_groups(&id, events.clone(), library_id, section, &loaded_books, child_icon, layout.0, cx);
        let flow = cx.new(|_| Paginator::new(format!("{id}-contents"), groups).flow_axis(PaginatorAxis::Horizontal).max_rows(1).horizontal_only());
        let subscription = cx.observe(&flow, |card, _, cx| {
            card.load_visible_books(cx);
            cx.notify();
        });
        let mut card = Self {
            events,
            library_id,
            id,
            child_icon,
            band_index: 0,
            section: section.clone(),
            flow,
            layout,
            _flow_subscription: subscription,
            navigator: navigator.downgrade(),
            revision,
            loaded_books,
            request: None,
            request_failed: false,
            pending_turn: None,
        };
        card.update_band();
        card
    }

    fn update_band(&mut self) {
        let parent = &self.section.parent;
        let key = match parent.id {
            library_model::BrowseLocation::Folder(_) => parent.name.clone(),
            library_model::BrowseLocation::Subject(route) | library_model::BrowseLocation::Catalog(route) => route.to_string(),
            library_model::BrowseLocation::Author(author) | library_model::BrowseLocation::CatalogAuthor(author) => author.map(|id| id.to_string()).unwrap_or_default(),
            library_model::BrowseLocation::Home(_) => parent.id.to_string(),
            library_model::BrowseLocation::Books | library_model::BrowseLocation::Opds { .. } | library_model::BrowseLocation::Store(_) => parent.id.to_string(),
        };
        self.band_index = components::section_card_band_index(self.child_icon, &key);
    }

    /// Installs a refreshed section in place, keeping the strip's position.
    pub(super) fn set_section(&mut self, section: &BrowseSection, revision: u64, cx: &mut Context<Self>) {
        if self.revision != revision {
            self.request.take();
            self.request_failed = false;
            self.pending_turn = None;
            self.loaded_books = section.books.iter().cloned().enumerate().collect();
            self.revision = revision;
        }
        let groups = mixed_groups(&self.id, self.events.clone(), self.library_id, section, &self.loaded_books, self.child_icon, self.layout.0, cx);
        self.flow.update(cx, |paginator, cx| paginator.refresh_groups(groups, cx));
        self.section = section.clone();
        self.layout.1 = crate::stores::current_cover_text(cx);
        self.update_band();
        cx.notify();
    }

    pub(super) fn present(card: Entity<Self>, focused: bool, selected: bool, marquee: bool) -> gpui::ViewElement<SectionCardElement> {
        gpui::ViewElement::new(SectionCardElement { card, focused, selected, marquee })
    }

    /// Turns the shared strip through entry columns and then book covers.
    fn turn_page(&mut self, next: bool, cx: &mut Context<Self>) {
        if self.request.is_some() || !self.flow.read(cx).page_turn_ready() {
            return;
        }
        self.request_failed = false;
        if let Some(range) = self.flow.read(cx).adjacent_child_range(next) {
            if self.request_books(range, Some(next), cx) {
                return;
            }
        }
        self.flow.update(cx, |paginator, cx| if next { paginator.next_with_axis(PaginatorAxis::Horizontal, cx) } else { paginator.previous_with_axis(PaginatorAxis::Horizontal, cx) });
    }

    fn load_visible_books(&mut self, cx: &mut Context<Self>) {
        if self.request.is_some() || self.request_failed {
            return;
        }
        if let Some(next) = self.pending_turn {
            if self.flow.read(cx).page_turn_ready() {
                self.pending_turn = None;
                self.turn_page(next, cx);
            }
            return;
        }
        if let Some(range) = self.flow.read(cx).visible_child_range() {
            self.request_books(range, None, cx);
        }
    }

    /// Fetch only missing covers in the measured page. Entry columns occupy
    /// slots before books, so a strip of subjects needs no book request.
    fn request_books(&mut self, range: Range<usize>, turn: Option<bool>, cx: &mut Context<Self>) -> bool {
        let Some(total) = self.section.book_count else {
            return false;
        };
        let columns = self.section.children.len().div_ceil(browse_section_subject_rows(self.layout.1, self.layout.0));
        let Some(missing) = missing_book_range(range, columns, total, |index| self.loaded_books.contains_key(&index)) else {
            return false;
        };
        let offset = missing.start;
        let limit = missing.len().min(library_model::MAX_BROWSE_SECTION_BOOK_LIMIT);
        let Some(navigator) = self.navigator.upgrade() else {
            return false;
        };
        let navigator = navigator.read(cx);
        let Some(query) = navigator.section_query(self.section.parent.id) else {
            return false;
        };
        let ui = navigator.context();
        let result_query = navigator.result_query();
        let section = self.section.parent.id;
        let revision = self.revision;
        let anchor = self.flow.read(cx).visible_child_range().map(|range| range.start);
        self.request = Some(cx.spawn(async move |card, cx| {
            let request_stage = Stage::new("section arrow request including transport");
            let result = match result_query {
                Some((query, revision)) => ui.backend().browse_result_books(query, revision, section, offset, limit).await,
                None => ui.backend().browse_section_books(query, offset, limit).await.map(Some),
            };
            drop(request_stage);
            let failure = result.as_ref().err().cloned();
            let _ = card.update(cx, |card, cx| {
                let _stage = Stage::new("section arrow UI merge");
                if card.revision != revision {
                    return;
                }
                card.request.take();
                let Some(navigator) = card.navigator.upgrade() else { return };
                if navigator.read(cx).listing.revision != revision {
                    return;
                }
                match result {
                    Ok(Some(page)) => {
                        let unchanged = card.flow.read(cx).visible_child_range().map(|range| range.start) == anchor;
                        let books = navigator.update(cx, |navigator, cx| navigator.listing.section_book_cards(page.books, cx));
                        card.section.book_count = Some(page.total);
                        card.loaded_books.retain(|index, _| *index < page.total);
                        card.loaded_books.extend((page.offset..).zip(books));
                        let groups = mixed_groups(&card.id, card.events.clone(), card.library_id, &card.section, &card.loaded_books, card.child_icon, card.layout.0, cx);
                        card.flow.update(cx, |paginator, cx| paginator.refresh_groups(groups, cx));
                        card.pending_turn = turn.filter(|_| unchanged);
                    }
                    Ok(None) => {
                        navigator.update(cx, |navigator, cx| navigator.refresh(cx));
                    }
                    Err(error) => {
                        card.request_failed = true;
                        log::warn!("could not load section books: {error}");
                    }
                }
                cx.notify();
            });
            if let Some(error) = failure {
                let _ = card.update_in(cx, |card, window, cx| {
                    if card.revision == revision && card.request_failed {
                        crate::services::notify_error("section-books-error", format!("Could not load books: {error}"), window, cx);
                    }
                });
            }
        }));
        cx.notify();
        true
    }
}

fn missing_book_range(slots: Range<usize>, columns: usize, total: usize, loaded: impl Fn(usize) -> bool) -> Option<Range<usize>> {
    let range = slots.start.saturating_sub(columns).min(total)..slots.end.saturating_sub(columns).min(total);
    let start = range.clone().find(|index| !loaded(*index))?;
    let last = range.rev().find(|index| !loaded(*index))?;
    Some(start..last + 1)
}

#[cfg(test)]
mod tests {
    use super::missing_book_range;

    #[test]
    fn child_columns_do_not_fetch_books_and_mixed_pages_start_at_the_first_cover() {
        assert_eq!(missing_book_range(0..3, 5, 20, |_| false), None);
        assert_eq!(missing_book_range(3..8, 5, 20, |_| false), Some(0..3));
        assert_eq!(missing_book_range(7..10, 5, 20, |index| index < 3), Some(3..5));
    }

    #[test]
    fn backward_pages_reuse_loaded_covers_and_last_pages_stay_bounded() {
        assert_eq!(missing_book_range(8..11, 0, 20, |index| (8..11).contains(&index)), None);
        assert_eq!(missing_book_range(18..25, 0, 20, |_| false), Some(18..20));
        assert_eq!(missing_book_range(0..10, 0, 0, |_| false), None);
        assert_eq!(missing_book_range(30..40, 0, 20, |_| false), None);
    }
}

pub(super) struct SectionCardElement {
    card: Entity<SectionCard>,
    focused: bool,
    selected: bool,
    marquee: bool,
}

impl gpui::View for SectionCardElement {
    fn entity_id(&self) -> Option<gpui::EntityId> {
        Some(self.card.entity_id())
    }
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        self.card.update(cx, |card, cx| card.render_card(self.focused, self.selected, self.marquee, window, cx))
    }
}

impl SectionCard {
    fn render_card(&mut self, focused: bool, selected: bool, marquee: bool, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let layout = (window.rem_size(), crate::stores::current_cover_text(cx));
        if self.layout != layout {
            // Font and cover-text changes alter how many entry rows fit beside
            // a cover. Repack the columns while retaining the strip's position.
            self.layout = layout;
            let groups = mixed_groups(&self.id, self.events.clone(), self.library_id, &self.section, &self.loaded_books, self.child_icon, layout.0, cx);
            self.flow.update(cx, |paginator, cx| paginator.refresh_groups(groups, cx));
        }
        let theme = components::browser_theme(cx);
        let flow = self.flow.read(cx);
        let arrow = |id: &str, label: &str, icon: IconName, enabled: bool, forward: bool| {
            components::section_card_cover_arrow(format!("{}-{id}", self.id), label.to_owned(), icon, enabled).opacity(if enabled { 1.0 } else { 0.35 }).on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation()).on_click(
                cx.listener(move |card, _, _, cx| {
                    cx.stop_propagation();
                    if enabled {
                        card.turn_page(forward, cx);
                    }
                }),
            )
        };
        let arrows = div()
            .flex_none()
            .flex()
            .items_center()
            .gap(gpui::px(4.0))
            .text_size(gpui::rems(0.875))
            .text_color(gpui::rgb(0xf2efe8))
            .child(arrow("previous-contents", "Previous entries or books", IconName::ArrowLeft, flow.has_previous() && self.request.is_none() && self.pending_turn.is_none(), false))
            .child(arrow("next-contents", "Next entries or books", IconName::ArrowRight, flow.has_next() && self.request.is_none() && self.pending_turn.is_none(), true));
        // A shelf of Home is a row of the page, not a place to go.
        let parent = &self.section.parent;
        let opens = !matches!(parent.id, library_model::BrowseLocation::Home(_));
        let count = opens.then_some(self.section.book_count.unwrap_or(self.section.books.len()));
        let heading =
            components::section_card_heading(format!("browse-card-title-{}", parent.id), parent.name.clone(), count, arrows.into_any_element(), opens, focused, marquee, theme).debug_selector(|| format!("browse-card-title-{}", parent.id));
        components::section_card_frame(focused || selected, theme)
            .child(components::section_card_band(self.band_index, heading))
            .child(
                components::section_card_content(theme).child(components::section_card_body().id(format!("{}-body", self.id)).mx(gpui::px(-components::BOOK_CARD_PADDING)).child(self.flow.clone()).on_click(|_, _, cx| cx.stop_propagation())),
            )
            .into_any_element()
    }
}

impl Render for SectionCard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_card(false, false, false, window, cx)
    }
}

/// Child-entry columns come first in the same horizontal paginator as the
/// covers. A later page contains only covers once every entry has passed.
fn mixed_groups(
    id: &str, events: CardEvents, library_id: uuid::Uuid, section: &BrowseSection, loaded_books: &BTreeMap<usize, Entity<BookCardView>>, child_icon: components::NavigationIcon, rem_size: Pixels, cx: &mut App,
) -> Vec<PaginatorGroup> {
    // The folder this card shows, on a folder page. Its subfolders and books
    // are dragged out of it, and subfolders take drops.
    let folder = section.parent.id.folder();
    let mut children = Vec::new();
    let cover_text = crate::stores::current_cover_text(cx);
    let rows_per_column = browse_section_subject_rows(cover_text, rem_size);
    let subject_columns = section.children.len().div_ceil(rows_per_column);
    for (column_index, column) in section.children.chunks(rows_per_column).enumerate() {
        let column = column.to_vec();
        let before_direct_books = column_index + 1 == subject_columns && section.book_count.unwrap_or(section.books.len()) > 0;
        let item_id = format!("column:{}", column[0].id);
        let events = events.clone();
        let id = id.to_owned();
        children.push(
            PaginatorChild::new(move |_, _, cx| {
                let theme = components::browser_theme(cx);
                let rows = column
                    .iter()
                    .enumerate()
                    .map(|(row_index, subject)| {
                        let index = column_index * rows_per_column + row_index;
                        let row = components::section_card_subject(format!("{id}-subject-{index}"), child_icon, subject.name.clone(), shown_count(subject), theme)
                            .debug_selector(|| format!("browse-section-subject-{}", subject.id))
                            .children(download_mark(subject, events.clone(), theme));
                        let item = BrowseItem::Child { row: subject.clone(), parent: folder };
                        let row = child_frame(row, subject, events.clone(), library_id, folder, vec![item.clone()], false, child_icon, theme, cx)
                            .h(gpui::rems(components::SECTION_CARD_SUBJECT_ROW_HEIGHT_REM))
                            .flex_none()
                            .on_mouse_down(gpui::MouseButton::Right, |_, _, cx| cx.stop_propagation());
                        let activate = events.clone();
                        let menu = events.clone();
                        let location = subject.id;
                        keyboard_item(row, move |window, cx| activate(CardEvent::OpenChild(location), window, cx), move |position, window, cx| menu(CardEvent::ItemsMenu(vec![item.clone()], position), window, cx), cx).into_any_element()
                    })
                    .collect();
                components::section_card_subject_rows(rows).px(gpui::px(components::BOOK_CARD_PADDING)).when(before_direct_books, |rows| rows.pr(gpui::px(components::SECTION_CARD_COLUMN_GAP))).into_any_element()
            })
            .with_id(item_id),
        );
    }
    let placeholder = PaginatorChild::new(|_, _, _| div().size_full().into_any_element());
    for index in 0..section.book_count.unwrap_or(section.books.len()) {
        let slot = format!("{id}-book-slot-{index}");
        let Some(book) = loaded_books.get(&index) else {
            children.push(placeholder.clone().with_id(slot));
            continue;
        };
        let dragged = book.clone();
        let drag = Some(Rc::new(move |_: &PaginatorState, _: &SharedString, cx: &App| drag_payload(&[BrowseItem::Book(dragged.clone())], folder, library_id, child_icon, cx)) as BookDrag);
        children.push(super::layout::book_child(book.clone(), BookCardPresentation::Section, |_, _, _| BookCardInteraction::default(), drag, cx).with_id(slot));
    }
    vec![PaginatorGroup::new(browse_section_item_policy(cover_text), children)]
}
