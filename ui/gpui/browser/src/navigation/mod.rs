//! The navigation rail.
//!
//! Universal chrome: it lists both library and universal routes, so it is owned
//! by the application root rather than by the selected-library root.

mod switcher;

use ui_components as components;
use gpui::prelude::*;
use gpui::{App, AppContext, Context, Entity, EventEmitter, IntoElement, Render, SharedString, Subscription, Window};
use gpui_component::{Icon, IconName};

use crate::library::LibraryState;
use crate::route::{LibraryRoute, Route, UniversalRoute};

use self::switcher::{LibrarySwitcherEvent, LibrarySwitcherView};

#[derive(Clone)]
pub(crate) enum NavEvent {
    Select(Route),
    SelectLibrary(String),
    RemoveLibrary(String),
    CreateLibrary,
    OpenLibrary,
    Quit,
}

pub(crate) struct NavigationView {
    route: Route,
    sync_label: SharedString,
    library_switcher: Entity<LibrarySwitcherView>,
    _library_switcher_subscription: Subscription,
}

impl EventEmitter<NavEvent> for NavigationView {}

impl NavigationView {
    pub(crate) fn new(route: Route, sync_label: SharedString, state: Entity<LibraryState>, cx: &mut Context<Self>) -> Self {
        let library_switcher = cx.new(|cx| LibrarySwitcherView::new(state, cx));
        let subscription = cx.subscribe(&library_switcher, |_: &mut Self, _, event: &LibrarySwitcherEvent, cx| match event {
            LibrarySwitcherEvent::Select(library_id) => cx.emit(NavEvent::SelectLibrary(library_id.clone())),
            LibrarySwitcherEvent::Remove(library_id) => cx.emit(NavEvent::RemoveLibrary(library_id.clone())),
            LibrarySwitcherEvent::Create => cx.emit(NavEvent::CreateLibrary),
            LibrarySwitcherEvent::OpenFolder => cx.emit(NavEvent::OpenLibrary),
        });
        Self { route, sync_label, library_switcher, _library_switcher_subscription: subscription }
    }

    pub(crate) fn set_route(&mut self, route: Route, cx: &mut Context<Self>) {
        if self.route == route {
            return;
        }
        self.route = route;
        cx.notify();
    }

    pub(crate) fn is_library_switcher_open(&self, cx: &App) -> bool {
        self.library_switcher.read(cx).is_open()
    }

    pub(crate) fn close_library_switcher(&mut self, cx: &mut Context<Self>) {
        self.library_switcher.update(cx, |switcher, cx| switcher.close(cx));
    }
}

impl Render for NavigationView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let emitter = cx.entity();
        let route = self.route;

        // (element id, icon, label, route)
        let primary = [
            ("section-home", components::home_navigation_icon(), SharedString::from("Home"), Route::Library(LibraryRoute::Home)),
            ("section-folders", Icon::from(IconName::BookOpen), "Library".into(), Route::Library(LibraryRoute::Folders)),
            ("section-authors", Icon::from(IconName::User), "Authors".into(), Route::Library(LibraryRoute::Authors)),
            ("section-search", Icon::from(IconName::Search), "Search".into(), Route::Library(LibraryRoute::Search)),
            ("section-store", Icon::from(IconName::Building2), "Store".into(), Route::Universal(UniversalRoute::Store)),
        ];
        let trailing = [
            ("section-sync", Icon::from(IconName::User), self.sync_label.clone(), Route::Universal(UniversalRoute::Sync)),
            ("section-settings", Icon::from(IconName::Settings), "Settings".into(), Route::Universal(UniversalRoute::Settings)),
        ];

        let mut sidebar = components::sidebar(theme).child(self.library_switcher.clone());
        for (id, icon, label, target) in primary {
            let emitter = emitter.clone();
            sidebar = sidebar.child(components::nav_rail_item(id, icon, label, route == target, theme).on_click(move |_, _, cx| {
                emitter.update(cx, |_, cx| cx.emit(NavEvent::Select(target)));
            }));
        }

        sidebar = sidebar.child(gpui::div().flex_1().min_h_0());

        if cfg!(feature = "kobo") {
            let emitter = emitter.clone();
            sidebar = sidebar.child(components::nav_rail_item("exit-kobo", IconName::WindowClose, "Exit to Kobo", false, theme).on_click(move |_, _, cx| {
                emitter.update(cx, |_, cx| cx.emit(NavEvent::Quit));
            }));
        }

        for (id, icon, label, target) in trailing {
            let emitter = emitter.clone();
            sidebar = sidebar.child(components::nav_rail_item(id, icon, label, route == target, theme).on_click(move |_, _, cx| {
                emitter.update(cx, |_, cx| cx.emit(NavEvent::Select(target)));
            }));
        }
        sidebar
    }
}
