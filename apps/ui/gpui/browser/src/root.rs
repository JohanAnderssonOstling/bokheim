//! Window-level owner and router.
//!
//! Owns application navigation, the selected library, and the current route
//! root. Nested navigation and book detail stay inside page components.

use epub_svelte_lib::LibraryId;
use epub_svelte_lib::app::LibraryEntry;
use epub_svelte_lib::library::LibraryBackend;
use gpui::prelude::*;
use gpui::{AnyView, App, AppContext, Context, Entity, FocusHandle, Focusable, IntoElement, Render, SharedString, Subscription, Task, Window, px};
use gpui_component::notification::Notification;
use gpui_component::{Root, WindowExt};
use ui_components as components;

use crate::library::{AuthorsPage, HomePage, LibraryContext, LibraryState, TrashPage, new_folder_browse_page, new_store_browse_page, new_subject_browse_page};
use crate::model::BrowserSession;
use crate::navigation::{NavEvent, NavigationView};
use crate::services::AppServices;
use crate::stores::Preferences;
use crate::universal::settings::SettingsPage;
use crate::universal::sync::SyncPage;
use crate::{DismissSettings, Route, ShowSettings};

#[cfg(feature = "audiobooks")]
struct ActiveAudiobook {
    dock: Entity<audiobook_player::AudiobookDock>,
    _subscription: Subscription,
}

pub struct BrowserRoot {
    services: AppServices,
    preferences: Entity<Preferences>,
    library_state: Entity<LibraryState>,
    library: LibraryContext,
    route: Route,
    page: AnyView,
    navigation: Entity<NavigationView>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
    _library_list_updates: Task<()>,
    #[cfg(feature = "audiobooks")]
    audiobook: Option<ActiveAudiobook>,
    #[cfg(feature = "kobo")]
    kobo_auto_rotate: bool,
}

