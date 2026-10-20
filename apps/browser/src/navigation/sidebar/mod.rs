//! Responsive navigation sidebar and compact navigation bar.
//!
//! Renders whichever route the shell says is current; it owns no route of its
//! own, only the open/closed state of its own overlays.

mod switcher_overlay;

use gpui::prelude::*;
use gpui::{Context, Edges, Entity, FocusHandle, Pixels, SharedString};
use gpui_component::Icon;
use ui_components as components;

use super::{GlobalRoute, Libraries, LibraryRoute, Navigate, Route};

/// The rail's destinations, in the order it draws them. The keyboard walks this
/// same list, so a shortcut can never land somewhere other than the neighbour
/// the reader can see.
pub(crate) const RAIL_ROUTES: [Route; 7] = [
    Route::Library(LibraryRoute::Home),
    Route::Library(LibraryRoute::Books),
    Route::Library(LibraryRoute::Folders),
    Route::Library(LibraryRoute::Subjects),
    Route::Library(LibraryRoute::Authors),
    Route::Library(LibraryRoute::Catalog),
    Route::Global(GlobalRoute::Settings),
];


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
    library_menu: Option<(sync_common::LibraryId, Entity<gpui_component::menu::PopupMenu>, gpui::Subscription)>,
    library_name_dialog: Option<switcher_overlay::LibraryNameDialog>,
    navigate: Navigate,
    pub(super) switcher_cursor: SwitcherCursor,
    pub(super) switcher_focus: FocusHandle,
}

pub(super) fn route_definition(route: Route) -> (&'static str, Icon) {
    use components::NavigationIcon;
    let (label, icon) = match route {
        Route::Library(LibraryRoute::Home) => ("Home", NavigationIcon::House),
        Route::Library(LibraryRoute::Books) => ("Books", NavigationIcon::BookOpen),
        Route::Library(LibraryRoute::Authors) => ("Authors", NavigationIcon::User),
        Route::Library(LibraryRoute::Folders) => ("Folders", NavigationIcon::Folder),
        Route::Library(LibraryRoute::Subjects) => ("Subjects", NavigationIcon::Tags),
        Route::Library(LibraryRoute::Catalog) => ("Catalog", NavigationIcon::Building2),
        Route::Library(LibraryRoute::Trash) => ("Trash", NavigationIcon::Trash2),
        Route::Global(GlobalRoute::Settings) => ("Settings", NavigationIcon::Settings),
    };
    (label, components::navigation_icon(icon))
}

impl Sidebar {
    pub(crate) fn new(libraries: Entity<Libraries>, navigate: Navigate, cx: &mut Context<Self>) -> Self {
        Self { libraries, library_menu: None, library_name_dialog: None, switcher_open: false, navigate, switcher_cursor: SwitcherCursor::Add, switcher_focus: cx.focus_handle() }
    }

    /// Draws the rail or the bottom bar, whichever the window width calls for.
    /// `library_view` is the Library tab's view: the one last shown, which the
    /// tab returns to.
    pub(super) fn render_navigation(&self, emitter: Entity<Self>, route: Route, library_view: LibraryRoute, compact: bool, safe_area: Edges<Pixels>, cx: &gpui::App) -> gpui::AnyElement {
        if compact { self.render_bottom_navigation(emitter, route, library_view, safe_area, cx) } else { self.render_sidebar(emitter, route, cx) }
    }

    /// The compact tab bar: the rail's destinations. On a phone the library's
    /// browse views share one Library tab instead, switched from the menu in
    /// their heading (`library_view_heading`).
    fn render_bottom_navigation(&self, emitter: Entity<Self>, route: Route, library_view: LibraryRoute, safe_area: Edges<Pixels>, cx: &gpui::App) -> gpui::AnyElement {
        let theme = components::browser_theme(cx);
        let in_library_view = matches!(route, Route::Library(view) if super::LIBRARY_VIEWS.contains(&view));
        let tabs = if components::touch_ui() {
            vec![Route::Library(LibraryRoute::Home), Route::Library(library_view), Route::Library(LibraryRoute::Catalog), Route::Global(GlobalRoute::Settings)]
        } else {
            RAIL_ROUTES.to_vec()
        };
        let mut navigation = components::bottom_navigation(theme);
        for target in tabs {
            let (id, icon) = route_definition(target);
            let library_tab = components::touch_ui() && target == Route::Library(library_view);
            let (id, label, selected) = if library_tab { ("library", "Library", in_library_view) } else { (id, id, route == target) };
            let select = emitter.clone();
            navigation = navigation.child(components::bottom_navigation_item(format!("bottom-{id}"), icon, label, selected, theme).on_click(move |_, window, cx| {
                select.update(cx, |navigation, cx| (navigation.navigate)(target, window, cx));
            }));
        }
        let library_name = self.active_library_name(cx);
        let navigation = navigation.child(components::bottom_navigation_item("bottom-libraries", components::navigation_icon(components::NavigationIcon::Books), library_name, false, theme).on_click(move |_, window, cx| {
            emitter.update(cx, |navigation, cx| navigation.open_library_switcher(window, cx));
        }));
        gpui::div().w_full().flex_none().flex().flex_col().pl(safe_area.left).pr(safe_area.right).bg(theme.page_bg).child(navigation).child(gpui::div().w_full().h(safe_area.bottom).flex_none()).into_any_element()
    }

    fn render_sidebar(&self, emitter: Entity<Self>, route: Route, cx: &gpui::App) -> gpui::AnyElement {
        let theme = components::browser_theme(cx);
        // The switcher and destinations share the same square tile geometry.
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
