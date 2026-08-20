//! Window-level owner and router.
//!
//! Owns application navigation, the selected library, and the current flat
//! library page. Nested navigation and book detail stay inside each page.

use epub_svelte_lib::LibraryId;
use epub_svelte_lib::app::LibraryEntry;
use epub_svelte_lib::library::LibraryBackend;
use epub_svelte_lib::platform::scanner::LibraryEvent;
use gpui::prelude::*;
use gpui::{AnyView, App, AppContext, Context, Entity, FocusHandle, Focusable, IntoElement, Render, SharedString, Subscription, Window, px};
use gpui_component::notification::Notification;
use gpui_component::{Root, WindowExt};
use ui_components as components;

use crate::library::{AuthorsPage, FoldersPage, HomePage, LibraryContext, LibrarySetup, LibrarySetupEvent, LibraryState, SubjectsPage, TrashPage};
use crate::model::BrowserSession;
use crate::navigation::{NavEvent, NavigationView};
use crate::services::AppServices;
use crate::stores::{LibraryDisplayChanged, Preferences};
use crate::universal::settings::SettingsPage;
use crate::universal::store::StorePage;
use crate::universal::sync::{SyncPage, SyncPageEvent};
use crate::{DismissSettings, Route};

#[cfg(feature = "audiobooks")]
struct ActiveAudiobook {
    dock: Entity<audiobook_player::AudiobookDock>,
    _subscription: Subscription,
}

pub struct BrowserRoot {
    services: AppServices,
    preferences: Entity<Preferences>,
    library_state: Entity<LibraryState>,
    library: Option<LibraryBackend>,
    route: Route,
    page: Option<AnyView>,
    page_subscription: Option<Subscription>,
    library_setup: Entity<LibrarySetup>,
    navigation: Entity<NavigationView>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
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
        let requested_id = session.active_library_id.filter(|id| libraries.iter().any(|entry| entry.library_id() == id)).or_else(|| libraries.first().map(|entry| *entry.library_id()));
        let selected = requested_id.and_then(|library_id| match resolve_library(&services, &library_id) {
            Ok(library) => Some((library_id, library)),
            Err(error) => {
                log::error!("failed to restore library selection: {error}");
                None
            }
        });
        let active_library_id = selected.as_ref().map(|(library_id, _)| library_id.clone());
        let library = selected.map(|(_, library)| library);
        let library_state = cx.new(|_| LibraryState::new(libraries, active_library_id));
        let sync_label = services.contracts.account.navigation_label.clone();
        let restored_route = Route::Folders;
        let navigation = cx.new(|cx| NavigationView::new(restored_route, sync_label.into(), services.backend.clone(), library_state.clone(), cx));
        let library_setup = cx.new(|_| LibrarySetup::new(services.clone()));
        let subscriptions = vec![
            cx.subscribe_in(&navigation, window, |root: &mut Self, _, event, window, cx| root.handle_navigation(event, window, cx)),
            cx.subscribe_in(&library_setup, window, |root: &mut Self, _, event, window, cx| root.handle_setup(event, window, cx)),
            // The current page is cheap to recreate and reads the new display
            // preference while rebuilding its grids.
            cx.subscribe_in(&preferences, window, |root: &mut Self, _, _: &LibraryDisplayChanged, window, cx| {
                if matches!(root.route, Route::Home | Route::Authors | Route::Folders | Route::Subjects | Route::Trash) {
                    root.show_page(root.route, window, cx);
                }
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

        let reconciliation = services.backend.spawn(|core| async move { core.reconcile_libraries().await.map(|state| state.libraries).map_err(|error| error.to_string()) });
        cx.spawn(async move |root, cx| {
            let Ok(Ok(libraries)) = reconciliation.await else { return };
            let _ = root.update_in(cx, |root, window, cx| root.update_library_list(libraries, window, cx));
        })
        .detach();

        let mut root = Self {
            services,
            preferences,
            library_state,
            library,
            route: restored_route,
            page: None,
            page_subscription: None,
            library_setup,
            navigation,
            focus,
            _subscriptions: subscriptions,
            #[cfg(feature = "audiobooks")]
            audiobook: None,
            #[cfg(feature = "kobo")]
            kobo_auto_rotate,
        };
        root.show_page(restored_route, window, cx);
        if root.library.is_some() {
            root.start_library_scanning(window, cx);
        }
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

    fn handle_navigation(&mut self, event: &NavEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            NavEvent::Select(route) => self.navigate(*route, window, cx),
            NavEvent::SelectLibrary(library_id) => self.activate_library_later(library_id.clone(), cx),
            NavEvent::LibraryRemoved => {
                let result = self.services.backend.libraries().map_err(|error| error.to_string()).and_then(|libraries| self.replace_libraries(libraries, None, window, cx));
                match result {
                    Ok(changed) => {
                        if changed {
                            self.show_selected_library(cx);
                        }
                    }
                    Err(error) => self.notify_error(error.into(), window, cx),
                }
            }
            NavEvent::OpenLibrary => self.library_setup.update(cx, |setup, cx| setup.choose_library_folders(cx)),
            NavEvent::LibrarySwitcherVisibilityChanged => cx.notify(),
            NavEvent::Quit => cx.quit(),
        }
    }

    fn handle_setup(&mut self, event: &LibrarySetupEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            LibrarySetupEvent::LibrariesAdded { libraries, activate_library_id, warning } => {
                self.add_libraries(libraries.clone(), activate_library_id.clone(), warning.clone(), window, cx);
            }
            LibrarySetupEvent::OpenSettings => self.navigate(Route::Settings, window, cx),
            LibrarySetupEvent::OpenAccount => self.navigate(Route::Account, window, cx),
            LibrarySetupEvent::Error(error) => self.notify_error_later(error.clone(), cx),
        }
    }

    fn handle_sync(&mut self, event: &SyncPageEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            SyncPageEvent::LibraryListUpdated(libraries) => self.update_library_list(libraries.clone(), window, cx),
        }
    }