impl BrowserRoot {
    pub fn new(libraries: Vec<LibraryEntry>, services: AppServices, #[cfg(feature = "kobo")] kobo_auto_rotate: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let session = services.backend.browser_session().unwrap_or_else(|error| {
            log::warn!("failed to restore browser session: {error}");
            BrowserSession::default()
        });
        let preferences = cx.new(|cx| Preferences::new(services.clone(), cx));
        let active_library_id =
            session.active_library_id.filter(|id| libraries.iter().any(|entry| entry.library_id() == id)).or_else(|| libraries.first().map(|entry| *entry.library_id())).expect("the application backend must provide at least one library");
        let library = resolve_library(&services, &active_library_id).expect("the application backend must provide an openable library");
        if let Err(error) = services.backend.save_active_library_id(Some(active_library_id)) {
            log::warn!("failed to persist restored library selection: {error}");
        }
        let library = LibraryContext::new(services.clone(), preferences.clone(), library);
        let library_state = cx.new(|_| LibraryState::new(libraries, active_library_id));
        let sync_label = services.contracts.account.navigation_label.clone();
        let restored_route = Route::Folders;
        let navigation = cx.new(|cx| NavigationView::new(restored_route, sync_label.into(), services.backend.clone(), library_state.clone(), cx));
        let subscriptions = vec![cx.subscribe_in(&navigation, window, |root: &mut Self, _, event, window, cx| match event {
            NavEvent::Select(route) => root.navigate(*route, window, cx),
            NavEvent::SelectLibrary(library_id) => root.select_library(*library_id, window, cx),
        })];

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

        let library_list_changes = services.backend.library_list_changes();
        let list_backend = services.backend.clone();
        let library_list_updates = cx.spawn(async move |root, cx| {
            while library_list_changes.recv().await.is_ok() {
                let load = list_backend.spawn(|core| async move { core.libraries().map_err(|error| error.to_string()) });
                let result = load.await.map_err(|error| error.to_string()).and_then(|result| result);
                let _ = root.update_in(cx, |root, window, cx| match result {
                    Ok(libraries) => root.update_library_list(libraries, window, cx),
                    Err(error) => root.notify_error(error.into(), window, cx),
                });
            }
        });

        let reconciliation = services.backend.spawn(|core| async move { core.reconcile_libraries().await.map(|state| state.libraries).map_err(|error| error.to_string()) });
        cx.spawn(async move |root, cx| match reconciliation.await {
            Ok(Ok(libraries)) => {
                let _ = root.update_in(cx, |root, window, cx| root.update_library_list(libraries, window, cx));
            }
            Ok(Err(error)) => log::warn!("failed to reconcile libraries: {error}"),
            Err(error) => log::warn!("library reconciliation task failed: {error}"),
        })
        .detach();

        let page = cx.new(|cx| new_folder_browse_page(library.clone(), cx)).into();
        let root = Self {
            services,
            preferences,
            library_state,
            library,
            route: restored_route,
            page,
            navigation,
            focus,
            _subscriptions: subscriptions,
            _library_list_updates: library_list_updates,
            #[cfg(feature = "audiobooks")]
            audiobook: None,
            #[cfg(feature = "kobo")]
            kobo_auto_rotate,
        };
        root.start_library_scanning(window, cx);
        root
    }

    #[cfg(feature = "audiobooks")]
    pub fn show_audiobook(&mut self, source: audiobook_player::AudiobookSource, initial_position: std::time::Duration, save_position: audiobook_player::SavePlaybackPosition, cx: &mut Context<Self>) {
        let dock = audiobook_player::open_library_dock(source, initial_position, save_position, cx);
        let subscription = cx.subscribe(&dock, |root: &mut Self, _, _: &audiobook_player::AudiobookDockEvent, cx| {
            root.audiobook = None;
            cx.notify();
        });
        self.audiobook = Some(ActiveAudiobook { dock, _subscription: subscription });
        cx.notify();
    }

    fn navigate(&mut self, route: Route, window: &mut Window, cx: &mut Context<Self>) {
        if self.route == route {
            return;
        }
        self.route = route;
        self.navigation.update(cx, |navigation, cx| navigation.set_route(route, cx));

        self.page = match route {
            Route::Home => cx.new(|cx| HomePage::new(self.library.clone(), cx)).into(),
            Route::Authors => cx.new(|cx| AuthorsPage::new(self.library.clone(), window, cx)).into(),
            Route::Folders => cx.new(|cx| new_folder_browse_page(self.library.clone(), cx)).into(),
            Route::Subjects => cx.new(|cx| new_subject_browse_page(self.library.clone(), cx)).into(),
            Route::Trash => cx.new(|cx| TrashPage::new(self.library.clone(), cx)).into(),
            Route::Store => cx.new(|cx| new_store_browse_page(self.services.clone(), self.library.library.clone(), cx)).into(),
            Route::Account => cx.new(|cx| SyncPage::new(self.services.clone(), self.library_state.clone(), window, cx)).into(),
            Route::Settings => cx
                .new(|cx| {
                    SettingsPage::new(
                        self.services.clone(),
                        self.preferences.clone(),
                        #[cfg(feature = "kobo")]
                        self.kobo_auto_rotate,
                        cx,
                    )
                })
                .into(),
        };
        cx.notify();
    }

    fn update_library_list(&mut self, libraries: Vec<LibraryEntry>, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(error) = self.replace_libraries(libraries, None, window, cx) {
            self.notify_error(error.into(), window, cx);
        }
    }

    fn select_library(&mut self, library_id: LibraryId, window: &mut Window, cx: &mut Context<Self>) {
        if self.library_state.read(cx).library_id() == &library_id {
            return;
        }
        let result = self.services.backend.libraries().map_err(|error| error.to_string()).and_then(|libraries| self.replace_libraries(libraries, Some(library_id), window, cx));
        if let Err(error) = result {
            self.notify_error(error.into(), window, cx);
        }
    }

    fn replace_libraries(&mut self, libraries: Vec<LibraryEntry>, preferred_library_id: Option<LibraryId>, _window: &mut Window, cx: &mut Context<Self>) -> Result<(), String> {
        let libraries = if libraries.is_empty() {
            self.services.backend.ensure_default_library().map_err(|error| error.to_string())?;
            self.services.backend.libraries().map_err(|error| error.to_string())?
        } else {
            libraries
        };
        let library_ids = libraries.iter().map(|entry| *entry.library_id()).collect::<Vec<_>>();
        self.services.backend.retain_browser_session_libraries(&library_ids).map_err(|error| error.to_string())?;
        let current_id = *self.library_state.read(cx).library_id();
        let target_id = preferred_library_id
            .filter(|id| libraries.iter().any(|entry| entry.library_id() == id))
            .or_else(|| libraries.iter().any(|entry| entry.library_id() == &current_id).then_some(current_id))
            .unwrap_or_else(|| *libraries.first().expect("an empty library list was repaired above").library_id());

        if target_id == current_id {
            self.library_state.update(cx, |state, cx| state.set_entries(libraries, cx));
            return Ok(());
        }

        let library = resolve_library(&self.services, &target_id)?;
        self.services.backend.save_active_library_id(Some(target_id)).map_err(|error| error.to_string())?;
        self.library_state.update(cx, |state, cx| state.replace(libraries, target_id, cx));
        self.library = LibraryContext::new(self.services.clone(), self.preferences.clone(), library);
        self.route = Route::Folders;
        self.navigation.update(cx, |navigation, cx| navigation.set_route(Route::Folders, cx));
        self.page = cx.new(|cx| new_folder_browse_page(self.library.clone(), cx)).into();
        cx.notify();
        Ok(())
    }

    fn start_library_scanning(&self, window: &mut Window, cx: &mut Context<Self>) {
        let backend = self.services.backend.clone();
        cx.on_next_frame(window, move |_, _, _| {
            if let Err(error) = backend.enable_library_scanning() {
                log::error!("failed to start library scanning after the first frame: {error}");
            }
        });
    }

    /// Reports a failed action.
    ///
    /// Errors do not auto-hide: each one is an action the user explicitly
    /// asked for that did not happen, and there are few enough of them that a
    /// missed message costs more than a lingering one.
    fn notify_error(&self, error: SharedString, window: &mut Window, cx: &mut App) {
        window.push_notification(Notification::error(error).id1::<Self>("browser-error").autohide(false), cx);
    }

    fn on_dismiss(&mut self, _: &DismissSettings, window: &mut Window, cx: &mut Context<Self>) {
        if self.navigation.read(cx).is_library_switcher_open() {
            self.navigation.update(cx, |navigation, cx| navigation.close_library_switcher(cx));
            return;
        }
        if matches!(self.route, Route::Store | Route::Account | Route::Settings) {
            self.navigate(Route::Folders, window, cx);
        }
    }
}

fn resolve_library(services: &AppServices, library_id: &LibraryId) -> Result<LibraryBackend, String> {
    services.backend.library(library_id).map_err(|error| {
        log::error!("failed to select library {library_id}: {error}");
        format!("Could not open library: {error}")
    })
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
            .on_action(cx.listener(Self::on_dismiss))
            .on_action(cx.listener(|root, _: &ShowSettings, window, cx| root.navigate(Route::Settings, window, cx)));

        let content = components::library_section_content().child(self.page.clone());
        #[cfg(feature = "audiobooks")]
        let content = content.when_some(self.audiobook.as_ref().map(|active| active.dock.clone()), |content, dock| content.child(dock));

        let page = page.child(components::browser_body().child(self.navigation.clone()).child(content));

        page.children(self.navigation.read(cx).library_switcher_overlay(self.navigation.clone(), cx)).children(Root::render_notification_layer(window, cx)).children(Root::render_dialog_layer(window, cx)).into_any_element()
    }
}
