//! Folder cards preview books or one column of nested folders.
//! Container cards share the height of book previews across library pages.
//! Home shelves and author cards show books in a horizontal strip.
//! Tab reaches each child and book; Enter opens it and Ctrl+Enter opens its menu.

use ui_components::user_facing_error;

use crate::library::widgets::PaginatorState;
use std::{
    cell::Cell,
    collections::{BTreeMap, HashMap},
    ops::Range,
    rc::Rc,
};

use app_preferences::CoverText;
use gpui::prelude::*;
use gpui::{AnyElement, App, Bounds, Context, Entity, IntoElement, Pixels, Render, SharedString, Subscription, Task, WeakEntity, Window, div, px};
use gpui_component::IconName;
use sync_common::DirId;
use ui_components as components;

use super::cards::{CardEvent, CardEvents};
use super::drag_drop::{BookDrag, DragPayload, drag_payload, folder_drag_source};
use super::layout::{ChildInteraction, child_frame, child_interactions, download_mark, keyboard_item};
use super::listing::{BrowseItem, BrowseItems, BrowseKind, BrowseSection, chip_id, is_author_heading, resolve_items, section_shows_books, shown_count};
use super::navigator::BrowseNavigator;
use super::profile::Stage;
use crate::library::widgets::BookCardView;
use crate::library::widgets::{
    BookCardInteraction, BookCardPresentation, Paginator, PaginatorAxis, PaginatorChild, PaginatorGroup, PaginatorGroupPolicy, PaginatorSizing, browse_folder_item_policy, browse_section_item_policy, browse_section_subject_rows,
};

#[derive(Clone, Copy, PartialEq)]
struct SectionLayout {
    rem_size: Pixels,
    cover_text: CoverText,
    mobile: bool,
}

pub(super) struct SectionCard {
    events: CardEvents,
    library_id: uuid::Uuid,
    id: String,
    child_icon: components::NavigationIcon,
    band_index: usize,
    section: BrowseSection,
    flow: Entity<Paginator>,
    layout: SectionLayout,
    _flow_subscription: Subscription,
    navigator: WeakEntity<BrowseNavigator>,
    revision: u64,
    loaded_books: BTreeMap<usize, Entity<BookCardView>>,
    request: Option<Task<()>>,
    request_failed: bool,
    pending_turn: Option<bool>,
}

impl SectionCard {
    pub(super) fn selected_ids(&self, cx: &App) -> Vec<SharedString> {
        let flow = self.flow.read(cx);
        flow.selection().iter().filter_map(|&index| flow.child_id(index).cloned()).collect()
    }

    pub(super) fn selected_items(&self, cx: &App) -> (Option<DirId>, Vec<BrowseItem>) {
        (self.section.parent.id.folder(), resolve_items(&self.selected_ids(cx), &section_items(&self.id, &self.section, &self.loaded_books)))
    }

    pub(super) fn clear_selection(&mut self, cx: &mut Context<Self>) -> bool {
        self.flow.update(cx, |flow, cx| flow.clear_selection(cx))
    }

