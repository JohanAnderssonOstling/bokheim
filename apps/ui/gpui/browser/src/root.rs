//! Window-level owner and router.
//!
//! Connects navigation/setup intents to [`LibraryHost`], which controls
//! selected-library state and lifetime. The active screen determines the route.

use epub_svelte_lib::app::LibraryEntry;
use epub_svelte_lib::platform::scanner::LibraryEvent;
use gpui::prelude::*;
use gpui::{px, App, AppContext, Context, Entity, FocusHandle, Focusable, IntoElement, PromptLevel, Render, SharedString, Subscription, Window};
use gpui_component::notification::Notification;
use gpui_component::{IconName, Root, WindowExt};
use ui_components as components;

use crate::library::{LibraryHost, LibraryHostEvent, LibrarySetup, LibrarySetupEvent};
use crate::model::BrowserSession;
use crate::navigation::{NavEvent, NavigationView};
use crate::route::{LibraryRoute, Route, UniversalRoute};
use crate::services::{AppServices, SaveBrowserState};
use crate::stores::{interface_font, LibraryAvailabilityChanged, LibraryDisplayChanged, Preferences};
use crate::universal::settings::SettingsPage;
use crate::universal::store::StorePage;
use crate::universal::sync::{SyncPage, SyncPageEvent};
use crate::{DismissSettings, FocusSearch, ShowAuthors, ShowFolders, ShowHome, ShowSettings, ShowStore, ShowSubjects, ShowSync};

pub struct BrowserRoot {
    services: AppServices,
    save_state: SaveBrowserState,
    preferences: Entity<Preferences>,
    screen: BrowserScreen,
    library_host: Entity<LibraryHost>,
    library_setup: Entity<LibrarySetup>,
    navigation: Entity<NavigationView>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
    #[cfg(feature = "kobo")]
    kobo_auto_rotate: bool,
}

enum UniversalPage {
    Store(Entity<StorePage>),
    Sync { page: Entity<SyncPage>, _subscription: Subscription },
    Settings(Entity<SettingsPage>),
}

enum BrowserScreen {
    Library,
    Universal(UniversalPage),
}

impl UniversalPage {
    fn route(&self) -> UniversalRoute {
        match self {
            Self::Store(_) => UniversalRoute::Store,
            Self::Sync { .. } => UniversalRoute::Sync,
            Self::Settings(_) => UniversalRoute::Settings,
        }
    }

    fn element(&self) -> gpui::AnyElement {
        match self {
            Self::Store(page) => page.clone().into_any_element(),
            Self::Sync { page, .. } => page.clone().into_any_element(),
            Self::Settings(page) => page.clone().into_any_element(),
        }
    }
}

impl BrowserRoot {
    pub fn new(libraries: Vec<LibraryEntry>, session: BrowserSession, services: AppServices, #[cfg(feature = "kobo")] kobo_auto_rotate: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let save_state = services.save_state.clone();
        let preferences = cx.new(|cx| Preferences::new(services.clone(), cx));
        let library_host = cx.new(|cx| LibraryHost::new(libraries, &session, services.clone(), window, cx));
        let library_state = library_host.read(cx).state();
        let sync_label = services.contracts.account.navigation_label.clone();
        // The host has already resolved which library is actually selected, so
        // ask it rather than re-deriving the route from the session.
        let restored_route = library_host.read(cx).route(cx);
        let navigation = cx.new(|cx| NavigationView::new(Route::Library(restored_route), sync_label.into(), library_state.clone(), cx));
        let library_setup = cx.new(|_| LibrarySetup::new(services.clone()));
        let subscriptions = vec![
            cx.subscribe(&navigation, |root: &mut Self, _, event, cx| root.handle_navigation(event, cx)),
            cx.subscribe(&library_host, |root: &mut Self, _, event, cx| root.handle_host(event, cx)),
            cx.subscribe(&library_setup, |root: &mut Self, _, event, cx| root.handle_setup(event, cx)),
            cx.subscribe(&preferences, |root: &mut Self, _, _: &LibraryAvailabilityChanged, cx| {
                root.library_host.update(cx, |host, cx| host.refresh_active(cx));
            }),
            // A display change needs no re-query, only a different presentation
            // of cards the grids already hold.
            cx.subscribe(&preferences, |root: &mut Self, _, event: &LibraryDisplayChanged, cx| {
                root.library_host.update(cx, |host, cx| host.set_library_view(event.0, cx));
            }),
        ];

        // The platform reports light/dark and reports it *changing*; without
        // observing the change, "follow system" would only be honoured at
        // startup.
        let appearance_preferences = preferences.clone();
        window
            .observe_window_appearance(move |_, cx| {
                let browsing = appearance_preferences.read(cx).browsing();
                crate::stores::preferences::apply_browsing_appearance(browsing, cx);
                cx.refresh_windows();
            })
            .detach();

        let focus = cx.focus_handle();
        focus.focus(window, cx);

        let reconciliation = services.backend.spawn(|core| async move { core.reconcile_library_catalogue().await.map(|catalogue| catalogue.libraries).map_err(|error| error.to_string()) });
        cx.spawn(async move |root, cx| {
            let Ok(Ok(libraries)) = reconciliation.await else { return };
            let _ = root.update_in(cx, |root, window, cx| root.apply_catalogue(libraries, None, None, true, window, cx));
        })
        .detach();

        Self {
            services,
            save_state,
            preferences,
            screen: BrowserScreen::Library,
            library_host,
            library_setup,
            navigation,
            focus,
            _subscriptions: subscriptions,
            #[cfg(feature = "kobo")]
            kobo_auto_rotate,
        }
    }

