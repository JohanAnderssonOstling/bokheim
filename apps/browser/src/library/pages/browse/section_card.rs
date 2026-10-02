//! One card of a sectioned listing. Child-entry columns and book covers share
//! one horizontal paginator, with entries first. The card owns the keyboard
//! within either kind of item and releases it at the outer edge.

use crate::library::widgets::PaginatorState;
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{AnyElement, App, Context, Entity, IntoElement, KeyDownEvent, Render, SharedString, Subscription, WeakEntity, Window, div};
use gpui_component::IconName;
use library_model::BrowseRow;
use sync_common::DirId;
use ui_components as components;

use super::folder_actions::FolderTargets;
use super::layout::{DragPreviewItem, child_frame, download_mark};
use super::listing::{BrowseItem, BrowseKind, BrowseSection};
use super::page::BrowsePage;
use crate::library::widgets::{BookCardInteraction, BookCardPresentation, BookCardView, CursorMove, Paginator, PaginatorAxis, PaginatorChild, PaginatorGroup, browse_section_item_policy};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SectionRail {
    Subjects,
    Books,
}

pub(super) struct SectionCard {
    page: WeakEntity<BrowsePage>,
    id: String,
    child_icon: components::NavigationIcon,
    band_index: usize,
    /// The folder this card shows, on a folder page. Its subfolders and books
    /// are dragged out of it, and subfolders take drops.
    folder: Option<DirId>,
    subjects: Vec<BrowseRow>,
    subject_cursor: Option<usize>,
    flow: Entity<Paginator>,
    parent: BrowseRow,
    _flow_subscription: Subscription,
    book_entities: Vec<Entity<BookCardView>>,
    /// The part holding the keyboard, if the card has it at all.
    rail: Option<SectionRail>,
}

impl SectionCard {
    pub(super) fn new(page: WeakEntity<BrowsePage>, id: String, kind: BrowseKind, section: &BrowseSection, cx: &mut Context<Self>) -> Self {
        let child_icon = kind.child_icon();
        let folder = if let library_model::BrowseLocation::Folder(id) = section.parent.id { Some(id) } else { None };
        let groups = mixed_groups(&id, page.clone(), section, folder, child_icon, cx.entity().downgrade(), cx);
        let flow = cx.new(|_| Paginator::new(format!("{id}-contents"), groups).flow_axis(PaginatorAxis::Horizontal).max_rows(1).horizontal_only());
        let subscription = cx.observe(&flow, |_, _, cx| cx.notify());
        let mut card = Self {
            page,
            id,
            child_icon,
            band_index: 0,
            folder,
            subjects: section.children.clone(),
            subject_cursor: None,
            flow,
            parent: section.parent.clone(),
            _flow_subscription: subscription,
            book_entities: section.books.clone(),
            rail: None,
        };
        card.update_band();
        card
    }

    fn update_band(&mut self) {
        let key = match self.parent.id {
            library_model::BrowseLocation::Folder(_) => self.parent.name.clone(),
            library_model::BrowseLocation::Subject(route) => route.to_string(),
        };
        self.band_index = components::section_card_band_index(self.child_icon, &key);
    }

    /// Installs a refreshed section in place, keeping each part's position.
    pub(super) fn set_section(&mut self, section: &BrowseSection, cx: &mut Context<Self>) {
        self.parent = section.parent.clone();
        self.update_band();
        let groups = mixed_groups(&self.id, self.page.clone(), section, self.folder, self.child_icon, cx.entity().downgrade(), cx);
        self.flow.update(cx, |paginator, cx| paginator.refresh_groups(groups, cx));
        let previous_subject = self.subject_cursor.and_then(|index| self.subjects.get(index)).map(|row| row.id.clone());
        self.subjects = section.children.clone();
        self.book_entities = section.books.clone();
        self.subject_cursor = previous_subject.as_ref().and_then(|id| self.subjects.iter().position(|row| &row.id == id));
        if self.rail.is_some_and(|rail| !self.has_items(rail)) {
            self.rail = None;
        }
        self.sync(cx);
    }

    pub(super) fn has_subjects(&self) -> bool {
        !self.subjects.is_empty()
    }