    pub(super) fn new(events: CardEvents, library_id: uuid::Uuid, id: String, kind: BrowseKind, section: &BrowseSection, navigator: Entity<BrowseNavigator>, revision: u64, mobile: bool, cx: &mut Context<Self>) -> Self {
        let child_icon = kind.child_icon();
        let layout = SectionLayout { rem_size: px(16.0), cover_text: crate::stores::current_cover_text(cx), mobile };
        let loaded_books = section.books.iter().cloned().enumerate().collect();
        let groups = mixed_groups(&id, events.clone(), library_id, section, &loaded_books, child_icon, layout, cx);
        let owner = cx.entity().downgrade();
        let menu_events = events.clone();
        let flow = cx.new(|_| {
            Paginator::new(format!("{id}-contents"), groups).flow_axis(PaginatorAxis::Horizontal).horizontal_only().select_on_drag().next_page_preview(!container_section(section) && section.parent.id.folder().is_none()).on_context_menu(
                move |id, position, window, cx| {
                    let Some(card) = owner.upgrade() else { return };
                    let card = card.read(cx);
                    let flow = card.flow.read(cx);
                    let ids = id.map(|id| flow.target_ids(&id)).unwrap_or_else(|| flow.selection().iter().filter_map(|&index| flow.child_id(index).cloned()).collect());
                    let items = resolve_items(&ids, &section_items(&card.id, &card.section, &card.loaded_books));
                    if !items.is_empty() {
                        menu_events(CardEvent::ItemsMenu(items, position), window, cx);
                        cx.stop_propagation();
                    }
                },
            )
        });
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
    pub(super) fn set_section(&mut self, section: &BrowseSection, revision: u64, mobile: bool, cx: &mut Context<Self>) {
        if self.revision != revision || self.layout.mobile != mobile {
            self.request.take();
            self.request_failed = false;
            self.pending_turn = None;
            self.loaded_books = section.books.iter().cloned().enumerate().collect();
            self.revision = revision;
        }
        self.layout.mobile = mobile;
        self.layout.cover_text = crate::stores::current_cover_text(cx);
        let groups = mixed_groups(&self.id, self.events.clone(), self.library_id, section, &self.loaded_books, self.child_icon, self.layout, cx);
        self.flow.update(cx, |paginator, cx| paginator.refresh_groups(groups, cx));
        self.section = section.clone();
        self.update_band();
        cx.notify();
    }

    pub(super) fn present(card: Entity<Self>, focused: bool, selected: bool, marquee: bool, header_drag: Option<DragPayload>, preview: bool) -> gpui::ViewElement<SectionCardElement> {
        gpui::ViewElement::new(SectionCardElement { card, focused, selected, marquee, header_drag, preview })
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
        if container_section(&self.section) {
            return false;
        }
        let Some(total) = self.section.book_count else {
            return false;
        };
        let columns = self.section.children.len().div_ceil(browse_section_subject_rows(self.layout.cover_text, self.layout.rem_size));
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
                        let groups = mixed_groups(&card.id, card.events.clone(), card.library_id, &card.section, &card.loaded_books, card.child_icon, card.layout, cx);
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
                        crate::services::notify_error("section-books-error", format!("Could not load books. {}", user_facing_error(&error, "Please try again.")), window, cx);
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
    use super::*;
    use gpui::TestAppContext;
    use library_model::{BrowseLocation, BrowseRow};
    use uuid::Uuid;

    #[cfg(all(not(target_arch = "wasm32"), not(feature = "kobo")))]
    #[gpui::test]
    async fn nested_container_context_menus_target_entries_and_the_enclosing_folder(cx: &mut TestAppContext) {
        use super::super::listing::BrowseListing;
        use crate::library::widgets::CursorMove;
        use crate::navigation::LibrarySession;
        use gpui::{Modifiers, MouseButton, point};
        use std::cell::RefCell;
        use sync_common::ROOT_DIR_ID;

        let _backend = crate::test_support::BACKEND.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let (_directory, _session, library) = LibrarySession::test_context(cx).await;
        let library_id = *library.backend().id();
        let listing = BrowseListing::new(library.clone(), BrowseKind::Folder, Rc::new(|_, _, _| {}), false);
        let navigator = cx.new(|cx| BrowseNavigator::new(library, BrowseKind::Folder, listing, cx));
        let row = |id| BrowseRow { id: BrowseLocation::Folder(Uuid::from_u128(id)), name: format!("Folder {id}"), path: None, book_count: 0, downloaded_book_count: 0 };
        let section = BrowseSection { parent: row(1), children: (2..6).map(row).collect(), books: Vec::new(), book_count: Some(0) };
        let menus = Rc::new(RefCell::new(Vec::new()));
        let recorded = menus.clone();
        let events: CardEvents = Rc::new(move |event, _, _| {
            if let CardEvent::ItemsMenu(items, _) = event {
                recorded.borrow_mut().push(items.iter().filter_map(|item| item.child().map(|row| row.id)).collect::<Vec<_>>());
            }
        });
        let card = cx.new(|cx| SectionCard::new(events.clone(), library_id, "menu-card".into(), BrowseKind::Folder, &section, navigator, 0, false, cx));
        let inner = card.read_with(cx, |card, _| card.flow.clone());
        let parent = section.parent.id;
        let (outer, visual) = cx.add_window_view(|_, cx| {
            let items = Rc::new(HashMap::from([(chip_id(parent), BrowseItem::Child { row: section.parent.clone(), parent: Some(ROOT_DIR_ID) })]));
            let cards = HashMap::from([(parent, card)]);
            let groups =
                super::super::layout::paginator_groups(events, library_id, Rc::new(vec![section.parent]), BrowseKind::Folder, Rc::new(Vec::new()), Some(ROOT_DIR_ID), &cards, items, None, &BTreeMap::new(), &BTreeMap::new(), false, cx);
            Paginator::new("menu-outer", groups).selectable()
        });
        visual.run_until_parked();
        let entry = visual.update(|_, cx| inner.read(cx).child_bounds(0).unwrap().center());
        let bounds = visual.update(|_, cx| outer.read(cx).child_bounds(0).unwrap());
        let header = bounds.origin + point(px(40.0), px(20.0));
        let empty_body = point(bounds.center().x, bounds.bottom() - px(30.0));
        for (position, expected) in [(entry, section.children[0].id), (header, parent), (empty_body, parent)] {
            visual.simulate_mouse_down(position, MouseButton::Right, Modifiers::default());
            visual.simulate_mouse_up(position, MouseButton::Right, Modifiers::default());
            visual.run_until_parked();
            assert_eq!(menus.borrow_mut().drain(..).collect::<Vec<_>>(), vec![vec![expected]], "right-click at {position:?} must open exactly one menu for the clicked entry or container");
        }
        visual.update(|_, cx| {
            inner.update(cx, |flow, cx| {
                flow.move_cursor(CursorMove::Down, cx);
                flow.move_and_toggle_selection(CursorMove::Down, cx);
            });
        });
        visual.run_until_parked();
        assert_eq!(visual.update(|_, cx| inner.read(cx).selection().iter().copied().collect::<Vec<_>>()), vec![0, 1]);
        for position in [entry, empty_body] {
            visual.simulate_mouse_down(position, MouseButton::Right, Modifiers::default());
            visual.simulate_mouse_up(position, MouseButton::Right, Modifiers::default());
            visual.run_until_parked();
            assert_eq!(menus.borrow_mut().drain(..).collect::<Vec<_>>(), vec![vec![section.children[0].id, section.children[1].id]], "right-click must preserve the nested selection and open its menu once");
        }
    }

    #[cfg(all(not(target_arch = "wasm32"), not(feature = "kobo")))]
    #[gpui::test]
    async fn only_the_header_drags_the_enclosing_folder(cx: &mut TestAppContext) {
        use super::super::listing::BrowseListing;
        use crate::navigation::LibrarySession;
        use gpui::{MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, point};
        use sync_common::ROOT_DIR_ID;

        let _backend = crate::test_support::BACKEND.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let (_directory, _session, library) = LibrarySession::test_context(cx).await;
        let library_id = *library.backend().id();
        let listing = BrowseListing::new(library.clone(), BrowseKind::Folder, Rc::new(|_, _, _| {}), false);
        let navigator = cx.new(|cx| BrowseNavigator::new(library, BrowseKind::Folder, listing, cx));
        let row = |id| BrowseRow { id: BrowseLocation::Folder(Uuid::from_u128(id)), name: format!("Folder {id}"), path: None, book_count: 0, downloaded_book_count: 0 };
        let section = BrowseSection { parent: row(1), children: (2..6).map(row).collect(), books: Vec::new(), book_count: Some(0) };
        let events: CardEvents = Rc::new(|_, _, _| {});
        let card = cx.new(|cx| SectionCard::new(events.clone(), library_id, "test-card".into(), BrowseKind::Folder, &section, navigator, 0, false, cx));
        let inner = card.read_with(cx, |card, _| card.flow.clone());
        let (outer, visual) = cx.add_window_view(|_, cx| {
            let items = Rc::new(HashMap::from([(chip_id(section.parent.id), BrowseItem::Child { row: section.parent.clone(), parent: Some(ROOT_DIR_ID) })]));
            let cards = HashMap::from([(section.parent.id, card)]);
            let groups =
                super::super::layout::paginator_groups(events, library_id, Rc::new(vec![section.parent]), BrowseKind::Folder, Rc::new(Vec::new()), Some(ROOT_DIR_ID), &cards, items, None, &BTreeMap::new(), &BTreeMap::new(), false, cx);
            Paginator::new("outer", groups).selectable()
        });
        visual.run_until_parked();
        let inner_bounds = visual.update(|_, cx| inner.read(cx).child_bounds(0).unwrap());
        let card_size = visual.update(|_, cx| outer.read(cx).child_bounds(0).unwrap().size);
        let full_cards = |window: &Window| {
            window
                .painted_quads()
                .iter()
                .filter(|quad| (quad.bounds.size.width.0 - f32::from(card_size.width) * window.scale_factor()).abs() < 0.5 && (quad.bounds.size.height.0 - f32::from(card_size.height) * window.scale_factor()).abs() < 0.5)
                .count()
        };
        let source_cards = visual.update(|window, _| full_cards(window));
        let header = visual.update(|_, cx| outer.read(cx).child_bounds(0).unwrap().origin + point(px(40.0), px(20.0)));
        visual.simulate_event(MouseDownEvent { position: header, button: MouseButton::Left, click_count: 1, ..Default::default() });
        visual.simulate_event(MouseMoveEvent { position: header + point(px(30.0), px(0.0)), pressed_button: Some(MouseButton::Left), ..Default::default() });
        visual.run_until_parked();
        assert!(visual.update(|_, cx| cx.has_active_drag()), "the header must carry the enclosing folder");
        assert!(visual.update(|window, _| full_cards(window)) > source_cards, "the drag must render the full-size card, not a folder chip");
        assert_eq!(visual.update(|_, cx| inner.read(cx).child_bounds(0).unwrap()), inner_bounds, "drawing the preview must not move the source's hit bounds");
        visual.update(|window, cx| cx.stop_active_drag(window));
        visual.simulate_event(MouseUpEvent { position: header, button: MouseButton::Left, ..Default::default() });
        visual.run_until_parked();

        let (start, end) = visual.update(|_, cx| {
            let inner = inner.read(cx);
            (inner.child_bounds(0).unwrap().center(), inner.child_bounds(1).unwrap().center() + point(px(10.0), px(0.0)))
        });
        visual.simulate_event(MouseDownEvent { position: start, button: MouseButton::Left, click_count: 1, ..Default::default() });
        visual.simulate_event(MouseMoveEvent { position: end, pressed_button: Some(MouseButton::Left), ..Default::default() });
        visual.run_until_parked();
        assert!(!visual.update(|_, cx| cx.has_active_drag()), "dragging inside must select children instead of carrying the enclosing folder");
        visual.simulate_event(MouseUpEvent { position: end, button: MouseButton::Left, ..Default::default() });
        visual.run_until_parked();
        assert_eq!(visual.update(|_, cx| inner.read(cx).selection().iter().copied().collect::<Vec<_>>()), vec![0, 1]);
        let (padding, end) = visual.update(|_, cx| {
            let flow = inner.read(cx);
            let first = flow.child_bounds(0).unwrap();
            (point(first.left() - px(2.0), first.center().y), flow.child_bounds(1).unwrap().center())
        });
        visual.simulate_event(MouseDownEvent { position: padding, button: MouseButton::Left, click_count: 1, ..Default::default() });
        visual.simulate_event(MouseMoveEvent { position: end, pressed_button: Some(MouseButton::Left), ..Default::default() });
        visual.run_until_parked();
        assert!(visual.update(|_, cx| inner.read(cx).marquee_active()), "the nested paginator must receive selection drags from row padding");
        assert!(!visual.update(|_, cx| cx.has_active_drag()));
        visual.simulate_event(MouseUpEvent { position: end, button: MouseButton::Left, ..Default::default() });
        visual.run_until_parked();
    }

    #[gpui::test]
    fn nested_entry_padding_starts_selection_instead_of_opening_or_dragging_folders(cx: &mut TestAppContext) {
        use crate::library::widgets::CursorMove;
        use gpui::{Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, point};
        use std::cell::RefCell;

        cx.update(gpui_component::init);
        let row = |id| BrowseRow { id: BrowseLocation::Folder(Uuid::from_u128(id)), name: format!("Folder {id}"), path: None, book_count: 0, downloaded_book_count: 0 };
        let section = BrowseSection { parent: row(1), children: (2..6).map(row).collect(), books: Vec::new(), book_count: Some(0) };
        let received = Rc::new(RefCell::new(Vec::new()));
        let recorded = received.clone();
        let events: CardEvents = Rc::new(move |event, _, _| recorded.borrow_mut().push(event));
        let (flow, visual) = cx.add_window_view(|_, cx| {
            let layout = SectionLayout { rem_size: px(16.0), cover_text: CoverText::CoverOnly, mobile: false };
            Paginator::new("entry-hit-bounds", mixed_groups("test", events, Uuid::nil(), &section, &BTreeMap::new(), components::NavigationIcon::Folder, layout, cx)).select_on_drag()
        });
        visual.run_until_parked();
        let first = visual.debug_bounds("browse-section-subject-folder:00000000-0000-0000-0000-000000000002").unwrap();
        let second = visual.debug_bounds("browse-section-subject-folder:00000000-0000-0000-0000-000000000003").unwrap();
        assert_eq!(visual.update(|_, cx| flow.read(cx).child_bounds(0).unwrap()), first, "selection must use the highlighted entry's bounds");
        assert_eq!(visual.update(|_, cx| flow.read(cx).child_bounds(1).unwrap()), second);

        for x in [first.left() - px(2.0), first.right() + px(2.0)] {
            let padding = point(x, first.center().y);
            visual.simulate_mouse_down(padding, MouseButton::Left, Modifiers::default());
            visual.simulate_mouse_up(padding, MouseButton::Left, Modifiers::default());
            visual.run_until_parked();
            assert!(received.borrow().is_empty(), "clicking beside the highlight must not open the entry");
        }
        visual.simulate_mouse_down(first.center(), MouseButton::Left, Modifiers::default());
        visual.simulate_mouse_up(first.center(), MouseButton::Left, Modifiers::default());
        visual.run_until_parked();
        assert!(matches!(received.borrow_mut().drain(..).collect::<Vec<_>>().as_slice(), [CardEvent::OpenChild(location)] if *location == section.children[0].id));

        visual.update(|_, cx| {
            flow.update(cx, |flow, cx| {
                flow.move_cursor(CursorMove::Down, cx);
                flow.move_and_toggle_selection(CursorMove::Down, cx);
            })
        });
        visual.run_until_parked();
        assert_eq!(visual.update(|_, cx| flow.read(cx).selection().len()), 2);
        let padding = point(first.left() - px(2.0), first.center().y);
        let below = point(padding.x, second.center().y);
        visual.simulate_event(MouseDownEvent { position: padding, button: MouseButton::Left, click_count: 1, ..Default::default() });
        visual.simulate_event(MouseMoveEvent { position: below, pressed_button: Some(MouseButton::Left), ..Default::default() });
        visual.run_until_parked();
        assert!(!visual.update(|_, cx| cx.has_active_drag()), "padding must not drag an already selected entry");
        assert!(visual.update(|_, cx| flow.read(cx).marquee_active()));
        assert!(visual.update(|_, cx| flow.read(cx).selection().is_empty()), "a band outside the highlights must not select padded cells");
        visual.simulate_event(MouseUpEvent { position: below, button: MouseButton::Left, click_count: 1, ..Default::default() });
        visual.run_until_parked();
        assert!(received.borrow().is_empty());

        visual.simulate_event(MouseDownEvent { position: padding, button: MouseButton::Left, click_count: 1, ..Default::default() });
        visual.simulate_event(MouseMoveEvent { position: second.center(), pressed_button: Some(MouseButton::Left), ..Default::default() });
        visual.run_until_parked();
        visual.simulate_event(MouseUpEvent { position: second.center(), button: MouseButton::Left, click_count: 1, ..Default::default() });
        visual.run_until_parked();
        assert_eq!(visual.update(|_, cx| flow.read(cx).selection().iter().copied().collect::<Vec<_>>()), vec![0, 1]);
        assert!(received.borrow().is_empty());
    }

    #[gpui::test]
    fn dragging_inside_selects_individual_folders_then_moves_the_selection(cx: &mut TestAppContext) {
        use super::super::drag_drop::FolderDrop;
        use gpui::{MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, point};
        use std::cell::RefCell;

        cx.update(gpui_component::init);
        let row = |id| BrowseRow { id: BrowseLocation::Folder(Uuid::from_u128(id)), name: format!("Folder {id}"), path: None, book_count: 0, downloaded_book_count: 0 };
        let section = BrowseSection { parent: row(1), children: (2..6).map(row).collect(), books: Vec::new(), book_count: Some(0) };
        let received = Rc::new(RefCell::new(Vec::new()));
        let recorded = received.clone();
        let events: CardEvents = Rc::new(move |event, _, _| recorded.borrow_mut().push(event));
        let (flow, visual) = cx.add_window_view(|_, cx| {
            let layout = SectionLayout { rem_size: px(16.0), cover_text: CoverText::CoverOnly, mobile: false };
            let groups = mixed_groups("test", events, Uuid::nil(), &section, &BTreeMap::new(), components::NavigationIcon::Folder, layout, cx);
            Paginator::new("test", groups).select_on_drag()
        });
        visual.run_until_parked();
        let center = |index, visual: &mut gpui::VisualTestContext| visual.update(|_, cx| flow.read(cx).child_bounds(index).unwrap().center());
        let start = center(0, visual);
        let end = center(1, visual) + point(px(10.0), px(0.0));
        visual.simulate_event(MouseDownEvent { position: start, button: MouseButton::Left, click_count: 1, ..Default::default() });
        visual.simulate_event(MouseMoveEvent { position: end, pressed_button: Some(MouseButton::Left), ..Default::default() });
        visual.run_until_parked();
        assert!(!visual.update(|_, cx| cx.has_active_drag()), "unselected rows start a selection, not a folder drag");
        visual.simulate_event(MouseUpEvent { position: end, button: MouseButton::Left, click_count: 1, ..Default::default() });
        visual.run_until_parked();
        assert_eq!(visual.update(|_, cx| flow.read(cx).selection().iter().copied().collect::<Vec<_>>()), vec![0, 1]);
        assert!(received.borrow().is_empty(), "drawing the band must not open a folder");

        let start = center(0, visual);
        let destination = center(2, visual);
        visual.simulate_event(MouseDownEvent { position: start, button: MouseButton::Left, click_count: 1, ..Default::default() });
        visual.simulate_event(MouseMoveEvent { position: destination, pressed_button: Some(MouseButton::Left), ..Default::default() });
        visual.run_until_parked();
        assert!(visual.update(|_, cx| cx.has_active_drag()));
        visual.simulate_event(MouseUpEvent { position: destination, button: MouseButton::Left, click_count: 1, ..Default::default() });
        visual.run_until_parked();
        let received = received.borrow();
        let [CardEvent::DropOnto(destination, FolderDrop::Items(drag))] = received.as_slice() else { panic!("expected one drop of the selected folders") };
        assert_eq!(*destination, Uuid::from_u128(4));
        assert_eq!(drag.targets.folders, vec![Uuid::from_u128(2), Uuid::from_u128(3)]);
        assert_eq!(drag.targets.source, Some(Uuid::from_u128(1)));
    }

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

    #[gpui::test]
    fn containers_page_all_children_without_book_slots_on_every_platform(cx: &mut TestAppContext) {
        let row = |id| BrowseRow { id: BrowseLocation::Folder(Uuid::from_u128(id)), name: format!("Category {id}"), path: None, book_count: 20, downloaded_book_count: 0 };
        let section = BrowseSection { parent: row(1), children: (2..35).map(row).collect(), books: Vec::new(), book_count: Some(20) };
        let events: CardEvents = Rc::new(|_, _, _| {});
        cx.update(|cx| {
            for cover_text in [CoverText::Always, CoverText::CoverOnly] {
                for mobile in [false, true] {
                    let layout = SectionLayout { rem_size: px(16.0), cover_text, mobile };
                    let groups = mixed_groups("test", events.clone(), Uuid::nil(), &section, &BTreeMap::new(), components::NavigationIcon::Folder, layout, cx);
                    let flow = Paginator::new("test", groups);
                    for (index, child) in section.children.iter().enumerate() {
                        assert_eq!(flow.child_id(index), Some(&chip_id(child.id)));
                    }
                    assert!(flow.child_id(section.children.len()).is_none(), "container cards must not reserve slots for books");

                    let leaf = BrowseSection { children: Vec::new(), ..section.clone() };
                    let groups = mixed_groups("leaf", events.clone(), Uuid::nil(), &leaf, &BTreeMap::new(), components::NavigationIcon::Folder, layout, cx);
                    let flow = Paginator::new("leaf", groups);
                    assert_eq!(flow.child_id(0), Some(&SharedString::from("leaf-book-slot-0")));
                    assert_eq!(flow.child_id(19), Some(&SharedString::from("leaf-book-slot-19")));
                }

                for location in [BrowseLocation::Home(None), BrowseLocation::Author(Some(Uuid::from_u128(1))), BrowseLocation::CatalogAuthor(Some(Uuid::from_u128(1)))] {
                    for mobile in [false, true] {
                        let shelf = BrowseSection { parent: BrowseRow { id: location, ..section.parent.clone() }, children: Vec::new(), books: Vec::new(), book_count: Some(20) };
                        let layout = SectionLayout { rem_size: px(16.0), cover_text, mobile };
                        let groups = mixed_groups("test", events.clone(), Uuid::nil(), &shelf, &BTreeMap::new(), components::NavigationIcon::Folder, layout, cx);
                        let flow = Paginator::new("test", groups);
                        assert_eq!(flow.child_id(0), Some(&SharedString::from("test-book-slot-0")));
                        assert_eq!(flow.child_id(19), Some(&SharedString::from("test-book-slot-19")));
                    }
                }
            }
        });
    }
}

pub(super) struct SectionCardElement {
    card: Entity<SectionCard>,
    focused: bool,
    selected: bool,
    marquee: bool,
    header_drag: Option<DragPayload>,
    preview: bool,
}

impl gpui::View for SectionCardElement {
    fn entity_id(&self) -> Option<gpui::EntityId> {
        Some(self.card.entity_id())
    }
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        self.card.update(cx, |card, cx| card.render_card(self.focused, self.selected, self.marquee, self.header_drag, self.preview, window, cx))
    }
}

impl SectionCard {
    fn render_card(&mut self, focused: bool, selected: bool, marquee: bool, header_drag: Option<DragPayload>, preview: bool, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let layout = SectionLayout { rem_size: window.rem_size(), cover_text: crate::stores::current_cover_text(cx), ..self.layout };
        if !preview && self.layout != layout {
            // Recompute heights at the current font size without losing the page.
            self.layout = layout;
            let groups = mixed_groups(&self.id, self.events.clone(), self.library_id, &self.section, &self.loaded_books, self.child_icon, layout, cx);
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
            .child(arrow("previous-contents", "Previous entries", IconName::ArrowLeft, flow.has_previous() && self.request.is_none() && self.pending_turn.is_none(), false))
            .child(arrow("next-contents", "Next entries", IconName::ArrowRight, flow.has_next() && self.request.is_none() && self.pending_turn.is_none(), true));
        // A shelf of Home is a row of the page, not a place to go.
        let parent = &self.section.parent;
        let author = is_author_heading(parent.id);
        let opens = !matches!(parent.id, library_model::BrowseLocation::Home(_));
        let count = opens.then_some(parent.book_count);
        let pager = (!components::uses_mobile_navigation(window)).then(|| arrows.into_any_element());
        let heading = components::section_card_heading(format!("browse-card-title-{}", parent.id), parent.name.clone(), count, pager, opens, focused, marquee, theme).debug_selector(|| format!("browse-card-title-{}", parent.id));
        let band = components::section_card_band(self.band_index, heading).id(format!("{}-drag-header", self.id)).when(author, |band| band.px(px(components::BOOK_CARD_PADDING)));
        let band = match header_drag {
            Some((targets, preview)) => folder_drag_source(band, targets, preview),
            None => band,
        };
        let contents = if preview { Paginator::preview(self.flow.clone(), window, cx).into_any_element() } else { self.flow.clone().into_any_element() };
        components::section_card_frame(focused || selected, theme)
            .child(band)
            .child(
                components::section_card_content(theme).when(author, |content| content.px_0()).child(
                    components::section_card_body()
                        .id(format!("{}-body", self.id))
                        // Author books supply the inset themselves; cancel only the content border.
                        .mx(px(if author { -1.0 } else { -components::BOOK_CARD_PADDING }))
                        .when(container_section(&self.section), |body| body.py(px(components::BOOK_CARD_PADDING)))
                        .child(contents)
                        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(|_, _, cx| cx.stop_propagation()),
                ),
            )
            .into_any_element()
    }
}

impl Render for SectionCard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_card(false, false, false, None, false, window, cx)
    }
}

