//! One card of a sectioned listing. Child-entry columns and book covers share
//! one horizontal paginator, with entries first. Tab reaches each child and
//! book; Enter opens it and Ctrl+Enter opens its context menu.

use crate::library::widgets::PaginatorState;
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{AnyElement, App, Context, Entity, IntoElement, Render, SharedString, Subscription, Window, div};
use gpui_component::IconName;
use ui_components as components;

use super::cards::{CardEvent, CardEvents};
use super::drag_drop::{BookDrag, drag_payload};
use super::layout::{child_frame, download_mark, keyboard_item};
use super::listing::{BrowseItem, BrowseKind, BrowseSection};
use crate::library::widgets::{BookCardInteraction, BookCardPresentation, Paginator, PaginatorAxis, PaginatorChild, PaginatorGroup, browse_section_item_policy};

pub(super) struct SectionCard {
    events: CardEvents,
    id: String,
    child_icon: components::NavigationIcon,
    band_index: usize,
    section: BrowseSection,
    flow: Entity<Paginator>,
    _flow_subscription: Subscription,
}

impl SectionCard {
    pub(super) fn new(events: CardEvents, id: String, kind: BrowseKind, section: &BrowseSection, cx: &mut Context<Self>) -> Self {
        let child_icon = kind.child_icon();
        let groups = mixed_groups(&id, events.clone(), section, child_icon, cx);
        let flow = cx.new(|_| Paginator::new(format!("{id}-contents"), groups).flow_axis(PaginatorAxis::Horizontal).max_rows(1).horizontal_only());
        let subscription = cx.observe(&flow, |_, _, cx| cx.notify());
        let mut card = Self { events, id, child_icon, band_index: 0, section: section.clone(), flow, _flow_subscription: subscription };
        card.update_band();
        card
    }

    fn update_band(&mut self) {
        let parent = &self.section.parent;
        let key = match parent.id {
            library_model::BrowseLocation::Folder(_) => parent.name.clone(),
            library_model::BrowseLocation::Subject(route) | library_model::BrowseLocation::Catalog(route) => route.to_string(),
            library_model::BrowseLocation::Author(author) => author.map(|id| id.to_string()).unwrap_or_default(),
            library_model::BrowseLocation::Home(_) => parent.id.to_string(),
            library_model::BrowseLocation::Books | library_model::BrowseLocation::Opds { .. } => parent.id.to_string(),
        };
        self.band_index = components::section_card_band_index(self.child_icon, &key);
    }

    /// Installs a refreshed section in place, keeping the strip's position.
    pub(super) fn set_section(&mut self, section: &BrowseSection, cx: &mut Context<Self>) {
        let groups = mixed_groups(&self.id, self.events.clone(), section, self.child_icon, cx);
        self.flow.update(cx, |paginator, cx| paginator.refresh_groups(groups, cx));
        self.section = section.clone();
        self.update_band();
        cx.notify();
    }

    pub(super) fn present(card: Entity<Self>, focused: bool, marquee: bool) -> gpui::ViewElement<SectionCardElement> {
        gpui::ViewElement::new(SectionCardElement { card, focused, marquee })
    }

    /// Turns the shared strip through entry columns and then book covers.
    fn turn_page(&mut self, next: bool, cx: &mut Context<Self>) {
        self.flow.update(cx, |paginator, cx| if next { paginator.next_with_axis(PaginatorAxis::Horizontal, cx) } else { paginator.previous_with_axis(PaginatorAxis::Horizontal, cx) });
    }
}

pub(super) struct SectionCardElement {
    card: Entity<SectionCard>,
    focused: bool,
    marquee: bool,
}

impl gpui::View for SectionCardElement {
    fn entity_id(&self) -> Option<gpui::EntityId> {
        Some(self.card.entity_id())
    }
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        self.card.update(cx, |card, cx| card.render_card(self.focused, self.marquee, window, cx))
    }
}

impl SectionCard {
    fn render_card(&self, focused: bool, marquee: bool, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
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
            .child(arrow("previous-contents", "Previous entries or books", IconName::ArrowLeft, flow.has_previous(), false))
            .child(arrow("next-contents", "Next entries or books", IconName::ArrowRight, flow.has_next(), true));
        // A shelf of Home is a row of the page, not a place to go.
        let parent = &self.section.parent;
        let opens = !matches!(parent.id, library_model::BrowseLocation::Home(_));
        let heading = components::section_card_heading(format!("browse-card-title-{}", parent.id), parent.name.clone(), self.section.books.len(), arrows.into_any_element(), opens, focused, marquee)
            .debug_selector(|| format!("browse-card-title-{}", parent.id));
        components::section_card_frame(theme)
            .child(components::section_card_band(self.band_index, heading))
            .child(components::section_card_body().id(format!("{}-body", self.id)).mx(gpui::px(-components::BOOK_CARD_PADDING)).child(self.flow.clone()).on_click(|_, _, cx| cx.stop_propagation()))
            .into_any_element()
    }
}

impl Render for SectionCard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_card(false, false, window, cx)
    }
}

/// Child-entry columns come first in the same horizontal paginator as the
/// covers. A later page contains only covers once every entry has passed.
fn mixed_groups(id: &str, events: CardEvents, section: &BrowseSection, child_icon: components::NavigationIcon, cx: &App) -> Vec<PaginatorGroup> {
    // The folder this card shows, on a folder page. Its subfolders and books
    // are dragged out of it, and subfolders take drops.
    let folder = section.parent.id.folder();
    let mut children = Vec::new();
    let subject_columns = section.children.len().div_ceil(components::SECTION_CARD_SUBJECT_ROWS_PER_COLUMN);
    for (column_index, column) in section.children.chunks(components::SECTION_CARD_SUBJECT_ROWS_PER_COLUMN).enumerate() {
        let column = column.to_vec();
        let before_direct_books = column_index + 1 == subject_columns && !section.books.is_empty();
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
                        let index = column_index * components::SECTION_CARD_SUBJECT_ROWS_PER_COLUMN + row_index;
                        let row = components::section_card_subject(format!("{id}-subject-{index}"), child_icon, subject.name.clone(), subject.book_count, theme)
                            .debug_selector(|| format!("browse-section-subject-{}", subject.id))
                            .children(download_mark(subject, events.clone(), theme));
                        let item = BrowseItem::Child { row: subject.clone(), parent: folder };
                        let row = child_frame(row, subject, events.clone(), folder, vec![item.clone()], false, child_icon, theme, cx).on_mouse_down(gpui::MouseButton::Right, |_, _, cx| cx.stop_propagation());
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
    for book in &section.books {
        let dragged = book.clone();
        let drag = folder.map(|folder| Rc::new(move |_: &PaginatorState, _: &SharedString, cx: &App| drag_payload(&[BrowseItem::Book(dragged.clone())], folder, child_icon, cx)) as BookDrag);
        children.push(super::layout::book_child(book.clone(), BookCardPresentation::Section, |_, _, _| BookCardInteraction::default(), drag, cx));
    }
    vec![PaginatorGroup::new(browse_section_item_policy(), children)]
}