    pub(super) fn has_books(&self) -> bool {
        !self.book_entities.is_empty()
    }

    pub(super) fn present(card: Entity<Self>, focused: bool, marquee: bool) -> gpui::ViewElement<SectionCardElement> {
        gpui::ViewElement::new(SectionCardElement { card, focused, marquee })
    }

    fn subject_columns(&self) -> usize {
        self.subjects.len().div_ceil(components::SECTION_CARD_SUBJECT_ROWS_PER_COLUMN)
    }

    pub(super) fn is_focused(&self) -> bool {
        self.rail.is_some()
    }

    fn has_items(&self, rail: SectionRail) -> bool {
        match rail {
            SectionRail::Subjects => self.has_subjects(),
            SectionRail::Books => self.has_books(),
        }
    }

    /// Takes the keyboard from a move arriving at the card. Moving left enters
    /// the covers, since that is the side it came in from; anything else
    /// enters the subjects. Arriving from the right or below lands on the last
    /// item.
    pub(super) fn enter(&mut self, movement: CursorMove, cx: &mut Context<Self>) -> bool {
        let rail = if movement == CursorMove::Left && self.has_books() || !self.has_subjects() { SectionRail::Books } else { SectionRail::Subjects };
        self.focus(rail, matches!(movement, CursorMove::Left | CursorMove::Up), cx)
    }

    pub(super) fn leave(&mut self, cx: &mut Context<Self>) {
        if self.rail.take().is_some() {
            self.sync(cx);
        }
    }

    fn focus(&mut self, rail: SectionRail, last: bool, cx: &mut Context<Self>) -> bool {
        if !self.has_items(rail) {
            return false;
        }
        self.rail = Some(rail);
        match rail {
            SectionRail::Subjects => {
                let last_subject = self.subjects.len() - 1;
                self.subject_cursor = Some(if last { last_subject } else { self.subject_cursor.unwrap_or(0).min(last_subject) });
                let column = self.subject_cursor.expect("subject focus has a cursor") / components::SECTION_CARD_SUBJECT_ROWS_PER_COLUMN;
                self.flow.update(cx, |paginator, cx| paginator.restore_cursor(column, column, cx));
            }
            SectionRail::Books => {
                let first = self.subject_columns();
                let target = if last { first + self.book_entities.len() - 1 } else { first };
                self.flow.update(cx, |paginator, cx| paginator.restore_cursor(target, target, cx));
            }
        }
        self.sync(cx);
        true
    }

    /// Child entries fill columns from top to bottom. Horizontal movement
    /// crosses columns before it reaches the book strip.
    fn move_subject_cursor(&mut self, movement: CursorMove) -> bool {
        let (count, rows) = (self.subjects.len(), components::SECTION_CARD_SUBJECT_ROWS_PER_COLUMN);
        let Some(current) = self.subject_cursor else { return false };
        let (column, row) = (current / rows, current % rows);
        let target = match movement {
            CursorMove::Up => (row > 0).then(|| current - 1),
            CursorMove::Down => (row + 1 < rows && current + 1 < count).then(|| current + 1),
            CursorMove::Left => column.checked_sub(1).map(|previous| previous * rows + row),
            CursorMove::Right => {
                let next_start = (column + 1) * rows;
                (next_start < count).then(|| (next_start + row).min(count - 1))
            }
        };
        let Some(target) = target else { return false };
        self.subject_cursor = Some(target);
        true
    }