    fn handle_navigation(&mut self, event: &NavEvent, cx: &mut Context<Self>) {
        match event {
            NavEvent::Select(route) => self.navigate_later(*route, cx),
            NavEvent::SelectLibrary(library_id) => self.activate_library_later(library_id.clone(), cx),
            NavEvent::RemoveLibrary(library_id) => self.remove_library_later(library_id.clone(), cx),
            NavEvent::OpenLibrary => self.library_setup.update(cx, |setup, cx| setup.choose_library_folders(cx)),
            NavEvent::LibrarySwitcherVisibilityChanged => cx.notify(),
            NavEvent::Quit => cx.quit(),
        }
    }

    fn handle_host(&mut self, event: &LibraryHostEvent, cx: &mut Context<Self>) {
        match event {
            LibraryHostEvent::SessionChanged => self.persist(cx),
        }
    }

    fn handle_setup(&mut self, event: &LibrarySetupEvent, cx: &mut Context<Self>) {
        match event {
            LibrarySetupEvent::CatalogueReady { libraries, activate_library_id, warning } => {
                self.apply_catalogue_later(libraries.clone(), activate_library_id.clone(), warning.clone(), false, cx);
            }
            LibrarySetupEvent::Error(error) => self.notify_error_later(error.clone(), cx),
        }
    }

    fn handle_sync(&mut self, event: &SyncPageEvent, cx: &mut Context<Self>) {
        match event {
            SyncPageEvent::CatalogueReconciled(libraries) => self.apply_catalogue_later(libraries.clone(), None, None, true, cx),
        }
    }

    pub fn session(&self, cx: &App) -> BrowserSession {
        self.session_for(cx)
    }

    fn session_for(&self, cx: &App) -> BrowserSession {
        self.library_host.read(cx).session(cx)
    }

    fn persist(&self, cx: &mut App) {
        (self.save_state)(self.session_for(cx), None, cx);
    }

    fn navigate_later(&mut self, route: Route, cx: &mut Context<Self>) {
        cx.spawn(async move |root, cx| {
            let _ = root.update_in(cx, |root, window, cx| root.navigate(route, window, cx));
        })
        .detach();
    }

    fn navigate(&mut self, route: Route, window: &mut Window, cx: &mut Context<Self>) {
        if self.current_route(cx) == route {
            return;
        }
        if let Some(library_route) = route.library() {
            self.screen = BrowserScreen::Library;
            self.library_host.update(cx, |host, cx| host.set_route(library_route, window, cx));
        } else if let Some(universal_route) = route.universal() {
            let Some(page) = self.open_universal_page(universal_route, window, cx) else { return };
            self.screen = BrowserScreen::Universal(page);
        }
        self.navigation.update(cx, |navigation, cx| navigation.set_route(route, cx));
        cx.notify();
        self.persist(cx);
    }

