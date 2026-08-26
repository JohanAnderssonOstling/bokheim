//! Window-level owner and router.
//!
//! Owns application navigation, the selected library, and the current route
//! root. Nested navigation and book detail stay inside page components.

use app_contract::library::{BookLocator, LibrarySummary};
use gpui::prelude::*;
use gpui::{AnyView, App, AppContext, Context, Entity, FocusHandle, Focusable, IntoElement, Render, SelectedFile, SharedString, Subscription, Task, Window, px};
use gpui_component::notification::Notification;
use gpui_component::{Root, WindowExt};
use sync_common::{ContentHash, LibraryId, ROOT_DIR_ID};
use ui_components as components;

use crate::library::{AuthorsPage, HomePage, LibraryContext, TrashPage, folder_browse_page, store_browse_page, subject_browse_page};
use crate::navigation::{NavigationRequest, NavigationState, NavigationView, Route};
use crate::services::AppServices;
use crate::stores::Preferences;
use crate::universal::settings::SettingsPage;
use crate::{DecreaseUiFontSize, DismissSettings, IncreaseUiFontSize, ShowSettings};
use app_contract::backend::BrowserSession;
use app_contract::backend::LibraryBackend;

#[cfg(feature = "audiobooks")]
struct ActiveAudiobook {
    locator: BookLocator,
    dock: Entity<audiobook_player::AudiobookDock>,
    _subscription: Subscription,
}