    fn navigate(&mut self, route: Route, window: &mut Window, cx: &mut Context<Self>) {
        if self.route == route {
            return;
        }
        self.show_page(route, window, cx);
        self.navigation.update(cx, |navigation, cx| navigation.set_route(route, cx));
        cx.notify();
    }

    fn activate_library_later(&mut self, library_id: epub_svelte_lib::LibraryId, cx: &mut Context<Self>) {
        cx.spawn(async move |root, cx| {
            let _ = root.update_in(cx, |root, window, cx| match root.activate_library(library_id, window, cx) {
                Ok(changed) => {
                    if changed {
                        root.show_selected_library(cx);
                    }
                }
                Err(error) => root.notify_error(error.into(), window, cx),
            });
        })
        .detach();
    }

    fn add_libraries(&mut self, libraries: Vec<LibraryEntry>, activate_library_id: Option<epub_svelte_lib::LibraryId>, warning: Option<SharedString>, window: &mut Window, cx: &mut Context<Self>) {
        match self.replace_libraries(libraries, activate_library_id, window, cx) {
            Ok(changed) => {
                if changed {
                    self.show_selected_library(cx);
                }
                if let Some(warning) = warning {
                    self.notify_warning(warning, window, cx);
                }
            }
            Err(error) => self.notify_error(error.into(), window, cx),
        }
    }

    fn update_library_list(&mut self, libraries: Vec<LibraryEntry>, window: &mut Window, cx: &mut Context<Self>) {
        match self.replace_libraries(libraries, None, window, cx) {
            Ok(true) => self.show_selected_library(cx),
            Ok(false) => self.show_page(self.route, window, cx),
            Err(error) => self.notify_error(error.into(), window, cx),
        }
    }

    fn activate_library(&mut self, library_id: LibraryId, window: &mut Window, cx: &mut Context<Self>) -> Result<bool, String> {
        if self.library_state.read(cx).library_id() == Some(&library_id) {
            return Ok(false);
        }
        let library = resolve_library(&self.services, &library_id)?;
        let entries = self.library_state.read(cx).entries().to_vec();
        self.services.backend.save_active_library_id(Some(library_id.clone())).map_err(|error| error.to_string())?;
        self.library_state.update(cx, |state, cx| state.replace(entries, Some(library_id.clone()), cx));
        self.library = Some(library);
        self.show_page(Route::Folders, window, cx);
        self.start_library_scanning(window, cx);
        Ok(true)
    }

