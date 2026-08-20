//! The navigation rail.
//!
//! Application chrome for the single flat route list.

mod switcher;

use gpui::prelude::*;
use gpui::{App, Context, Entity, EventEmitter, IntoElement, Render, SharedString, Subscription, Window};
use gpui_component::{Icon, IconName};
use ui_components as components;

use crate::Route;
use crate::library::{LibraryState, LibraryStateChanged};
use epub_svelte_lib::LibraryId;
use epub_svelte_lib::app::AppBackend;

#[derive(Clone)]
pub(crate) enum NavEvent {
    Select(Route),
    SelectLibrary(LibraryId),
    LibraryRemoved,
    OpenLibrary,
    LibrarySwitcherVisibilityChanged,
    Quit,
}

pub(crate) struct NavigationView {
    route: Route,
    sync_label: SharedString,
    backend: AppBackend,
    library_state: Entity<LibraryState>,
    switcher_open: bool,
    _library_state_subscription: Subscription,
}

fn route_definition(route: Route) -> (&'static str, Icon) {
    use app_contract::NavigationIcon;
    let (label, icon) = match route {
        Route::Home => ("Home", NavigationIcon::House),
        Route::Authors => ("Authors", NavigationIcon::User),
        Route::Folders => ("Folders", NavigationIcon::Folder),
        Route::Subjects => ("Subjects", NavigationIcon::Tags),
        Route::Trash => ("Trash", NavigationIcon::Trash2),
        Route::Store => ("Store", NavigationIcon::Building2),
        Route::Account => ("Account", NavigationIcon::CircleUser),
        Route::Settings => ("Settings", NavigationIcon::Settings),
    };
    (label, components::navigation_icon(icon))
}

impl EventEmitter<NavEvent> for NavigationView {}

impl NavigationView {
    pub(crate) fn new(route: Route, sync_label: SharedString, backend: AppBackend, state: Entity<LibraryState>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.subscribe(&state, |navigation: &mut Self, _, _: &LibraryStateChanged, cx| {
            cx.notify();
            if navigation.switcher_open {
                cx.emit(NavEvent::LibrarySwitcherVisibilityChanged);
            }
        });
        Self { route, sync_label, backend, library_state: state, switcher_open: false, _library_state_subscription: subscription }
    }

    pub(crate) fn set_route(&mut self, route: Route, cx: &mut Context<Self>) {
        if self.route == route {
            return;
        }
        self.route = route;
        cx.notify();
    }

    pub(crate) fn is_library_switcher_open(&self) -> bool {
        self.switcher_open
    }

    pub(crate) fn close_library_switcher(&mut self, cx: &mut Context<Self>) {
        if !self.switcher_open {
            return;
        }
        self.switcher_open = false;
        cx.notify();
        cx.emit(NavEvent::LibrarySwitcherVisibilityChanged);
    }

    pub(crate) fn library_switcher_overlay(&self, emitter: Entity<Self>, cx: &App) -> Option<gpui::AnyElement> {
        self.switcher_open.then(|| self.render_library_switcher_overlay(emitter, cx))
    }
}

impl Render for NavigationView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let emitter = cx.entity();
        let route = self.route;

        let sidebar_width = components::nav_rail_width(f32::from(window.rem_size()));
        let mut sidebar = components::sidebar(sidebar_width, theme).child(self.render_library_switcher_trigger(emitter.clone(), cx));
        for target in [Route::Home, Route::Folders, Route::Subjects, Route::Authors, Route::Store, Route::Account, Route::Settings] {
            if target == Route::Store {
                sidebar = sidebar.child(gpui::div().flex_1().min_h_0()).child(components::nav_rail_divider(theme));
                if cfg!(feature = "kobo") {
                    let emitter = emitter.clone();
                    sidebar = sidebar.child(components::nav_rail_item("exit-kobo", IconName::WindowClose, "Exit to Kobo", false, theme).on_click(move |_, _, cx| {
                        emitter.update(cx, |_, cx| cx.emit(NavEvent::Quit));
                    }));
                }
            }
            let (id, icon) = route_definition(target);
            let label = if target == Route::Account { self.sync_label.clone() } else { SharedString::from(id) };
            let emitter = emitter.clone();
            sidebar = sidebar.child(components::nav_rail_item(id, icon, label, route == target, theme).on_click(move |_, _, cx| {
                emitter.update(cx, |_, cx| cx.emit(NavEvent::Select(target)));
            }));
        }
        sidebar
    }
}