    /// Moves within the focused part. Returns false when the move runs off the
    /// card, for the page to continue it in the outer listing.
    pub(super) fn move_cursor(&mut self, movement: CursorMove, cx: &mut Context<Self>) -> bool {
        let Some(rail) = self.rail else { return self.enter(movement, cx) };
        let moved = match rail {
            SectionRail::Subjects => {
                let previous_column = self.subject_cursor.map(|cursor| cursor / components::SECTION_CARD_SUBJECT_ROWS_PER_COLUMN);
                let moved = self.move_subject_cursor(movement);
                let next_column = self.subject_cursor.map(|cursor| cursor / components::SECTION_CARD_SUBJECT_ROWS_PER_COLUMN);
                if moved && previous_column != next_column {
                    let column = next_column.expect("moving a subject keeps a cursor");
                    self.flow.update(cx, |paginator, cx| paginator.restore_cursor(column, column, cx));
                }
                moved
            }
            SectionRail::Books => {
                let paginator = self.flow.clone();
                let moved = paginator.update(cx, |paginator, cx| paginator.move_cursor(movement, cx));
                // A page still turning swallows the key rather than leaving
                // the card from a position the reader cannot see yet.
                if !moved && !paginator.read(cx).is_idle() {
                    return true;
                }
                if moved && paginator.read(cx).cursor().is_some_and(|cursor| cursor < self.subject_columns()) {
                    self.rail = Some(SectionRail::Subjects);
                    self.subject_cursor = Some(self.subjects.len() - 1);
                }
                moved
            }
        };
        if moved {
            self.sync(cx);
            return true;
        }
        // The covers sit to the right of the subjects, so a horizontal edge
        // crosses to the other part before it leaves the card.
        match (movement, rail) {
            (CursorMove::Right, SectionRail::Subjects) if self.has_books() => self.focus(SectionRail::Books, false, cx),
            (CursorMove::Left, SectionRail::Books) if self.has_subjects() => self.focus(SectionRail::Subjects, true, cx),
            _ => false,
        }
    }

    /// Tab walks from the subjects to the covers and Shift-Tab back. Past
    /// either end the card lets go of the keyboard and returns false, leaving
    /// the key to the ordinary focus chain.
    fn tab(&mut self, shift: bool, cx: &mut Context<Self>) -> bool {
        if self.rail.is_none() {
            return self.enter(if shift { CursorMove::Up } else { CursorMove::Down }, cx);
        }
        let next = match (self.rail, shift) {
            (Some(SectionRail::Subjects), false) if self.has_books() => Some(SectionRail::Books),
            (Some(SectionRail::Books), true) if self.has_subjects() => Some(SectionRail::Subjects),
            _ => None,
        };
        match next {
            Some(rail) => self.focus(rail, shift, cx),
            None => {
                self.leave(cx);
                false
            }
        }
    }