pub struct BrowserRoot {
    services: AppServices,
    preferences: Entity<Preferences>,
    navigation_state: Entity<NavigationState>,
    library: LibraryContext,
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
    pub fn new(services: AppServices, #[cfg(feature = "kobo")] kobo_auto_rotate: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        cx.set_system_bars_visible(false);
        cx.set_volume_button_capture(true);
        let libraries = services.backend.libraries().expect("the application backend must provide a library list");
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
        let library = LibraryContext::new(services.clone(), library, cx);
        let restored_route = Route::Folders;
        let navigation_state = cx.new(|_| NavigationState::new(restored_route, libraries, active_library_id));
        let import_extensions = services.contracts.browsing_content.import_extensions.clone();
        let navigation = cx.new(|cx| NavigationView::new(services.backend.clone(), import_extensions, navigation_state.clone(), cx));
        let subscriptions = vec![cx.subscribe_in(&navigation, window, |root: &mut Self, _, event, window, cx| match event {
            NavigationRequest::Select(route) => root.navigate(*route, window, cx),
            NavigationRequest::SelectLibrary(library_id) => root.select_library(*library_id, window, cx),
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
                let result = list_backend.libraries();
                let _ = root.update_in(cx, |root, window, cx| match result {
                    Ok(libraries) => root.update_library_list(libraries, window, cx),
                    Err(error) => root.notify_error(error.into(), window, cx),
                });
            }
        });

        let reconciliation = services.backend.reconcile_libraries();
        cx.spawn(async move |root, cx| match reconciliation.await {
            Ok(libraries) => {
                let _ = root.update_in(cx, |root, window, cx| root.update_library_list(libraries, window, cx));
            }
            Err(error) => log::warn!("failed to reconcile libraries: {error}"),
        })
        .detach();

        let page = cx.new(|cx| folder_browse_page(library.clone(), cx));
        cx.focus_view(&page, window);
        let page = page.into();
        let root = Self {
            services,
            preferences,
            navigation_state,
            library,
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

    pub fn import_and_open_file(&mut self, file: SelectedFile, window: &mut Window, cx: &mut Context<Self>) {
        let library_id = *self.navigation_state.read(cx).library_id();
        let library = match self.services.backend.library(&library_id) {
            Ok(library) => library,
            Err(error) => {
                window.push_notification(Notification::error(format!("Could not open downloaded book: {error}")).id1::<Self>("incoming-book-library-error").autohide(false), cx);
                return;
            }
        };
        let title = std::path::Path::new(&file.name).file_stem().and_then(|title| title.to_str()).filter(|title| !title.is_empty()).unwrap_or(&file.name).to_owned();
        let open_reader = self.services.open_reader.clone();
        cx.spawn(async move |root, cx| {
            let result = library.import_book(ROOT_DIR_ID, file.name, file.bytes).await;
            let _ = root.update_in(cx, |_, window, cx| match result {
                Ok(content_hash) => open_reader(app_contract::library::BookLocator::new(library_id, content_hash), title, None, window, cx),
                Err(error) => window.push_notification(Notification::error(format!("Could not import downloaded book: {error}")).id1::<Self>("incoming-book-import-error").autohide(false), cx),
            });
        })
        .detach();
    }

    #[cfg(feature = "audiobooks")]
    pub fn show_audiobook(&mut self, locator: BookLocator, source: audiobook_player::AudiobookSource, initial_position: std::time::Duration, save_position: audiobook_player::SavePlaybackPosition, cx: &mut Context<Self>) {
        let dock = audiobook_player::open_library_dock(source, initial_position, save_position, cx);
        let subscription = cx.subscribe(&dock, |root: &mut Self, _, _: &audiobook_player::AudiobookDockEvent, cx| {
            root.audiobook = None;
            cx.notify();
        });
        self.audiobook = Some(ActiveAudiobook { locator, dock, _subscription: subscription });
        cx.notify();
    }

    #[cfg(feature = "audiobooks")]
    pub(crate) fn audiobook_matches(&self, library_id: &LibraryId, content_hash: ContentHash) -> bool {
        self.audiobook.as_ref().is_some_and(|active| active.locator.library_id() == library_id && active.locator.content_hash() == content_hash)
    }

    #[cfg(feature = "audiobooks")]
    pub(crate) fn dismiss_audiobook(&mut self, cx: &mut Context<Self>) {
        if self.audiobook.take().is_some() {
            cx.notify();
        }
    }

    fn navigate(&mut self, route: Route, window: &mut Window, cx: &mut Context<Self>) {
        if self.navigation_state.read(cx).route() == route {
            return;
        }
        cx.set_volume_button_capture(route_uses_volume_pagination(route));
        self.navigation_state.update(cx, |state, cx| state.set_route(route, cx));
        self.focus.focus(window, cx);

        self.page = match route {
            Route::Home => cx.new(|cx| HomePage::new(self.library.clone(), cx)).into(),
            Route::Authors => cx.new(|cx| AuthorsPage::new(self.library.clone(), window, cx)).into(),
            Route::Folders => {
                let page = cx.new(|cx| folder_browse_page(self.library.clone(), cx));
                cx.focus_view(&page, window);
                page.into()
            }
            Route::Subjects => {
                let page = cx.new(|cx| subject_browse_page(self.library.clone(), cx));
                cx.focus_view(&page, window);
                page.into()
            }
            Route::Trash => cx.new(|cx| TrashPage::new(self.library.clone(), cx)).into(),
            Route::Store => {
                let page = cx.new(|cx| store_browse_page(self.library.clone(), cx));
                cx.focus_view(&page, window);
                page.into()
            }
            Route::Settings => cx
                .new(|cx| {
                    SettingsPage::new(
                        self.services.clone(),
                        self.preferences.clone(),
                        self.navigation_state.clone(),
                        #[cfg(feature = "kobo")]
                        self.kobo_auto_rotate,
                        window,
                        cx,
                    )
                })
                .into(),
        };
        cx.notify();
    }

    fn update_library_list(&mut self, libraries: Vec<LibrarySummary>, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(error) = self.replace_libraries(libraries, None, window, cx) {
            self.notify_error(error.into(), window, cx);
        }
    }

    fn select_library(&mut self, library_id: LibraryId, window: &mut Window, cx: &mut Context<Self>) {
        if self.navigation_state.read(cx).library_id() == &library_id {
            return;
        }
        let result = self.services.backend.libraries().map_err(|error| error.to_string()).and_then(|libraries| self.replace_libraries(libraries, Some(library_id), window, cx));
        if let Err(error) = result {
            self.notify_error(error.into(), window, cx);
        }
    }

    fn replace_libraries(&mut self, libraries: Vec<LibrarySummary>, preferred_library_id: Option<LibraryId>, window: &mut Window, cx: &mut Context<Self>) -> Result<(), String> {
        let libraries = if libraries.is_empty() {
            self.services.backend.ensure_default_library().map_err(|error| error.to_string())?;
            self.services.backend.libraries().map_err(|error| error.to_string())?
        } else {
            libraries
        };
        let library_ids = libraries.iter().map(|entry| *entry.library_id()).collect::<Vec<_>>();
        self.services.backend.retain_browser_session_libraries(&library_ids).map_err(|error| error.to_string())?;
        let current_id = *self.navigation_state.read(cx).library_id();
        let target_id = preferred_library_id
            .filter(|id| libraries.iter().any(|entry| entry.library_id() == id))
            .or_else(|| libraries.iter().any(|entry| entry.library_id() == &current_id).then_some(current_id))
            .unwrap_or_else(|| *libraries.first().expect("an empty library list was repaired above").library_id());

        if target_id == current_id {
            self.navigation_state.update(cx, |state, cx| state.set_entries(libraries, cx));
            return Ok(());
        }

        let library = resolve_library(&self.services, &target_id)?;
        let scan_required = !library.scanner_running();
        self.services.backend.save_active_library_id(Some(target_id)).map_err(|error| error.to_string())?;
        self.navigation_state.update(cx, |state, cx| state.select_library(libraries, target_id, Route::Folders, cx));
        cx.set_volume_button_capture(true);
        self.library = LibraryContext::new(self.services.clone(), library, cx);
        let page = cx.new(|cx| folder_browse_page(self.library.clone(), cx));
        cx.focus_view(&page, window);
        self.page = page.into();
        if scan_required {
            self.services.backend.request_library_scan(&target_id).map_err(|error| format!("Could not scan selected library: {error}"))?;
        }
        cx.notify();
        Ok(())
    }

    pub(crate) fn captures_volume_buttons(&self, cx: &App) -> bool {
        route_uses_volume_pagination(self.navigation_state.read(cx).route())
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
        if components::WindowWidthClass::for_window(window).is_compact() && self.navigation.read(cx).is_more_open() {
            self.navigation.update(cx, |navigation, cx| navigation.close_more(cx));
            return;
        }
        if self.navigation.read(cx).is_library_switcher_open() {
            self.navigation.update(cx, |navigation, cx| navigation.close_library_switcher(cx));
            return;
        }
        if matches!(self.navigation_state.read(cx).route(), Route::Store | Route::Settings) {
            self.navigate(Route::Folders, window, cx);
            return;
        }
        cx.propagate();
    }

    fn adjust_ui_font_size(&mut self, delta: i16, window: &mut Window, cx: &mut Context<Self>) {
        let current = self.preferences.read(cx).ui_font_size_px();
        let [minimum, maximum] = self.services.contracts.browsing_preferences.constraints.ui_font_size_px;
        let next = (i16::from(current) + delta).clamp(i16::from(minimum), i16::from(maximum)) as u8;
        if next == current {
            return;
        }

        let result = self.preferences.update(cx, |preferences, cx| preferences.update_browsing(app_contract::preferences::BrowsingPreferencePatch::UiFontSizePx(next), cx));
        if let Err(error) = result {
            self.notify_error(format!("Could not save interface font size: {error}").into(), window, cx);
        }
    }
}

fn route_uses_volume_pagination(route: Route) -> bool {
    matches!(route, Route::Folders | Route::Subjects | Route::Store)
}

fn resolve_library(services: &AppServices, library_id: &LibraryId) -> Result<std::rc::Rc<dyn LibraryBackend>, String> {
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
            .on_action(cx.listener(|root, _: &DecreaseUiFontSize, window, cx| root.adjust_ui_font_size(-1, window, cx)))
            .on_action(cx.listener(|root, _: &IncreaseUiFontSize, window, cx| root.adjust_ui_font_size(1, window, cx)))
            .on_action(cx.listener(|root, _: &ShowSettings, window, cx| root.navigate(Route::Settings, window, cx)));

        let content = components::library_section_content().child(self.page.clone());
        #[cfg(feature = "audiobooks")]
        let content = content.when_some(self.audiobook.as_ref().map(|active| active.dock.clone()), |content, dock| content.child(dock));

        let compact_navigation = components::WindowWidthClass::for_window(window).is_compact();
        let safe_area = window.insets().safe_area;
        let page = if compact_navigation {
            page.child(gpui::div().flex_1().min_h_0().min_w_0().flex().flex_col().pt(safe_area.top).child(content).child(self.navigation.clone()))
        } else {
            page.child(components::browser_body().pt(safe_area.top).child(self.navigation.clone()).child(content))
        };

        let more_overlay = compact_navigation.then(|| self.navigation.read(cx).more_overlay(self.navigation.clone(), safe_area, cx)).flatten();
        page.children(more_overlay)
            .children(self.navigation.read(cx).library_switcher_overlay(self.navigation.clone(), cx))
            .children(Root::render_notification_layer(window, cx))
            .children(Root::render_dialog_layer(window, cx))
            .into_any_element()
    }
}
