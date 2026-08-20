mod session;
#[cfg(not(feature = "kobo"))]
mod update_notice;
#[cfg(all(not(feature = "kobo"), target_os = "linux"))]
mod updater;
#[cfg(all(not(feature = "kobo"), target_os = "windows"))]
mod windows_updater;

use std::borrow::Cow;
#[cfg(feature = "audiobooks")]
use std::cell::Cell;
use std::cell::RefCell;
use std::rc::Rc;
#[cfg(not(feature = "kobo"))]
use std::sync::{Arc, LazyLock};
#[cfg(feature = "audiobooks")]
use std::time::Duration;

#[cfg(feature = "kobo")]
use browser_ui::SaveKoboAutoRotate;
use browser_ui::{AppServices, BrowserRoot, OpenReader};
use epub_svelte_lib::app::preferences::ReaderPreferencePatch;
use epub_svelte_lib::app::{AppBackend, BackendContext};
#[cfg(feature = "audiobooks")]
use gpui::WeakEntity;
use gpui::{App, AppContext, Bounds, Entity, SharedString, WindowBounds, WindowOptions, px, size};
use gpui_component::Root;
#[cfg(feature = "kobo")]
use gpui_kobo::KoboKeyboardRoot;
use reader_ui::{ReaderPreferences, ReaderWindowGeometry, SaveReaderSettings};
use ui_runtime::BlockingWorker;

use session::{SessionSaveScheduler, UiSession, session_path};

#[cfg(not(feature = "kobo"))]
const DESKTOP_APP_ID: &str = "se.bokheim.Bokheim";

#[cfg(not(feature = "kobo"))]
static DESKTOP_ICON: LazyLock<Option<Arc<image::RgbaImage>>> =
    LazyLock::new(|| image::load_from_memory_with_format(include_bytes!("../../ui/design-tokens/assets/icon/bokheim-180.png"), image::ImageFormat::Png).ok().map(|image| Arc::new(image.into_rgba8())));

#[cfg(not(feature = "kobo"))]
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
}

#[cfg(feature = "kobo")]
fn platform_content<V: gpui::Render + 'static>(content: Entity<V>, cx: &mut App) -> Entity<KoboKeyboardRoot<V>> {
    cx.new(|_| KoboKeyboardRoot::new(content))
}

#[cfg(not(feature = "kobo"))]
fn platform_content<V: gpui::Render + 'static>(content: Entity<V>, _: &mut App) -> Entity<V> {
    content
}