    fn open_universal_page(&mut self, route: UniversalRoute, window: &mut Window, cx: &mut Context<Self>) -> Option<UniversalPage> {
        let state = self.library_host.read(cx).state();
        Some(match route {
            UniversalRoute::Store => {
                let library = self.library_host.read(cx).library(cx)?;
                let page = cx.new(|cx| StorePage::new(self.services.clone(), self.preferences.clone(), library, window, cx));
                page.update(cx, |page, cx| page.refresh(cx));
                UniversalPage::Store(page)
            }
            UniversalRoute::Sync => {
                let page = cx.new(|cx| SyncPage::new(self.services.clone(), state, window, cx));
                let subscription = cx.subscribe(&page, |root: &mut Self, _, event, cx| root.handle_sync(event, cx));
                page.update(cx, |page, cx| page.refresh(cx));
                UniversalPage::Sync { page, _subscription: subscription }
            }
            UniversalRoute::Settings => {
                let page = cx.new(|cx| {
                    SettingsPage::new(
                        self.services.clone(),
                        self.preferences.clone(),
                        #[cfg(feature = "kobo")]
                        self.kobo_auto_rotate,
                        cx,
                    )
                });
                UniversalPage::Settings(page)
            }
        })
    }

    fn activate_library_later(&mut self, library_id: epub_svelte_lib::LibraryId, cx: &mut Context<Self>) {
        cx.spawn(async move |root, cx| {
            let _ = root.update_in(cx, |root, window, cx| root.activate_library(library_id, window, cx));
        })
        .detach();
    }

    fn remove_library_later(&mut self, library_id: epub_svelte_lib::LibraryId, cx: &mut Context<Self>) {
        cx.spawn(async move |root, cx| {
            let Ok(answer) = root.update_in(cx, |_, window, cx| {
                window.prompt(
                    PromptLevel::Warning,
                    "Delete this library?",
                    Some("Local database and state are removed immediately. Server deletion happens now when possible, or automatically after the next sign-in. Local files and stored server blobs are kept."),
                    &["Delete library", "Cancel"],
                    cx,
                )
            }) else {
                return;
            };
            if answer.await.ok() != Some(0) {
                return;
            }
            let Ok(backend) = root.update(cx, |root, _| root.services.backend.clone()) else {
                return;
            };
            let deletion = backend.spawn(move |core| async move { core.delete_library(&library_id).await.map_err(|error| error.to_string()) });
            let result = deletion.await.unwrap_or_else(|error| Err(error.to_string()));
            if let Err(error) = result {
                let _ = root.update_in(cx, |root, window, cx| root.notify_error(error.into(), window, cx));
                return;
            }
            let _ = root.update_in(cx, |root, window, cx| root.finish_library_removal(window, cx));
        })
        .detach();
    }