    fn replace_libraries(&mut self, libraries: Vec<LibraryEntry>, preferred_library_id: Option<LibraryId>, window: &mut Window, cx: &mut Context<Self>) -> Result<bool, String> {
        let library_ids = libraries.iter().map(|entry| entry.library_id().clone()).collect::<Vec<_>>();
        self.services.backend.retain_browser_session_libraries(&library_ids).map_err(|error| error.to_string())?;
        let current_id = self.library_state.read(cx).library_id().cloned();
        let target_id = preferred_library_id
            .filter(|id| libraries.iter().any(|entry| entry.library_id() == id))
            .or_else(|| current_id.clone().filter(|id| libraries.iter().any(|entry| entry.library_id() == id)))
            .or_else(|| libraries.first().map(|entry| entry.library_id().clone()));

        if target_id == current_id && current_id.is_some() {
            self.library_state.update(cx, |state, cx| state.set_entries(libraries, cx));
            return Ok(false);
        }

        let Some(target_id) = target_id else {
            self.services.backend.save_active_library_id(None).map_err(|error| error.to_string())?;
            self.library_state.update(cx, |state, cx| state.replace(libraries, None, cx));
            self.library = None;
            self.page = None;
            self.page_subscription = None;
            cx.notify();
            return Ok(current_id.is_some());
        };
        let library = resolve_library(&self.services, &target_id)?;
        self.services.backend.save_active_library_id(Some(target_id.clone())).map_err(|error| error.to_string())?;
        self.library_state.update(cx, |state, cx| state.replace(libraries, Some(target_id.clone()), cx));
        self.library = Some(library);
        self.show_page(Route::Folders, window, cx);
        self.start_library_scanning(window, cx);
        Ok(true)
    }

    fn start_library_scanning(&self, window: &mut Window, cx: &mut Context<Self>) {
        let backend = self.services.backend.clone();
        cx.on_next_frame(window, move |_, _, _| {
            if let Err(error) = backend.enable_library_scanning() {
                log::error!("failed to start library scanning after the first frame: {error}");
            }
        });
    }

    /// Creates the selected flat page. Only library pages receive a
    /// `LibraryContext`; each page owns its nested behavior and persistence.
    fn show_page(&mut self, route: Route, window: &mut Window, cx: &mut Context<Self>) {
        self.route = route;
        self.page_subscription = None;

        self.page = Some(match route {
            Route::Home => {
                let Some(context) = self.library.clone().map(|library| LibraryContext::new(self.services.clone(), library)) else {
                    self.page = None;
                    return;
                };
                cx.new(|cx| HomePage::new(context, cx)).into()
            }
            Route::Authors => {
                let Some(context) = self.library.clone().map(|library| LibraryContext::new(self.services.clone(), library)) else {
                    self.page = None;
                    return;
                };
                cx.new(|cx| AuthorsPage::new(context, window, cx)).into()
            }
            Route::Folders => {
                let Some(context) = self.library.clone().map(|library| LibraryContext::new(self.services.clone(), library)) else {
                    self.page = None;
                    return;
                };
                let place = self.services.backend.browser_session().map(|session| session.place(context.library.id())).unwrap_or_default();
                cx.new(|cx| FoldersPage::new(context, place.folder_path, window, cx)).into()
            }
            Route::Subjects => {
                let Some(context) = self.library.clone().map(|library| LibraryContext::new(self.services.clone(), library)) else {
                    self.page = None;
                    return;
                };
                let place = self.services.backend.browser_session().map(|session| session.place(context.library.id())).unwrap_or_default();
                cx.new(|cx| SubjectsPage::new(context, place.subject_path, window, cx)).into()
            }
            Route::Trash => {
                let Some(context) = self.library.clone().map(|library| LibraryContext::new(self.services.clone(), library)) else {
                    self.page = None;
                    return;
                };
                cx.new(|cx| TrashPage::new(context, cx)).into()
            }
            Route::Store => {
                let Some(library) = self.library.clone() else {
                    self.page = None;
                    return;
                };
                cx.new(|cx| StorePage::new(self.services.clone(), self.preferences.clone(), library, window, cx)).into()
            }
            Route::Account => {
                let page = cx.new(|cx| SyncPage::new(self.services.clone(), self.library_state.clone(), window, cx));
                self.page_subscription = Some(cx.subscribe_in(&page, window, |root, _, event, window, cx| root.handle_sync(event, window, cx)));
                page.into()
            }
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
        });
        cx.notify();
    }