/// Install and open the existing Bokheim browser/reader UI in the selected GPUI platform.
pub fn open_ui(backend_context: BackendContext, options: UiLaunchOptions, cx: &mut App) {
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
    reader_ui::bind_keys(cx);
    let blocking_worker = BlockingWorker::new(cx.background_executor(), 2);
    let persistence_worker = BlockingWorker::new(cx.background_executor(), 1);
    #[cfg(all(not(feature = "kobo"), target_os = "linux"))]
    let desktop_updater = updater::direct_appimage_updater(blocking_worker.clone());
    #[cfg(all(not(feature = "kobo"), target_os = "windows"))]
    let desktop_updater = windows_updater::windows_installer_updater(blocking_worker.clone());
    #[cfg(all(not(feature = "kobo"), not(any(target_os = "linux", target_os = "windows"))))]
    let desktop_updater: Option<browser_ui::DesktopUpdater> = None;
    #[cfg(not(feature = "kobo"))]
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
    let library_entries = backend.libraries().expect("failed to load library entries");
    let book_removals = backend.book_removals();

    match backend.reader_preferences() {
        Ok(preferences) => {
            let mut session = saved_session.borrow_mut();
            session.reader_preferences.font_size = preferences.font_size_px();
            session.reader_preferences.column_width = preferences.max_line_width_px();
            session.reader_preferences.scale = preferences.zoom();
            session.reader_preferences.line_height = preferences.line_height();
            session.reader_preferences.font_family = preferences.font_family();
            session.reader_preferences.text_alignment = preferences.text_alignment();
            session.reader_preferences.max_column_count = preferences.max_column_count();
            session.reader_preferences.pdf_zoom_mode = preferences.pdf_zoom_mode();
            session.reader_preferences.pdf_page_count = preferences.pdf_page_count();
            session.reader_preferences.pdf_zoom = preferences.pdf_zoom();
            session.reader_preferences.pdf_trim_margins = preferences.pdf_trim_margins();
        }
        Err(error) => log::warn!("failed to load shared reader preferences: {error}"),
    }

    let reader_settings = saved_session.clone();
    let reader_session_saves = session_saves.clone();
    let reader_preferences_core = backend.clone();
    let reader_preferences_worker = persistence_worker.clone();
    let save_reader_settings: SaveReaderSettings = Rc::new(move |preferences: ReaderPreferences, geometry: Option<ReaderWindowGeometry>, cx: &mut App| {
        let mut session = reader_settings.borrow_mut();
        let previous = session.reader_preferences.clone();
        let mut patches = Vec::new();
        if previous.font_size != preferences.font_size {
            patches.push(ReaderPreferencePatch::FontSizePx(preferences.font_size));
        }
        if previous.column_width != preferences.column_width {
            patches.push(ReaderPreferencePatch::MaxLineWidthPx(preferences.column_width));
        }
        if previous.scale != preferences.scale {
            patches.push(ReaderPreferencePatch::Zoom(preferences.scale));
        }
        if previous.line_height != preferences.line_height {
            patches.push(ReaderPreferencePatch::LineHeight(preferences.line_height));
        }
        if previous.font_family != preferences.font_family {
            patches.push(ReaderPreferencePatch::FontFamily(preferences.font_family));
        }
        if previous.text_alignment != preferences.text_alignment {
            patches.push(ReaderPreferencePatch::TextAlignment(preferences.text_alignment));
        }
        if previous.max_column_count != preferences.max_column_count {
            patches.push(ReaderPreferencePatch::MaxColumnCount(preferences.max_column_count));
        }
        if previous.pdf_zoom_mode != preferences.pdf_zoom_mode {
            patches.push(ReaderPreferencePatch::PdfZoomMode(preferences.pdf_zoom_mode));
        }
        if previous.pdf_page_count != preferences.pdf_page_count {
            patches.push(ReaderPreferencePatch::PdfPageCount(preferences.pdf_page_count));
        }
        if previous.pdf_zoom != preferences.pdf_zoom {
            patches.push(ReaderPreferencePatch::PdfZoom(preferences.pdf_zoom));
        }
        if previous.pdf_trim_margins != preferences.pdf_trim_margins {
            patches.push(ReaderPreferencePatch::PdfTrimMargins(preferences.pdf_trim_margins));
        }
        session.reader_preferences = preferences;
        if options.persist_window_geometry && geometry.is_some() {
            session.reader_window = geometry;
        }
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
    let configured_geometry = options.persist_window_geometry.then(|| saved_session.borrow().reader_window).flatten();
    reader_ui::configure(saved_session.borrow().reader_preferences.clone(), configured_geometry, save_reader_settings, blocking_worker.clone(), persistence_worker, cx);

    let reader_backend = backend.clone();
    #[cfg(feature = "audiobooks")]
    let audiobook_worker = blocking_worker.clone();
    #[cfg(feature = "audiobooks")]
    let audiobook_browser: Rc<RefCell<Option<WeakEntity<BrowserRoot>>>> = Rc::new(RefCell::new(None));
    #[cfg(feature = "audiobooks")]
    let audiobook_browser_for_open = audiobook_browser.clone();
    #[cfg(feature = "audiobooks")]
    let audiobook_request_generation = Rc::new(Cell::new(0_u64));
    let open_reader: OpenReader = Rc::new(move |locator, title, initial_target, cx| {
        let library = match reader_backend.library(locator.library_id()) {
            Ok(library) => library,
            Err(error) => {
                log::error!("cannot open book {}: library {} is unavailable: {error}", locator.content_hash(), locator.library_id());
                return;
            }
        };
        #[cfg(not(feature = "audiobooks"))]
        if library.is_audiobook(locator.content_hash()).unwrap_or(false) {
            log::error!("cannot open audiobook {}: audiobook support is disabled in this build", locator.content_hash());
            return;
        }
        #[cfg(feature = "audiobooks")]
        if library.is_audiobook(locator.content_hash()).unwrap_or(false) {
            let requested_position = initial_target.as_deref().and_then(epub_svelte_lib::library::audiobook_toc_position).map(Duration::from_millis);
            let request_generation = audiobook_request_generation.get().wrapping_add(1);
            audiobook_request_generation.set(request_generation);
            let active_request_generation = audiobook_request_generation.clone();
            let content_hash = locator.content_hash();
            let prepare_library = library.clone();
            let persistence_library = library;
            let persistence_worker = audiobook_worker.clone();
            let audiobook_browser = audiobook_browser_for_open.clone();
            let prepare_content_hash = content_hash.clone();
            let prepare = audiobook_worker.dispatch(move || {
                let source = prepare_library.resolve_book_for_reader(prepare_content_hash.clone()).map_err(|error| error.to_string())?;
                let saved_position = prepare_library.audiobook_reading_position(prepare_content_hash).map_err(|error| error.to_string())?.unwrap_or(0);
                let source = audiobook_player::AudiobookSource::new(source.reader, source.path);
                Ok::<_, String>((source, requested_position.unwrap_or_else(|| Duration::from_millis(saved_position))))
            });
            cx.spawn(async move |cx| {
                let Ok(result) = prepare.recv().await else { return };
                cx.update(|cx| match result {
                    Ok((source, initial_position)) => {
                        if active_request_generation.get() != request_generation {
                            return;
                        }
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
                                    let result = persistence_worker.dispatch(move || library.update_audiobook_reading_position(update_content_hash, position.as_millis() as u64, progress).map_err(|error| error.to_string())).recv().await;
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
                        let Some(browser) = audiobook_browser.borrow().clone() else {
                            log::error!("cannot show audiobook: browser window is unavailable");
                            return;
                        };
                        if browser.update(cx, |browser, cx| browser.show_audiobook(source, initial_position, save_position, cx)).is_err() {
                            log::error!("cannot show audiobook: browser window was closed");
                        }
                    }
                    Err(error) => log::error!("cannot prepare audiobook {content_hash}: {error}"),
                });
            })
            .detach();
            return;
        }
        reader_ui::open_reader_window(locator, title, initial_target, library, cx);
    });

    let browser_bounds = if options.persist_window_geometry {
        WindowBounds::Windowed(Bounds::centered(None, size(px(1240.0), px(820.0)), cx))
    } else {
        WindowBounds::Windowed(Bounds { origin: gpui::point(px(0.0), px(0.0)), size: size(px(300.0), px(400.0)) })
    };
    #[cfg(not(feature = "kobo"))]
    let (app_id, icon) = desktop_window_identity();
    #[cfg(feature = "kobo")]
    let (app_id, icon) = (None, None);
    let browser_window = cx
        .open_window(
            WindowOptions {
                window_bounds: Some(browser_bounds),
                titlebar: options.persist_window_geometry.then(|| gpui::TitlebarOptions { title: Some(SharedString::from(epub_svelte_lib::APP_NAME)), ..Default::default() }),
                focus: true,
                app_id,
                icon,
                ..Default::default()
            },
            move |window, cx| {
                #[cfg(feature = "kobo")]
                let services = AppServices::new(backend, open_reader, None, save_kobo_auto_rotate.clone());
                #[cfg(not(feature = "kobo"))]
                let services = AppServices::new(backend, open_reader, desktop_updater);
                #[cfg(feature = "kobo")]
                let browser = cx.new(|cx| BrowserRoot::new(library_entries, services, kobo_auto_rotate, window, cx));
                #[cfg(not(feature = "kobo"))]
                let browser = cx.new(|cx| BrowserRoot::new(library_entries, services, window, cx));
                #[cfg(feature = "audiobooks")]
                {
                    *audiobook_browser.borrow_mut() = Some(browser.downgrade());
                }
                let content = platform_content(browser, cx);
                cx.new(|cx| Root::new(content, window, cx))
            },
        )
        .expect("failed to open browsing window");

    #[cfg(not(feature = "kobo"))]
    update_notice::check_for_update(desktop_updater_for_check, browser_window.into(), cx);
    #[cfg(feature = "kobo")]
    let _ = browser_window;
    forward_book_removals(book_removals, cx);

    let quit_session_saves = session_saves.clone();
    cx.on_app_quit(move |_| {
        quit_session_saves.flush();
        async {}
    })
    .detach();
    cx.activate(true);
}

fn forward_book_removals(removals: async_channel::Receiver<(epub_svelte_lib::LibraryId, epub_svelte_lib::ContentHash)>, cx: &mut App) {
    cx.spawn(async move |cx| {
        while let Ok((library_id, content_hash)) = removals.recv().await {
            cx.update(|cx| {
                reader_ui::handle_book_removed(&library_id, content_hash, cx);
            });
        }
    })
    .detach();
}