fn container_section(section: &BrowseSection) -> bool {
    !section_shows_books(section.parent.id, section.children.is_empty())
}

fn section_items(id: &str, section: &BrowseSection, loaded_books: &BTreeMap<usize, Entity<BookCardView>>) -> BrowseItems {
    let folder = section.parent.id.folder();
    let children = section.children.iter().map(|row| (chip_id(row.id), BrowseItem::Child { row: row.clone(), parent: folder }));
    let books = loaded_books.iter().map(|(index, book)| (SharedString::from(format!("{id}-book-slot-{index}")), BrowseItem::Book(book.clone())));
    Rc::new(children.chain(books).collect::<HashMap<_, _>>())
}

/// Every nested folder is a selectable item, so dragging a selection carries
/// its folders rather than a whole rendered column or the enclosing card.
fn container_groups(events: CardEvents, library_id: uuid::Uuid, section: &BrowseSection, child_icon: components::NavigationIcon, layout: SectionLayout) -> Vec<PaginatorGroup> {
    let folder = section.parent.id.folder();
    let items: BrowseItems = Rc::new(section.children.iter().map(|row| (chip_id(row.id), BrowseItem::Child { row: row.clone(), parent: folder })).collect());
    let children: Vec<_> = section
        .children
        .iter()
        .map(|subject| {
            let subject = subject.clone();
            let id = chip_id(subject.id);
            let item_id = id.clone();
            let items = items.clone();
            let events = events.clone();
            let activate = events.clone();
            let location = subject.id;
            let hit_bounds = Rc::new(Cell::new(Bounds::default()));
            let painted_bounds = hit_bounds.clone();
            PaginatorChild::new(move |state, _, cx| {
                let theme = components::browser_theme(cx);
                let selected = state.is_selected(&item_id);
                let targets = resolve_items(&state.target_ids(&item_id), &items);
                let row = components::section_card_subject(format!("{item_id}-row"), child_icon, subject.name.clone(), shown_count(&subject), theme)
                    .debug_selector(|| format!("browse-section-subject-{}", subject.id))
                    .when(selected, |row| row.bg(theme.accent_wash))
                    .children(download_mark(&subject, events.clone(), theme));
                let interaction = ChildInteraction { selected, marquee: state.marquee, draggable: selected && !state.preview, drag_preview: state.drag_preview(&item_id) };
                let row = child_interactions(row, &subject, events.clone(), library_id, folder, targets.clone(), interaction, child_icon, theme, cx)
                    .when(!selected, |row| row.on_mouse_down(gpui::MouseButton::Right, |_, _, cx| cx.stop_propagation()));
                let activate = events.clone();
                let menu = events.clone();
                let row = keyboard_item(row, move |window, cx| activate(CardEvent::OpenChild(location), window, cx), move |position, window, cx| menu(CardEvent::ItemsMenu(targets.clone(), position), window, cx), cx);
                let painted_bounds = painted_bounds.clone();
                div()
                    .size_full()
                    .px(px(components::BOOK_CARD_PADDING))
                    .child(row)
                    .on_children_prepainted(move |children, _, _| {
                        if let Some(bounds) = children.first() {
                            painted_bounds.set(*bounds);
                        }
                    })
                    .into_any_element()
            })
            .with_id(id)
            .with_hit_bounds(hit_bounds)
            .with_activate(move |window, cx| activate(CardEvent::OpenChild(location), window, cx))
        })
        .collect();
    let policy = PaginatorGroupPolicy { sizing: PaginatorSizing::rems(gpui::rems(components::SECTION_CARD_SUBJECT_ROW_HEIGHT_REM)), ..browse_section_item_policy(layout.cover_text, true) };
    vec![PaginatorGroup::new(policy, children)]
}