    fn show_selected_library(&mut self, cx: &mut Context<Self>) {
        self.navigation.update(cx, |navigation, cx| navigation.set_route(self.route, cx));
        cx.notify();
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
    /// navigation and library-list paths already use.
    fn notify_error_later(&self, error: SharedString, cx: &mut Context<Self>) {
        cx.spawn(async move |root, cx| {
            let _ = root.update_in(cx, |root, window, cx| root.notify_error(error, window, cx));
        })
        .detach();
    }

    pub fn handle_library_event(&mut self, library_id: &epub_svelte_lib::LibraryId, event: &LibraryEvent, cx: &mut Context<Self>) {
        if matches!(event, LibraryEvent::LibraryListChanged) {
            let library_list = self.services.backend.spawn(|core| async move { core.libraries().map_err(|error| error.to_string()) });
            cx.spawn(async move |root, cx| {
                let Ok(Ok(libraries)) = library_list.await else { return };
                let _ = root.update_in(cx, |root, window, cx| root.update_library_list(libraries, window, cx));
            })
            .detach();
            return;
        }
        if matches!(event, LibraryEvent::TransferStatusChanged) {
            if let Some(page) = self.page.clone().and_then(|page| page.downcast::<SyncPage>().ok()) {
                page.update(cx, |page, cx| page.schedule_refresh(cx));
            }
            return;
        }
        if self.library_state.read(cx).library_id() != Some(library_id) {
            return;
        }
        match event {
            LibraryEvent::ThumbnailGenerated { content_hash } => crate::library::invalidate_covers(library_id.clone(), std::slice::from_ref(content_hash), cx),
            LibraryEvent::ThumbnailBatchProcessed { content_hashes, .. } => crate::library::invalidate_covers(library_id.clone(), content_hashes, cx),
            LibraryEvent::DownloadStatusChanged { .. } | LibraryEvent::TransferStatusChanged | LibraryEvent::LibraryListChanged => {}
            _ if matches!(self.route, Route::Home | Route::Authors | Route::Folders | Route::Subjects | Route::Trash) => {
                cx.spawn(async move |root, cx| {
                    let _ = root.update_in(cx, |root, window, cx| root.show_page(root.route, window, cx));
                })
                .detach();
            }
            _ => {}
        }
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
        let page = components::app_page(preferences.interface_font_name(), ui_font_size, theme).key_context("Browser").track_focus(&self.focus).on_action(cx.listener(Self::on_dismiss));

        let library_configured = self.library_state.read(cx).is_configured();
        let application_page = matches!(self.route, Route::Store | Route::Account | Route::Settings);
        let body = if library_configured || application_page { self.page.clone().map(IntoElement::into_any_element).unwrap_or_else(|| gpui::div().into_any_element()) } else { self.library_setup.clone().into_any_element() };

        let content = components::library_section_content().child(body);
        #[cfg(feature = "audiobooks")]
        let content = content.when_some(self.audiobook.as_ref().map(|active| active.dock.clone()), |content, dock| content.child(dock));

        let page = if library_configured || application_page { page.child(components::browser_body().child(self.navigation.clone()).child(content)) } else { page.child(content) };

        page.children(self.navigation.read(cx).library_switcher_overlay(self.navigation.clone(), cx)).children(Root::render_notification_layer(window, cx)).children(Root::render_dialog_layer(window, cx)).into_any_element()
    }
}