    fn finish_library_removal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let result = self.library_host.update(cx, |host, cx| host.refresh_after_library_removal(window, cx));
        match result {
            Ok(changed) => {
                if changed {
                    self.show_selected_library(cx);
                }
            }
            Err(error) => self.notify_error(error.into(), window, cx),
        }
    }

    fn activate_library(&mut self, library_id: epub_svelte_lib::LibraryId, window: &mut Window, cx: &mut Context<Self>) {
        match self.library_host.update(cx, |host, cx| host.activate_library(library_id, window, cx)) {
            Ok(changed) => {
                if changed {
                    self.show_selected_library(cx);
                }
            }
            Err(error) => self.notify_error(error.into(), window, cx),
        }
    }

    fn apply_catalogue_later(&mut self, libraries: Vec<LibraryEntry>, activate_library_id: Option<epub_svelte_lib::LibraryId>, warning: Option<SharedString>, refresh_active: bool, cx: &mut Context<Self>) {
        cx.spawn(async move |root, cx| {
            let _ = root.update_in(cx, |root, window, cx| root.apply_catalogue(libraries, activate_library_id, warning, refresh_active, window, cx));
        })
        .detach();
    }

    fn apply_catalogue(&mut self, libraries: Vec<LibraryEntry>, activate_library_id: Option<epub_svelte_lib::LibraryId>, warning: Option<SharedString>, refresh_active: bool, window: &mut Window, cx: &mut Context<Self>) {
        match self.library_host.update(cx, |host, cx| host.replace_catalogue(libraries, activate_library_id, window, cx)) {
            Ok(changed) => {
                if changed {
                    self.show_selected_library(cx);
                }
                if refresh_active {
                    self.library_host.update(cx, |host, cx| host.refresh_active(cx));
                }
                if let Some(warning) = warning {
                    self.notify_warning(warning, window, cx);
                }
            }
            Err(error) => self.notify_error(error.into(), window, cx),
        }
    }

    fn show_selected_library(&mut self, cx: &mut Context<Self>) {
        let route = self.library_host.read(cx).route(cx);
        self.screen = BrowserScreen::Library;
        self.navigation.update(cx, |navigation, cx| navigation.set_route(Route::Library(route), cx));
        cx.notify();
        self.persist(cx);
    }

    /// Reports a failed action.
    ///
    /// Errors do not auto-hide: each one is an action the user explicitly
    /// asked for that did not happen, and there are few enough of them that a
    /// missed message costs more than a lingering one.
    fn notify_error(&self, error: SharedString, window: &mut Window, cx: &mut App) {
        window.push_notification(Notification::error(error).id1::<Self>("browser-error").autohide(false), cx);
    }

    /// Reports something that succeeded with a caveat.
    ///
    /// Distinct from [`Self::notify_error`] so a warning is not dressed as a
    /// failure. These do auto-hide: the action worked.
    fn notify_warning(&self, warning: SharedString, window: &mut Window, cx: &mut App) {
        window.push_notification(Notification::warning(warning).id1::<Self>("browser-warning"), cx);
    }

    /// Reports a failure raised where no window is in scope.
    ///
    /// Subscription callbacks receive only a context, so the notification is
    /// pushed on the next update that has a window — the same deferral the
    /// navigation and catalogue paths already use.
    fn notify_error_later(&self, error: SharedString, cx: &mut Context<Self>) {
        cx.spawn(async move |root, cx| {
            let _ = root.update_in(cx, |root, window, cx| root.notify_error(error, window, cx));
        })
        .detach();
    }

    pub fn handle_library_event(&mut self, library_id: &epub_svelte_lib::LibraryId, event: &LibraryEvent, cx: &mut Context<Self>) {
        if matches!(event, LibraryEvent::CatalogueChanged) {
            let catalogue = self.services.backend.spawn(|core| async move { core.libraries().map_err(|error| error.to_string()) });
            cx.spawn(async move |root, cx| {
                let Ok(Ok(libraries)) = catalogue.await else { return };
                let _ = root.update_in(cx, |root, window, cx| root.apply_catalogue(libraries, None, None, true, window, cx));
            })
            .detach();
            return;
        }
        if matches!(event, LibraryEvent::TransferStatusChanged) {
            if let BrowserScreen::Universal(UniversalPage::Sync { page, .. }) = &self.screen {
                page.update(cx, |page, cx| page.schedule_refresh(cx));
            }
            return;
        }
        self.library_host.update(cx, |host, cx| host.handle_library_event(library_id, event, cx));
    }

    /// The search box belongs to the folder and subject pages now. From
    /// anywhere else this lands on Folders, whose box at the root searches the
    /// whole library — the scope the Search page used to have.
    fn on_focus_search(&mut self, _: &FocusSearch, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.current_route(cx), Route::Library(LibraryRoute::Folders | LibraryRoute::Subjects)) {
            self.navigate(Route::Library(LibraryRoute::Folders), window, cx);
        }
        self.library_host.update(cx, |host, cx| host.focus_search(window, cx));
    }

    fn on_show_home(&mut self, _: &ShowHome, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(Route::Library(LibraryRoute::Home), window, cx);
    }
    fn on_show_authors(&mut self, _: &ShowAuthors, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(Route::Library(LibraryRoute::Authors), window, cx);
    }
    fn on_show_folders(&mut self, _: &ShowFolders, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(Route::Library(LibraryRoute::Folders), window, cx);
    }
    fn on_show_subjects(&mut self, _: &ShowSubjects, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(Route::Library(LibraryRoute::Subjects), window, cx);
    }
    fn on_show_store(&mut self, _: &ShowStore, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(Route::Universal(UniversalRoute::Store), window, cx);
    }
    fn on_show_sync(&mut self, _: &ShowSync, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(Route::Universal(UniversalRoute::Sync), window, cx);
    }
    fn on_show_settings(&mut self, _: &ShowSettings, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(Route::Universal(UniversalRoute::Settings), window, cx);
    }

    fn on_dismiss(&mut self, _: &DismissSettings, window: &mut Window, cx: &mut Context<Self>) {
        if self.navigation.read(cx).is_library_switcher_open(cx) {
            self.navigation.update(cx, |navigation, cx| navigation.close_library_switcher(cx));
            return;
        }
        if matches!(self.screen, BrowserScreen::Universal(_)) {
            let route = self.library_host.read(cx).route(cx);
            self.navigate(Route::Library(route), window, cx);
        }
    }

    fn current_route(&self, cx: &App) -> Route {
        match &self.screen {
            BrowserScreen::Library => Route::Library(self.library_host.read(cx).route(cx)),
            BrowserScreen::Universal(page) => Route::Universal(page.route()),
        }
    }

    fn render_first_run(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = components::browser_theme(cx);
        let states = &self.services.contracts.browsing_states;
        let root = cx.entity();
        let create = root.clone();
        let open = root.clone();
        let settings = root.clone();
        let sync = root;
        let buttons = components::action_row()
            .child(components::primary_action_button("first-run-create", states.create_library_label.clone(), IconName::Plus, false).on_click(move |_, _, cx| {
                create.update(cx, |root, cx| root.library_setup.update(cx, |setup, cx| setup.show_create_dialog(cx)));
            }))
            .child(components::primary_action_button("first-run-open", "Open folder…", IconName::FolderOpen, false).on_click(move |_, _, cx| {
                open.update(cx, |root, cx| root.library_setup.update(cx, |setup, cx| setup.choose_library_folders(cx)));
            }));
        let secondary = components::action_row()
            .child(components::secondary_action_button("first-run-settings", "Settings", IconName::Settings, false).on_click(move |_, _, cx| {
                settings.update(cx, |root, cx| root.navigate_later(Route::Universal(UniversalRoute::Settings), cx));
            }))
            .child(components::secondary_action_button("first-run-sync", "Sign in", IconName::User, false).on_click(move |_, _, cx| {
                sync.update(cx, |root, cx| root.navigate_later(Route::Universal(UniversalRoute::Sync), cx));
            }));
        gpui::div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(18.0))
            .bg(theme.page_bg)
            .child(gpui::div().text_size(gpui::rems(28.0 / 16.0)).font_family(interface_font(cx)).font_weight(gpui::FontWeight::BOLD).text_color(theme.text).child(states.no_libraries_title.clone()))
            .child(gpui::div().max_w(px(420.0)).text_align(gpui::TextAlign::Center).text_color(theme.text_muted).child(states.no_libraries_description.clone()))
            .child(buttons)
            .child(secondary)
            .into_any_element()
    }
}

