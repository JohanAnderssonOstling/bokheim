#[cfg(all(target_os = "android", feature = "android"))]
mod android;
mod session;
#[cfg(feature = "desktop")]
mod update_notice;
#[cfg(all(feature = "desktop", target_os = "linux"))]
mod updater;
#[cfg(all(feature = "desktop", target_os = "windows"))]
mod windows_updater;

use std::borrow::Cow;
use std::cell::RefCell;
use std::rc::Rc;
#[cfg(feature = "desktop")]
use std::sync::{Arc, LazyLock};
#[cfg(feature = "audiobooks")]
use std::time::Duration;

use bokheim_backend::app::{AppBackend, BackendContext};
#[cfg(feature = "kobo")]
use browser_ui::SaveKoboAutoRotate;
use browser_ui::{AppServices, ApplicationRoot, BrowserRoot, OpenReader, OpenedBook};
use gpui::{App, AppContext, Bounds, Entity, SharedString, WeakEntity, WindowBounds, WindowOptions, px, size};
use gpui_component::Root;
#[cfg(feature = "kobo")]
use gpui_kobo::KoboKeyboardRoot;
use reader_ui::{ReaderPreferences, SaveReaderSettings};
use ui_runtime::BlockingWorker;

use session::{SessionSaveScheduler, UiSession, WindowGeometry, session_path};

#[cfg(feature = "desktop")]
const DESKTOP_APP_ID: &str = "se.bokheim.Bokheim";

#[cfg(feature = "desktop")]
static DESKTOP_ICON: LazyLock<Option<Arc<image::RgbaImage>>> =
    LazyLock::new(|| image::load_from_memory_with_format(include_bytes!("../../ui/design-tokens/assets/icon/bokheim-180.png"), image::ImageFormat::Png).ok().map(|image| Arc::new(image.into_rgba8())));

#[cfg(feature = "desktop")]
fn desktop_window_identity() -> (Option<String>, Option<Arc<image::RgbaImage>>) {
    (Some(DESKTOP_APP_ID.to_owned()), DESKTOP_ICON.clone())
}

/// Small host-specific policy boundary. The complete browser and reader UI is shared.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UiLaunchOptions {
    /// Persist desktop window positions and sizes. Kobo windows are always fullscreen.
    pub persist_window_geometry: bool,
    /// Override the persisted Kobo auto-rotation preference for this launch, if any.
    pub kobo_auto_rotate: Option<bool>,
}

impl UiLaunchOptions {
    pub const DESKTOP: Self = Self { persist_window_geometry: true, kobo_auto_rotate: None };
    pub const KOBO: Self = Self { persist_window_geometry: false, kobo_auto_rotate: None };
    pub const MOBILE: Self = Self { persist_window_geometry: false, kobo_auto_rotate: None };
}

#[cfg(feature = "kobo")]
fn platform_content<V: gpui::Render + 'static>(content: Entity<V>, cx: &mut App) -> Entity<KoboKeyboardRoot<V>> {
    cx.new(|_| KoboKeyboardRoot::new(content))
}

#[cfg(any(feature = "desktop", feature = "mobile"))]
fn platform_content<V: gpui::Render + 'static>(content: Entity<V>, _: &mut App) -> Entity<V> {
    content
}

