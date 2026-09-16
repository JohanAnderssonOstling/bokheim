//! Responsive navigation sidebar and compact navigation bar.
//!
//! Renders whichever route the shell says is current; it owns no route of its
//! own, only the open/closed state of its own overlays.

mod switcher_overlay;

use gpui::prelude::*;
use gpui::{Context, Edges, Entity, FocusHandle, MouseButton, Pixels, SharedString};
use gpui_component::{Icon, IconName};
use ui_components as components;

use super::{GlobalRoute, Libraries, LibraryRoute, Navigate, Route};

/// The rail's destinations, in the order it draws them. The keyboard walks this
/// same list, so a shortcut can never land somewhere other than the neighbour
/// the reader can see.
pub(crate) const RAIL_ROUTES: [Route; 6] =
    [Route::Library(LibraryRoute::Home), Route::Library(LibraryRoute::Folders), Route::Library(LibraryRoute::Subjects), Route::Library(LibraryRoute::Authors), Route::Library(LibraryRoute::Trash), Route::Global(GlobalRoute::Settings)];

const PRIMARY_ROUTES: [Route; 3] = [Route::Library(LibraryRoute::Home), Route::Library(LibraryRoute::Folders), Route::Library(LibraryRoute::Subjects)];
const SECONDARY_ROUTES: [Route; 3] = [Route::Library(LibraryRoute::Authors), Route::Library(LibraryRoute::Trash), Route::Global(GlobalRoute::Settings)];

/// Where the keyboard is inside the open library switcher.
///
/// A grid rather than a list: rows are the add button and then one per library,
/// and a library row has two cells. Tab order alone could not express this —
/// `down` has to skip a row's menu button and land on the next library.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SwitcherCursor {
    Add,
    Library { index: usize, menu: bool },
}

pub(crate) struct Sidebar {
    libraries: Entity<Libraries>,
    switcher_open: bool,
    more_open: bool,
    library_menu: Option<(sync_common::LibraryId, Entity<gpui_component::menu::PopupMenu>, gpui::Subscription)>,
    library_name_dialog: Option<switcher_overlay::LibraryNameDialog>,
    navigate: Navigate,
    pub(super) switcher_cursor: SwitcherCursor,
    pub(super) switcher_focus: FocusHandle,
}

fn route_definition(route: Route) -> (&'static str, Icon) {
    use components::NavigationIcon;
    let (label, icon) = match route {
        Route::Library(LibraryRoute::Home) => ("Home", NavigationIcon::House),
        Route::Library(LibraryRoute::Authors) => ("Authors", NavigationIcon::User),
        Route::Library(LibraryRoute::Folders) => ("Folders", NavigationIcon::Folder),
        Route::Library(LibraryRoute::Subjects) => ("Subjects", NavigationIcon::Tags),
        Route::Library(LibraryRoute::Trash) => ("Trash", NavigationIcon::Trash2),
        Route::Global(GlobalRoute::Settings) => ("Settings", NavigationIcon::Settings),
    };
    (label, components::navigation_icon(icon))
}

impl Sidebar {
    pub(crate) fn new(libraries: Entity<Libraries>, navigate: Navigate, cx: &mut Context<Self>) -> Self {
        Self { libraries, library_menu: None, library_name_dialog: None, switcher_open: false, more_open: false, navigate, switcher_cursor: SwitcherCursor::Add, switcher_focus: cx.focus_handle() }
    }

    /// Draws the rail or the bottom bar, whichever the window width calls for.
    pub(super) fn render_navigation(&self, emitter: Entity<Self>, route: Route, compact: bool, safe_area: Edges<Pixels>, cx: &gpui::App) -> gpui::AnyElement {
        if compact { self.render_bottom_navigation(emitter, route, safe_area, cx) } else { self.render_sidebar(emitter, route, cx) }
    }

    pub(crate) fn is_more_open(&self) -> bool {
        self.more_open
    }

    pub(crate) fn close_more(&mut self, cx: &mut Context<Self>) {
        if !self.more_open {
            return;
        }
        self.more_open = false;
        cx.notify();
    }

    pub(crate) fn more_overlay(&self, emitter: Entity<Self>, route: Route, safe_area: Edges<Pixels>, cx: &gpui::App) -> Option<gpui::AnyElement> {
        self.more_open.then(|| self.render_more_overlay(emitter, route, safe_area, cx))
    }