impl Focusable for BrowserRoot {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for BrowserRoot {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let preferences = self.preferences.read(cx);
        let ui_font_size = preferences.ui_font_size_px();
        // Components express their type scale in rems. Keep the window's rem
        // basis synchronized with the preference so explicit component text
        // sizes and their responsive containers change together.
        window.set_rem_size(px(ui_font_size as f32));
        let page = components::app_page(preferences.interface_font_name(), ui_font_size, theme)
            .key_context("Browser")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::on_focus_search))
            .on_action(cx.listener(Self::on_show_home))
            .on_action(cx.listener(Self::on_show_authors))
            .on_action(cx.listener(Self::on_show_folders))
            .on_action(cx.listener(Self::on_show_subjects))
            .on_action(cx.listener(Self::on_show_store))
            .on_action(cx.listener(Self::on_show_sync))
            .on_action(cx.listener(Self::on_show_settings))
            .on_action(cx.listener(Self::on_dismiss));

        if !self.library_host.read(cx).is_configured(cx) {
            return page.child(self.render_first_run(cx)).children(Root::render_notification_layer(window, cx)).children(Root::render_dialog_layer(window, cx)).into_any_element();
        }

        let body = match &self.screen {
            BrowserScreen::Universal(page) => page.element(),
            BrowserScreen::Library => self.library_host.clone().into_any_element(),
        };

        let content = components::library_section_content();

        page.child(components::browser_body().child(self.navigation.clone()).child(content.child(body)))
            .children(self.navigation.read(cx).library_switcher_overlay(cx))
            .children(Root::render_notification_layer(window, cx))
            .children(Root::render_dialog_layer(window, cx))
            .into_any_element()
    }
}