/// Install and open the existing Bokheim browser/reader UI in the selected GPUI platform.
pub fn open_ui(backend_context: BackendContext, options: UiLaunchOptions, incoming_files: Option<async_channel::Receiver<gpui::SelectedFile>>, cx: &mut App) {
    let _ = rustls::crypto::ring::default_provider().install_default();
    if let Err(error) = cx.text_system().add_fonts(vec![
        Cow::Borrowed(include_bytes!("../assets/fonts/NotoSans-Regular.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/NotoSans-Bold.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/LibertinusSerif-Regular.otf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/LibertinusSerif-Bold.otf")),
        #[cfg(feature = "kobo")]
        Cow::Borrowed(include_bytes!("../../../reference/plato/fonts/TerminusTTF-Regular.ttf")),
        #[cfg(feature = "kobo")]
        Cow::Borrowed(include_bytes!("../../../reference/plato/fonts/TerminusTTF-Bold.ttf")),
    ]) {
        log::warn!("failed to install application fonts: {error}");
    }
    let session_path = session_path(backend_context.app_data_dir());
    let saved_session = Rc::new(RefCell::new(UiSession::load(&session_path)));

    gpui_component::init(cx);
    ui_components::init_page_scrolling(cx);
    browser_ui::bind_keys(cx);
    #[cfg(feature = "mobile")]
    cx.bind_keys([gpui::KeyBinding::new("back", browser_ui::DismissSettings, Some("Application"))]);
    reader_ui::bind_keys(cx);
    #[cfg(any(feature = "desktop", feature = "audiobooks"))]
    let blocking_worker = BlockingWorker::new(cx.background_executor(), 2);
    let persistence_worker = BlockingWorker::new(cx.background_executor(), 1);
    #[cfg(all(feature = "desktop", target_os = "linux"))]
    let desktop_updater = updater::direct_appimage_updater(blocking_worker.clone());
    #[cfg(all(feature = "desktop", target_os = "windows"))]
    let desktop_updater = windows_updater::windows_installer_updater(blocking_worker.clone());
    #[cfg(all(feature = "desktop", not(any(target_os = "linux", target_os = "windows"))))]
    let desktop_updater: Option<browser_ui::DesktopUpdater> = None;
    #[cfg(feature = "mobile")]
    let desktop_updater: Option<browser_ui::DesktopUpdater> = None;
    #[cfg(feature = "desktop")]
    let desktop_updater_for_check = desktop_updater.clone();
    let session_saves = SessionSaveScheduler::new(saved_session.clone(), session_path, persistence_worker.clone());
    #[cfg(feature = "kobo")]
    let kobo_auto_rotate = {
        let fallback = saved_session.borrow().kobo_auto_rotate;
        let value = options.kobo_auto_rotate.unwrap_or(fallback);
        if options.kobo_auto_rotate.is_some_and(|override_value| override_value != fallback) {
            saved_session.borrow_mut().kobo_auto_rotate = value;
            session_saves.schedule(cx);
        }
        if let Err(error) = gpui_kobo::request_auto_rotation(value) {
            log::warn!("failed to apply Kobo auto-rotation setting: {error}");
        }
        value
    };

    let backend = AppBackend::new_deferred_scanning(backend_context.clone()).expect("failed to initialize shared application backend");
    backend.ensure_default_library().expect("failed to initialize a default library");
    let catalog_registry_refresh = backend.spawn(|backend| async move { backend.refresh_catalog_sources().await });
    cx.spawn(async move |_| match catalog_registry_refresh.await {
        Ok(Ok(sources)) => log::debug!("refreshed {} catalogue sources from the catalogue server", sources.len()),
        Ok(Err(error)) => log::warn!("continuing with the cached catalogue registry after refresh failed: {error}"),
        Err(error) => log::warn!("catalogue registry refresh task failed: {error}"),
    })
    .detach();
    let book_removals = backend.book_removals();

    match backend.reader_preferences() {
        Ok(preferences) => {
            saved_session.borrow_mut().reader_preferences.apply_synchronized(&preferences);
        }
        Err(error) => log::warn!("failed to load shared reader preferences: {error}"),
    }

    let reader_settings = saved_session.clone();
    let reader_session_saves = session_saves.clone();
    let reader_preferences_core = backend.clone();
    let reader_preferences_worker = persistence_worker.clone();
    let save_reader_settings: SaveReaderSettings = Rc::new(move |preferences: ReaderPreferences, cx: &mut App| {
        let mut session = reader_settings.borrow_mut();
        let previous = session.reader_preferences.clone();
        let patches = previous.synchronized_patches(&preferences);
        session.reader_preferences = preferences;
        drop(session);
        reader_session_saves.schedule(cx);
        if !patches.is_empty() {
            let core = reader_preferences_core.clone();
            reader_preferences_worker.dispatch(move || {
                for patch in patches {
                    if let Err(error) = core.update_reader_preference(patch) {
                        log::warn!("failed to persist reader preference: {error}");
                    }
                }
            });
        }
    });

    #[cfg(feature = "kobo")]
    let save_kobo_auto_rotate: SaveKoboAutoRotate = {
        let session = saved_session.clone();
        let session_saves = session_saves.clone();
        Rc::new(move |enabled: bool, cx: &mut App| {
            let mut session = session.borrow_mut();
            if session.kobo_auto_rotate == enabled {
                return;
            }
            session.kobo_auto_rotate = enabled;
            drop(session);
            session_saves.schedule(cx);
            if let Err(error) = gpui_kobo::request_auto_rotation(enabled) {
                log::warn!("failed to apply Kobo auto-rotation setting: {error}");
            }
        })
    };
    reader_ui::configure(saved_session.borrow().reader_preferences.clone(), save_reader_settings, cx);

    let reader_backend = backend.clone();
    let application_root: Rc<RefCell<Option<WeakEntity<ApplicationRoot>>>> = Rc::new(RefCell::new(None));
    let application_root_for_open = application_root.clone();
    let application_root_for_removals = application_root.clone();
    let application_root_for_incoming = application_root.clone();
    #[cfg(feature = "audiobooks")]
    let audiobook_worker = blocking_worker.clone();
    let open_reader: OpenReader = Rc::new(move |locator, title, initial_target, window, cx| {
        let Some(root) = application_root_for_open.borrow().clone() else {
            log::error!("cannot open book {}: application window is unavailable", locator.content_hash());
            return;
        };
        let request = match root.update(cx, |root, _| root.begin_open(locator.clone())) {
            Ok(request) => request,
            Err(_) => {
                log::error!("cannot open book {}: application window was closed", locator.content_hash());
                return;
            }
        };
        let library = match reader_backend.library(locator.library_id()) {
            Ok(library) => library,
            Err(error) => {
                let message = format!("Could not open book: library {} is unavailable: {error}", locator.library_id());
                log::error!("cannot open book {}: {message}", locator.content_hash());
                let _ = root.update_in(cx, move |root, window, cx| root.fail_open(request, message, window, cx));
                return;
            }
        };
        let is_audiobook = match library.is_audiobook(locator.content_hash()) {
            Ok(is_audiobook) => is_audiobook,
            Err(error) => {
                let message = format!("Could not determine the book format: {error}");
                log::error!("cannot open book {}: {message}", locator.content_hash());
                let _ = root.update_in(cx, move |root, window, cx| root.fail_open(request, message, window, cx));
                return;
            }
        };
        #[cfg(not(feature = "audiobooks"))]
        if is_audiobook {
            let message = "Could not open audiobook: audiobook support is disabled in this build";
            log::error!("cannot open audiobook {}: audiobook support is disabled in this build", locator.content_hash());
            let _ = root.update_in(cx, move |root, window, cx| root.fail_open(request, message, window, cx));
            return;
        }
        #[cfg(feature = "audiobooks")]
        if is_audiobook {
            let requested_position = initial_target.as_deref().and_then(library_model::audiobook_toc_position).map(Duration::from_millis);
            let content_hash = locator.content_hash();
            let prepare_library = library.clone();
            let persistence_library = library;
            let persistence_worker = audiobook_worker.clone();
            let prepare_content_hash = content_hash.clone();
            let prepare = audiobook_worker.dispatch(move || {
                let source = prepare_library.resolve_book_for_reader(prepare_content_hash.clone()).map_err(|error| error.to_string())?;
                let saved_position = prepare_library.audiobook_reading_position(prepare_content_hash).map_err(|error| error.to_string())?.unwrap_or(0);
                let source = audiobook_player::AudiobookSource::new(source.reader, source.path);
                Ok::<_, String>((source, requested_position.unwrap_or_else(|| Duration::from_millis(saved_position))))
            });
            let audiobook_locator = locator.clone();
            if root
                .update(cx, move |root, cx| {
                    let prepare = cx.spawn(async move |_, _| prepare.recv().await.map_err(|error| format!("book preparation worker stopped: {error}"))?);
                    root.open_book(
                        request,
                        prepare,
                        move |(source, initial_position), _, browser, _, cx| {
                            let persistence_content_hash = content_hash.clone();
                            let save_content_hash = content_hash.clone();
                            let (position_tx, position_rx) = async_channel::unbounded::<(Duration, f32)>();
                            let debounce_timer = cx.background_executor().clone();
                            cx.background_executor()
                                .spawn(async move {
                                    while let Ok((mut position, mut progress)) = position_rx.recv().await {
                                        debounce_timer.timer(Duration::from_millis(250)).await;
                                        while let Ok((newer_position, newer_progress)) = position_rx.try_recv() {
                                            position = newer_position;
                                            progress = newer_progress;
                                        }
                                        let library = persistence_library.clone();
                                        let update_content_hash = persistence_content_hash.clone();
                                        let result = persistence_worker
                                            .dispatch(move || {
                                                let serialized = library_model::audiobook_reading_position(position.as_millis() as u64);
                                                library.update_reading_position(update_content_hash, &serialized, Some(progress)).map_err(|error| error.to_string())
                                            })
                                            .recv()
                                            .await;
                                        match result {
                                            Ok(Ok(())) => {}
                                            Ok(Err(error)) => log::warn!("failed to persist audiobook position for book {persistence_content_hash}: {error}"),
                                            Err(_) => break,
                                        }
                                    }
                                })
                                .detach();
                            let save_position: audiobook_player::SavePlaybackPosition = Rc::new(move |position, progress| {
                                if position_tx.try_send((position, progress)).is_err() {
                                    log::warn!("audiobook position worker stopped for book {save_content_hash}");
                                }
                            });
                            browser.update(cx, |browser, cx| browser.show_audiobook(audiobook_locator, source, initial_position, save_position, cx));
                            OpenedBook::Library
                        },
                        cx,
                    );
                })
                .is_err()
            {
                log::error!("cannot open audiobook: application window was closed");
            }
            return;
        }
        let library = Rc::new(library);
        if root
            .update(cx, move |root, cx| {
                let return_to_library = root.return_to_library_callback(cx);
                let reader = cx.new(|cx| reader_ui::ReaderView::new(locator, title, initial_target, library, return_to_library, window, cx));
                root.show_reader(request, reader.into(), cx);
            })
            .is_err()
        {
            log::error!("cannot open book: application window was closed");
        }
    });

    let browser_bounds = if options.persist_window_geometry {
        let bounds = saved_session.borrow().window.map(WindowGeometry::bounds).unwrap_or_else(|| Bounds::centered(None, size(px(1240.0), px(820.0)), cx));
        WindowBounds::Windowed(bounds)
    } else {
        WindowBounds::Windowed(Bounds { origin: gpui::point(px(0.0), px(0.0)), size: size(px(300.0), px(400.0)) })
    };
    #[cfg(feature = "desktop")]
    let (app_id, icon) = desktop_window_identity();
    #[cfg(any(feature = "kobo", feature = "mobile"))]
    let (app_id, icon) = (None, None);
    let window_persistence = options.persist_window_geometry.then(|| (saved_session.clone(), session_saves.clone()));
    let browser_window = cx
        .open_window(
            WindowOptions {
                window_bounds: Some(browser_bounds),
                titlebar: options.persist_window_geometry.then(|| gpui::TitlebarOptions { title: Some(SharedString::from(bokheim_backend::APP_NAME)), ..Default::default() }),
                focus: true,
                app_id,
                icon,
                ..Default::default()
            },
            move |window, cx| {
                #[cfg(feature = "kobo")]
                let services = AppServices::new(backend, open_reader, None, save_kobo_auto_rotate.clone());
                #[cfg(any(feature = "desktop", feature = "mobile"))]
                let services = AppServices::new(backend, open_reader, desktop_updater);
                #[cfg(feature = "kobo")]
                let browser = cx.new(|cx| BrowserRoot::new(services, kobo_auto_rotate, window, cx));
                #[cfg(any(feature = "desktop", feature = "mobile"))]
                let browser = cx.new(|cx| BrowserRoot::new(services, window, cx));
                let application = cx.new(|cx| {
                    if let Some((session, saves)) = window_persistence {
                        cx.observe_window_bounds(window, move |_, window, cx| {
                            let geometry = WindowGeometry::from_bounds(window.bounds());
                            let mut session = session.borrow_mut();
                            if session.window == Some(geometry) {
                                return;
                            }
                            session.window = Some(geometry);
                            drop(session);
                            saves.schedule(cx);
                        })
                        .detach();
                    }
                    ApplicationRoot::new(browser, bokheim_backend::APP_NAME)
                });
                *application_root.borrow_mut() = Some(application.downgrade());
                let content = platform_content(application, cx);
                cx.new(|cx| Root::new(content, window, cx))
            },
        )
        .expect("failed to open browsing window");

    #[cfg(feature = "desktop")]
    update_notice::check_for_update(desktop_updater_for_check, browser_window.into(), cx);
    #[cfg(any(feature = "kobo", feature = "mobile"))]
    let _ = browser_window;
    forward_book_removals(book_removals, application_root_for_removals, cx);
    if let Some(incoming_files) = incoming_files {
        forward_incoming_files(incoming_files, application_root_for_incoming, cx);
    }

    let quit_session_saves = session_saves.clone();
    cx.on_app_quit(move |_| {
        quit_session_saves.flush();
        async {}
    })
    .detach();
    cx.activate(true);
}

fn forward_incoming_files(files: async_channel::Receiver<gpui::SelectedFile>, application_root: Rc<RefCell<Option<WeakEntity<ApplicationRoot>>>>, cx: &mut App) {
    cx.spawn(async move |cx| {
        while let Ok(file) = files.recv().await {
            let Some(root) = application_root.borrow().clone() else { continue };
            let _ = root.update_in(cx, |root, window, cx| root.import_and_open_file(file, window, cx));
        }
    })
    .detach();
}

fn forward_book_removals(removals: async_channel::Receiver<(bokheim_backend::LibraryId, bokheim_backend::ContentHash)>, application_root: Rc<RefCell<Option<WeakEntity<ApplicationRoot>>>>, cx: &mut App) {
    cx.spawn(async move |cx| {
        while let Ok((library_id, content_hash)) = removals.recv().await {
            let Some(root) = application_root.borrow().clone() else { continue };
            let _ = root.update_in(cx, |root, window, cx| root.handle_book_removed(&library_id, content_hash, window, cx));
        }
    })
    .detach();
}