    fn render_more_overlay(&self, emitter: Entity<Self>, route: Route, safe_area: Edges<Pixels>, cx: &gpui::App) -> gpui::AnyElement {
        let theme = components::browser_theme(cx);
        let item_count = SECONDARY_ROUTES.len() + 1 + usize::from(cfg!(feature = "kobo"));
        let open_libraries = emitter.clone();
        let mut items = gpui::div().w_full().flex().flex_col().gap(gpui::px(components::SPACE_XS)).child(components::more_navigation_header(theme)).child(
            components::more_navigation_item("more-libraries", components::navigation_icon(components::NavigationIcon::BookOpen), self.active_library_name(cx), false, theme).on_click(move |_, window, cx| {
                open_libraries.update(cx, |navigation, cx| {
                    navigation.more_open = false;
                    navigation.open_library_switcher(window, cx);
                });
            }),
        );
        for target in SECONDARY_ROUTES {
            let (id, icon) = route_definition(target);
            let label = SharedString::from(id);
            let select = emitter.clone();
            items = items.child(components::more_navigation_item(format!("more-{id}"), icon, label, route == target, theme).on_click(move |_, window, cx| {
                select.update(cx, |navigation, cx| {
                    navigation.more_open = false;
                    cx.notify();
                    (navigation.navigate)(target, window, cx);
                });
            }));
        }
        let dismiss = emitter;
        gpui::deferred(
            components::more_navigation_overlay()
                .pt(safe_area.top)
                .pr(safe_area.right)
                .pb(safe_area.bottom)
                .pl(safe_area.left)
                .child(components::modal_scrim("more-navigation-scrim", theme).on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()).on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    dismiss.update(cx, |navigation, cx| navigation.close_more(cx));
                }))
                .child(components::navigation_modal_list(item_count, theme).w_full().max_w(gpui::px(420.0)).p(gpui::px(components::SPACE_MD)).child(items)),
        )
        .with_priority(1)
        .into_any_element()
    }

    fn render_bottom_navigation(&self, emitter: Entity<Self>, route: Route, safe_area: Edges<Pixels>, cx: &gpui::App) -> gpui::AnyElement {
        let theme = components::browser_theme(cx);
        let mut navigation = components::bottom_navigation(theme);
        for target in PRIMARY_ROUTES {
            let (id, icon) = route_definition(target);
            let select = emitter.clone();
            navigation = navigation.child(components::bottom_navigation_item(format!("bottom-{id}"), icon, id, route == target, theme).on_click(move |_, window, cx| {
                select.update(cx, |navigation, cx| {
                    navigation.more_open = false;
                    (navigation.navigate)(target, window, cx);
                });
            }));
        }
        let more_selected = self.more_open || SECONDARY_ROUTES.contains(&route);
        let navigation = navigation.child(components::bottom_navigation_item("bottom-more", IconName::Ellipsis, "More", more_selected, theme).on_click(move |_, _, cx| {
            emitter.update(cx, |navigation, cx| {
                navigation.more_open = !navigation.more_open;
                cx.notify();
            });
        }));
        gpui::div().w_full().flex_none().flex().flex_col().pl(safe_area.left).pr(safe_area.right).bg(theme.page_bg).child(navigation).child(gpui::div().w_full().h(safe_area.bottom).flex_none()).into_any_element()
    }

    fn render_sidebar(&self, emitter: Entity<Self>, route: Route, cx: &gpui::App) -> gpui::AnyElement {
        let theme = components::browser_theme(cx);
        // The destinations carry the rail's edge, not the rail: the switcher
        // spans the topbar's height, so the edge starts on the topbar baseline.
        let mut destinations = components::nav_rail_destinations(theme);
        for target in RAIL_ROUTES {
            let (id, icon) = route_definition(target);
            let label = SharedString::from(id);
            let select = emitter.clone();
            destinations = destinations.child(components::nav_rail_item(id, icon, label, route == target, theme).on_click(move |_, window, cx| {
                select.update(cx, |navigation, cx| {
                    (navigation.navigate)(target, window, cx);
                });
            }));
        }
        let sidebar = components::sidebar(theme).child(self.render_library_switcher_trigger(emitter.clone(), cx)).child(destinations);
        gpui::div().w(gpui::rems(components::NAV_RAIL_SIZE_REM)).h_full().flex_none().child(sidebar).into_any_element()
    }
}