    /// Section-specific keys stay here; the page only handles movement out.
    pub(super) fn handle_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) -> Option<bool> {
        let modifiers = event.keystroke.modifiers;
        let plain = modifiers == gpui::Modifiers::default();
        let shift_only = modifiers == gpui::Modifiers { shift: true, ..gpui::Modifiers::default() };
        match event.keystroke.key.as_str() {
            "tab" if plain || shift_only => self.tab(shift_only, cx).then_some(true),
            "enter" if plain && self.is_focused() => Some(match self.activation(cx) {
                Some(activate) => {
                    window.defer(cx, move |window, cx| activate(window, cx));
                    true
                }
                None => false,
            }),
            "pageup" | "pagedown" if plain && self.is_focused() => Some(self.turn_page(event.keystroke.key == "pagedown", cx)),
            _ => None,
        }
    }

    /// Turns the shared strip through entry columns and then book covers.
    pub(super) fn turn_page(&mut self, next: bool, cx: &mut Context<Self>) -> bool {
        let moved = self.flow.update(cx, |paginator, cx| if next { paginator.next_with_axis(PaginatorAxis::Horizontal, cx) } else { paginator.previous_with_axis(PaginatorAxis::Horizontal, cx) });
        if !moved {
            return false;
        }
        let anchor = self.flow.read(cx).child_anchor();
        self.flow.update(cx, |paginator, cx| paginator.focus_child_on_current_page(anchor, cx));
        if self.rail.is_some() {
            if anchor < self.subject_columns() {
                self.rail = Some(SectionRail::Subjects);
                self.subject_cursor = Some((anchor * components::SECTION_CARD_SUBJECT_ROWS_PER_COLUMN).min(self.subjects.len() - 1));
            } else {
                self.rail = Some(SectionRail::Books);
            }
        }
        self.sync(cx);
        true
    }

    /// What Enter would run: the focused item, if it is on screen. Handed out
    /// rather than run, because it navigates the page that is asking.
    pub(super) fn activation(&self, cx: &App) -> Option<Rc<dyn Fn(&mut Window, &mut App)>> {
        match self.rail? {
            SectionRail::Subjects => {
                let location = self.subjects.get(self.subject_cursor?)?.id.clone();
                let page = self.page.clone();
                Some(Rc::new(move |window, cx| {
                    let _ = page.update(cx, |page, cx| page.open_child(location.clone(), window, cx));
                }))
            }
            SectionRail::Books => {
                let paginator = self.flow.read(cx);
                paginator.child_bounds(paginator.cursor()?)?;
                paginator.cursor_activation()
            }
        }
    }

    /// The subjects draw their own cursor; book cards are entities and are
    /// told.
    fn sync(&mut self, cx: &mut Context<Self>) {
        self.flow.update(cx, |_, cx| cx.notify());
        cx.notify();
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
        let heading = components::section_card_heading(format!("browse-card-title-{}", self.parent.id), self.parent.name.clone(), self.book_entities.len(), arrows.into_any_element(), focused, marquee)
            .debug_selector(|| format!("browse-card-title-{}", self.parent.id));
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
fn mixed_groups(id: &str, page: WeakEntity<BrowsePage>, section: &BrowseSection, folder: Option<DirId>, child_icon: components::NavigationIcon, card: WeakEntity<SectionCard>, cx: &App) -> Vec<PaginatorGroup> {
    let mut children = Vec::new();
    let subject_columns = section.children.len().div_ceil(components::SECTION_CARD_SUBJECT_ROWS_PER_COLUMN);
    for (column_index, column) in section.children.chunks(components::SECTION_CARD_SUBJECT_ROWS_PER_COLUMN).enumerate() {
        let column = column.to_vec();
        let before_direct_books = column_index + 1 == subject_columns && !section.books.is_empty();
        let item_id = format!("column:{}", column[0].id);
        let page = page.clone();
        let subject_card = card.clone();
        let id = id.to_owned();
        children.push(
            PaginatorChild::new(move |_, _, cx| {
                let theme = components::browser_theme(cx);
                let rows = column
                    .iter()
                    .enumerate()
                    .map(|(row_index, subject)| {
                        let index = column_index * components::SECTION_CARD_SUBJECT_ROWS_PER_COLUMN + row_index;
                        let focused = subject_card.upgrade().is_some_and(|card| {
                            let card = card.read(cx);
                            card.rail == Some(SectionRail::Subjects) && card.subject_cursor == Some(index)
                        });
                        let row = components::section_card_subject(format!("{id}-subject-{index}"), child_icon, subject.name.clone(), subject.book_count, focused, theme)
                            .debug_selector(|| format!("browse-section-subject-{}", subject.id))
                            .children(download_mark(subject, page.clone(), theme));
                        let item = BrowseItem::Child { row: subject.clone(), parent: folder };
                        let row = child_frame(row, subject, page.clone(), folder, vec![item], false, child_icon, theme, cx).on_mouse_down(gpui::MouseButton::Right, |_, _, cx| cx.stop_propagation());
                        row.into_any_element()
                    })
                    .collect();
                components::section_card_subject_rows(rows).px(gpui::px(components::BOOK_CARD_PADDING)).when(before_direct_books, |rows| rows.pr(gpui::px(components::SECTION_CARD_COLUMN_GAP))).into_any_element()
            })
            .with_id(item_id),
        );
    }
    for book in &section.books {
        let card = card.clone();
        let dragged = book.clone();
        let drag = folder.map(|folder| {
            Rc::new(move |_: &PaginatorState, _: &SharedString, cx: &App| {
                FolderTargets::from_items(&[BrowseItem::Book(dragged.clone())], folder, cx).map(|targets| (targets, vec![DragPreviewItem::Book(dragged.clone())]))
            }) as Rc<dyn Fn(&PaginatorState, &SharedString, &App) -> Option<(FolderTargets, Vec<DragPreviewItem>)>>
        });
        children.push(super::layout::book_child(
            book.clone(),
            BookCardPresentation::Section,
            move |state, id, cx| BookCardInteraction { focused: state.is_focused(id) && card.upgrade().is_some_and(|card| card.read(cx).rail == Some(SectionRail::Books)), ..Default::default() },
            drag,
            cx,
        ));
    }
    vec![PaginatorGroup::new(browse_section_item_policy(), children)]
}