/// Containers page through child entries; Home and author cards page through books.
fn mixed_groups(
    id: &str, events: CardEvents, library_id: uuid::Uuid, section: &BrowseSection, loaded_books: &BTreeMap<usize, Entity<BookCardView>>, child_icon: components::NavigationIcon, layout: SectionLayout, cx: &mut App,
) -> Vec<PaginatorGroup> {
    // The folder this card shows, on a folder page. Its subfolders and books
    // are dragged out of it, and subfolders take drops.
    let folder = section.parent.id.folder();
    let mut children = Vec::new();
    let container = container_section(section);
    if container {
        return container_groups(events, library_id, section, child_icon, layout);
    }
    let items = section_items(id, section, loaded_books);
    let folder_card = folder.is_some();
    let rows_per_column = browse_section_subject_rows(layout.cover_text, layout.rem_size);
    let subject_columns = section.children.len().div_ceil(rows_per_column);
    for (column_index, column) in section.children.chunks(rows_per_column).enumerate() {
        let column = column.to_vec();
        let before_direct_books = !container && column_index + 1 == subject_columns && section.book_count.unwrap_or(section.books.len()) > 0;
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
                        let row = child_frame(row, subject, events.clone(), library_id, folder, vec![item.clone()], ChildInteraction::default(), child_icon, theme, cx)
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
    let placeholder = PaginatorChild::new(|_, _, _| div().size_full().into_any_element()).prevent_selection_start();
    let book_count = if container { 0 } else { section.book_count.unwrap_or(section.books.len()) };
    for index in 0..book_count {
        let slot = format!("{id}-book-slot-{index}");
        let Some(book) = loaded_books.get(&index) else {
            children.push(placeholder.clone().with_id(slot));
            continue;
        };
        let items = items.clone();
        let drag = Some(Rc::new(move |state: &PaginatorState, id: &SharedString, cx: &App| drag_payload(&resolve_items(&state.target_ids(id), &items), folder, library_id, child_icon, cx)) as BookDrag);
        children.push(super::layout::book_child(
            slot.into(),
            book.clone(),
            BookCardPresentation::Section,
            |state, id, _| BookCardInteraction { focused: state.is_focused(id), selected: state.is_selected(id), marquee: state.marquee },
            drag,
            cx,
        ));
    }
    let policy = if folder_card { browse_folder_item_policy(layout.cover_text, container) } else { browse_section_item_policy(layout.cover_text, container) };
    vec![PaginatorGroup::new(policy, children)]
}
