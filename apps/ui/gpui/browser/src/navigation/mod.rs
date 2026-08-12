//! The navigation rail.
//!
//! Universal chrome: it lists both library and universal routes, so it is owned
//! by the application root rather than by the selected-library root.

mod switcher;

use gpui::prelude::*;
use gpui::{App, AppContext, Context, Entity, EventEmitter, IntoElement, Render, SharedString, Subscription, Window};
use gpui_component::{Icon, IconName};
use ui_components as components;

use crate::library::LibraryState;
use crate::route::{LibraryRoute, Route, UniversalRoute};
use app_contract::{LibraryFeature, UniversalFeature};
use epub_svelte_lib::LibraryId;

use self::switcher::{LibrarySwitcherEvent, LibrarySwitcherView};

#[derive(Clone)]
pub(crate) enum NavEvent {
    Select(Route),
    SelectLibrary(LibraryId),
    RemoveLibrary(LibraryId),
    OpenLibrary,
    LibrarySwitcherVisibilityChanged,
    Quit,
}

pub(crate) struct NavigationView {
    route: Route,
    sync_label: SharedString,
    library_switcher: Entity<LibrarySwitcherView>,
    _library_switcher_subscription: Subscription,
}

/// Where each shared destination leads in this frontend. Now that the contract
/// distinguishes the two scopes, both mappings are exhaustive and neither
/// renames anything on the way through.
fn library_feature_route(feature: LibraryFeature) -> Route {
    Route::Library(match feature {
        LibraryFeature::Home => LibraryRoute::Home,
        LibraryFeature::Folders => LibraryRoute::Folders,
        LibraryFeature::Subjects => LibraryRoute::Subjects,
        LibraryFeature::Authors => LibraryRoute::Authors,
        LibraryFeature::Trash => LibraryRoute::Trash,
    })
}

fn universal_feature_route(feature: UniversalFeature) -> Route {
    Route::Universal(match feature {
        UniversalFeature::Store => UniversalRoute::Store,
        UniversalFeature::Account => UniversalRoute::Sync,
        UniversalFeature::Settings => UniversalRoute::Settings,
    })
}

fn library_feature_icon(feature: LibraryFeature) -> Icon {
    match feature {
        LibraryFeature::Home => components::home_navigation_icon(),
        LibraryFeature::Folders => Icon::from(IconName::BookOpen),
        LibraryFeature::Subjects => Icon::from(IconName::Asterisk),
        LibraryFeature::Authors => Icon::from(IconName::User),
        LibraryFeature::Trash => Icon::empty().path("icons/bokheim-trash.svg"),
    }
}

fn universal_feature_icon(feature: UniversalFeature) -> Icon {
    match feature {
        UniversalFeature::Store => Icon::from(IconName::Building2),
        // `CircleUser` rather than `User`: the avatar-in-a-circle reads as
        // "your account", and leaves the bare figure to mean "a person",
        // which is what the Authors destination is about.
        UniversalFeature::Account => Icon::from(IconName::CircleUser),
        UniversalFeature::Settings => Icon::from(IconName::Settings),
    }
}

fn library_feature_element_id(feature: LibraryFeature) -> &'static str {
    match feature {
        LibraryFeature::Home => "section-home",
        LibraryFeature::Folders => "section-folders",
        LibraryFeature::Subjects => "section-subjects",
        LibraryFeature::Authors => "section-authors",
        LibraryFeature::Trash => "section-trash",
    }
}

fn universal_feature_element_id(feature: UniversalFeature) -> &'static str {
    match feature {
        UniversalFeature::Store => "section-store",
        UniversalFeature::Account => "section-sync",
        UniversalFeature::Settings => "section-settings",
    }
}

impl EventEmitter<NavEvent> for NavigationView {}

impl NavigationView {
    pub(crate) fn new(route: Route, sync_label: SharedString, state: Entity<LibraryState>, cx: &mut Context<Self>) -> Self {
        let library_switcher = cx.new(|cx| LibrarySwitcherView::new(state, cx));
        let subscription = cx.subscribe(&library_switcher, |_: &mut Self, _, event: &LibrarySwitcherEvent, cx| match event {
            LibrarySwitcherEvent::Select(library_id) => cx.emit(NavEvent::SelectLibrary(library_id.clone())),
            LibrarySwitcherEvent::Remove(library_id) => cx.emit(NavEvent::RemoveLibrary(library_id.clone())),
            LibrarySwitcherEvent::OpenFolder => cx.emit(NavEvent::OpenLibrary),
            LibrarySwitcherEvent::VisibilityChanged => cx.emit(NavEvent::LibrarySwitcherVisibilityChanged),
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

    pub(crate) fn library_switcher_overlay(&self, cx: &App) -> Option<gpui::AnyElement> {
        let switcher = self.library_switcher.clone();
        switcher.read(cx).is_open().then(|| switcher.read(cx).render_overlay(switcher.clone(), cx))
    }
}

impl Render for NavigationView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let emitter = cx.entity();
        let route = self.route;

        // Both groups come from the shared navigation contract, so a
        // destination cannot appear here without every frontend accounting for
        // it. Only the icon and element id stay local: the contract does not
        // know about GPUI icons.
        let library = app_contract::LIBRARY_FEATURES.map(|feature| (library_feature_element_id(feature), library_feature_icon(feature), SharedString::from(feature.label()), library_feature_route(feature)));
        let universal = app_contract::UNIVERSAL_FEATURES.map(|feature| {
            // The account destination is the one label the contract does not
            // own: it changes with the configured sync service.
            let label = match feature {
                UniversalFeature::Account => self.sync_label.clone(),
                _ => SharedString::from(feature.label()),
            };
            (universal_feature_element_id(feature), universal_feature_icon(feature), label, universal_feature_route(feature))
        });

        let sidebar_width = components::nav_rail_width(f32::from(window.rem_size()));
        let mut sidebar = components::sidebar(sidebar_width, theme).child(self.library_switcher.clone());
        for (id, icon, label, target) in library {
            let emitter = emitter.clone();
            sidebar = sidebar.child(components::nav_rail_item(id, icon, label, route == target, theme).on_click(move |_, _, cx| {
                emitter.update(cx, |_, cx| cx.emit(NavEvent::Select(target)));
            }));
        }

        // The scope boundary. Everything above belongs to the selected library;
        // everything below outlives it.
        sidebar = sidebar.child(gpui::div().flex_1().min_h_0()).child(components::nav_rail_divider(theme));

        if cfg!(feature = "kobo") {
            let emitter = emitter.clone();
            sidebar = sidebar.child(components::nav_rail_item("exit-kobo", IconName::WindowClose, "Exit to Kobo", false, theme).on_click(move |_, _, cx| {
                emitter.update(cx, |_, cx| cx.emit(NavEvent::Quit));
            }));
        }

        for (id, icon, label, target) in universal {
            let emitter = emitter.clone();
            sidebar = sidebar.child(components::nav_rail_item(id, icon, label, route == target, theme).on_click(move |_, _, cx| {
                emitter.update(cx, |_, cx| cx.emit(NavEvent::Select(target)));
            }));
        }
        sidebar
    }
}
