use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
#[cfg(feature = "audiobooks")]
use std::time::Duration;
use std::time::Instant;

use epub_svelte_lib::library::ReaderFormat;
#[cfg(any(feature = "pdf", feature = "audiobooks"))]
use epub_svelte_lib::library::ResolvedBook;
use epub_svelte_lib::{BookLocator, ContentHash, LibraryBackend, LibraryId};
use gpui::{AnyWindowHandle, App, AppContext, Bounds, Global, SharedString, Subscription, WeakEntity, WindowBounds, WindowOptions, px, size};
use gpui_component::Root;
use ui_runtime::BlockingWorker;

#[cfg(feature = "kobo")]
fn platform_content<V: gpui::Render + 'static>(content: gpui::Entity<V>, cx: &mut App) -> gpui::Entity<gpui_kobo::KoboKeyboardRoot<V>> {
    cx.new(|_| gpui_kobo::KoboKeyboardRoot::new(content))
}

#[cfg(not(feature = "kobo"))]
fn platform_content<V: gpui::Render + 'static>(content: gpui::Entity<V>, _: &mut App) -> gpui::Entity<V> {
    content
}

use crate::cbz_reader::{CbzReaderView, PendingCbzReader, cbz_page_from_cfi};
use crate::epub::{PendingReader, ReaderView, ReaderWindowId};
#[cfg(feature = "pdf")]
use crate::pdf_reader::{PdfReaderView, PendingPdfReader};
use crate::settings::{ReaderPreferences, ReaderServices, ReaderSettings, ReaderWindowGeometry, SaveReaderSettings};
#[cfg(feature = "audiobooks")]
use crate::shell::persistence::PositionWriter;

enum ReaderViewHandle {
    Epub(WeakEntity<ReaderView>),
    Cbz(WeakEntity<CbzReaderView>),
    #[cfg(feature = "audiobooks")]
    Audiobook(WeakEntity<audiobook_player::AudiobookPlayer>),
    #[cfg(feature = "pdf")]
    Pdf(WeakEntity<PdfReaderView>),
}

enum PreparedBook {
    Publication {
        publication: Arc<crate::publication::NativePublication>,
        nav_state: Option<String>,
    },
    #[cfg(feature = "pdf")]
    Pdf {
        book: ResolvedBook,
        initial_page: usize,
    },
    #[cfg(feature = "audiobooks")]
    Audiobook {
        book: ResolvedBook,
        initial_position: Duration,
    },
}

struct ReaderRecord {
    locator: BookLocator,
    handle: AnyWindowHandle,
    view: ReaderViewHandle,
}

#[derive(Default)]
struct ReaderWindowRegistry {
    next_id: u64,
    windows: HashMap<ReaderWindowId, ReaderRecord>,
    close_subscription: Option<Subscription>,
}

impl Global for ReaderWindowRegistry {}

pub fn configure(preferences: ReaderPreferences, geometry: Option<ReaderWindowGeometry>, save: SaveReaderSettings, blocking_worker: BlockingWorker, persistence_worker: BlockingWorker, cx: &mut App) {
    cx.set_global(ReaderSettings { preferences, geometry, save });
    cx.set_global(ReaderServices { blocking: blocking_worker, persistence: persistence_worker, speech: Arc::new(crate::backends::SystemTtsProvider::default()), audio: Arc::new(crate::backends::SystemAudioBackend::default()) });
}

pub fn configure_tts_provider(backend: Arc<dyn crate::TtsProvider>, cx: &mut App) {
    cx.global_mut::<ReaderServices>().speech = backend;
}

pub fn configure_audio_backend(backend: Arc<dyn crate::NativeAudioBackend>, cx: &mut App) {
    cx.global_mut::<ReaderServices>().audio = backend;
}

pub fn open_reader_window(locator: BookLocator, title: String, initial_target: Option<String>, library: LibraryBackend, cx: &mut App) {
    let startup_started = Instant::now();
    startup_phase!("phase=request content_hash={} elapsed_ms=0", locator.content_hash());
    #[cfg(feature = "kobo")]
    if let Err(error) = gpui_kobo::begin_book_launch_profile(startup_started) {
        log::error!("failed to start Kobo book launch profile: {error}");
    }
    let prepare_locator = locator.clone();
    let blocking_worker = ReaderServices::get(cx).blocking.clone();
    let prepare_dispatched_at = Instant::now();
    let prepare = blocking_worker.dispatch(move || {
        let worker_started = Instant::now();
        startup_phase!("phase=prepare_worker_started content_hash={} elapsed_ms={} queue_ms={}", prepare_locator.content_hash(), startup_started.elapsed().as_millis(), prepare_dispatched_at.elapsed().as_millis(),);

        let phase_started = Instant::now();
        let book = library.resolve_book_for_reader(prepare_locator.content_hash()).map_err(|error| format!("stage=book_resolve: {error}"))?;
        startup_phase!("phase=path_resolved content_hash={} elapsed_ms={} phase_ms={} path={}", prepare_locator.content_hash(), startup_started.elapsed().as_millis(), phase_started.elapsed().as_millis(), book.path.display(),);

        let prepared = match book.format {
            #[cfg(feature = "pdf")]
            ReaderFormat::Pdf => {
                let phase_started = Instant::now();
                let initial_page = library.pdf_reading_page(prepare_locator.content_hash()).map_err(|error| format!("stage=reading_position: {error}"))?.unwrap_or(0) as usize;
                startup_phase!("phase=reading_position_ready content_hash={} elapsed_ms={} phase_ms={} format=pdf", prepare_locator.content_hash(), startup_started.elapsed().as_millis(), phase_started.elapsed().as_millis(),);
                PreparedBook::Pdf { book, initial_page }
            }
            #[cfg(feature = "audiobooks")]
            ReaderFormat::Audiobook => {
                let initial_position = library.audiobook_reading_position(prepare_locator.content_hash()).map_err(|error| format!("stage=reading_position: {error}"))?.unwrap_or(0);
                PreparedBook::Audiobook { book, initial_position: Duration::from_millis(initial_position) }
            }
            #[cfg(not(feature = "audiobooks"))]
            ReaderFormat::Audiobook => return Err("stage=prepare: audiobook support is disabled in this build".to_owned()),
            _ => {
                let phase_started = Instant::now();
                let publication = crate::publication::NativePublication::from_reader(&book.publication_path, book.reader).map_err(|error| format!("stage=publication_open: {error}"))?;
                startup_phase!("phase=publication_opened content_hash={} elapsed_ms={} phase_ms={}", prepare_locator.content_hash(), startup_started.elapsed().as_millis(), phase_started.elapsed().as_millis(),);
                let nav_state = library.reading_position(prepare_locator.content_hash()).map_err(|error| format!("stage=reading_position: {error}"))?;
                PreparedBook::Publication { publication: Arc::new(publication), nav_state }
            }
        };
        startup_phase!("phase=prepare_worker_complete content_hash={} elapsed_ms={} worker_ms={}", prepare_locator.content_hash(), startup_started.elapsed().as_millis(), worker_started.elapsed().as_millis(),);
        Ok::<_, String>((library, prepared))
    });
    cx.spawn(async move |cx| {
        let Ok(result) = prepare.recv().await else {
            return;
        };
        startup_phase!("phase=prepare_result_received content_hash={} elapsed_ms={}", locator.content_hash(), startup_started.elapsed().as_millis(),);
        cx.update(|cx| {
            let (library, prepared) = match result {
                Ok(prepared) => prepared,
                Err(error) => {
                    log::error!("cannot open book {} in library {}: {error}", locator.content_hash(), locator.library_id());
                    return;
                }
            };
            startup_phase!("phase=prepared content_hash={} elapsed_ms={}", locator.content_hash(), startup_started.elapsed().as_millis());
            open_prepared_reader_window(locator, title, initial_target, library, prepared, startup_started, cx);
        });
    })
    .detach();
}

fn open_prepared_reader_window(locator: BookLocator, title: String, initial_target: Option<String>, library: LibraryBackend, prepared: PreparedBook, startup_started: Instant, cx: &mut App) {
    install_close_observer(cx);
    let id = {
        let registry = cx.default_global::<ReaderWindowRegistry>();
        registry.next_id = registry.next_id.wrapping_add(1);
        ReaderWindowId(registry.next_id)
    };
    let bounds = cx
        .global::<ReaderSettings>()
        .geometry
        .map(|geometry| WindowBounds::Windowed(Bounds { origin: gpui::point(px(geometry.x), px(geometry.y)), size: size(px(geometry.width), px(geometry.height)) }))
        .unwrap_or_else(|| WindowBounds::Windowed(Bounds::centered(None, size(px(1100.0), px(780.0)), cx)));

    let reader_out = Rc::new(RefCell::new(None));
    let reader_for_window = reader_out.clone();
    let locator_for_window = locator.clone();
    let library_for_window = library.clone();
    let handle = match cx.open_window(
        WindowOptions {
            window_bounds: Some(bounds),
            titlebar: Some(gpui::TitlebarOptions { title: Some(SharedString::from(title.clone())), ..Default::default() }),
            focus: true,
            app_id: (!cfg!(feature = "kobo")).then(|| "se.bokheim.Bokheim".to_owned()),
            ..Default::default()
        },
        move |window, cx| match prepared {
            #[cfg(feature = "audiobooks")]
            PreparedBook::Audiobook { book, initial_position } => {
                let content_hash = locator_for_window.content_hash();
                let positions = PositionWriter::new(
                    cx,
                    "persist audiobook position",
                    move |library, (position, progress): &(Duration, f32)| library.update_audiobook_reading_position(content_hash.clone(), position.as_millis() as u64, *progress).map_err(|error| error.to_string()),
                    library_for_window.clone(),
                );
                let save_position: audiobook_player::SavePlaybackPosition = Rc::new(move |position, progress| positions.record((position, progress), "audiobook position"));
                let source = audiobook_player::AudiobookSource::new(book.reader, book.path);
                let reader = audiobook_player::open_library_source(source, initial_position, save_position, cx);
                *reader_for_window.borrow_mut() = Some(ReaderViewHandle::Audiobook(reader.downgrade()));
                cx.new(|cx| Root::new(platform_content(reader, cx), window, cx))
            }
            #[cfg(feature = "pdf")]
            PreparedBook::Pdf { book, initial_page } => {
                let reader = cx.new(|cx| PdfReaderView::new(PendingPdfReader { id, locator: locator_for_window, title, library: library_for_window, book, initial_page }, window, cx));
                *reader_for_window.borrow_mut() = Some(ReaderViewHandle::Pdf(reader.downgrade()));
                cx.new(|cx| Root::new(platform_content(reader, cx), window, cx))
            }
            PreparedBook::Publication { publication, nav_state } if matches!(&publication.layout, html_publication::PublicationLayout::PagedImages { .. }) => {
                let initial_page = cbz_page_from_cfi(nav_state.as_deref(), publication.document_uris.len());
                let reader = cx.new(|cx| CbzReaderView::new(PendingCbzReader { id, locator: locator_for_window, title, library: library_for_window, publication, initial_page }, window, cx));
                reader.update(cx, |reader, cx| reader.start(cx));
                *reader_for_window.borrow_mut() = Some(ReaderViewHandle::Cbz(reader.downgrade()));
                cx.new(|cx| Root::new(platform_content(reader, cx), window, cx))
            }
            PreparedBook::Publication { publication, nav_state } => {
                let reader = cx.new(|cx| ReaderView::new(PendingReader { id, locator: locator_for_window, title, library: library_for_window, publication, nav_state, initial_target }, window, cx));
                *reader_for_window.borrow_mut() = Some(ReaderViewHandle::Epub(reader.downgrade()));
                cx.new(|cx| Root::new(platform_content(reader, cx), window, cx))
            }
        },
    ) {
        Ok(handle) => handle,
        Err(error) => {
            log::error!("failed to open reader window for book {}: {error}", locator.content_hash());
            return;
        }
    };
    let Some(reader) = reader_out.borrow_mut().take() else {
        log::error!("reader window for book {} was created without a reader view", locator.content_hash());
        return;
    };
    cx.default_global::<ReaderWindowRegistry>().windows.insert(id, ReaderRecord { locator, handle: handle.into(), view: reader });
    startup_phase!("phase=window_created elapsed_ms={}", startup_started.elapsed().as_millis());
    #[cfg(feature = "kobo")]
    if let Err(error) = gpui_kobo::request_experience_mode(gpui_kobo::KoboExperienceMode::ReaderQuality) {
        log::error!("failed to enter Kobo reader quality mode: {error}");
    }
}

pub fn handle_book_removed(library_id: &LibraryId, content_hash: ContentHash, cx: &mut App) {
    let readers = cx
        .default_global::<ReaderWindowRegistry>()
        .windows
        .values()
        .filter(|record| record.locator.library_id() == library_id && record.locator.content_hash() == content_hash)
        .map(|record| match &record.view {
            ReaderViewHandle::Epub(view) => ReaderViewHandle::Epub(view.clone()),
            ReaderViewHandle::Cbz(view) => ReaderViewHandle::Cbz(view.clone()),
            #[cfg(feature = "audiobooks")]
            ReaderViewHandle::Audiobook(view) => ReaderViewHandle::Audiobook(view.clone()),
            #[cfg(feature = "pdf")]
            ReaderViewHandle::Pdf(view) => ReaderViewHandle::Pdf(view.clone()),
        })
        .collect::<Vec<_>>();
    for reader in readers {
        match reader {
            ReaderViewHandle::Epub(reader) => {
                let _ = reader.update(cx, |reader, _| {
                    reader.unavailable = true;
                    log::warn!("book {} disappeared while reader {} remained open", reader.locator.content_hash(), reader.id.0);
                });
            }
            #[cfg(feature = "pdf")]
            ReaderViewHandle::Pdf(reader) => {
                let _ = reader.update(cx, |reader, _| {
                    reader.unavailable = true;
                    log::warn!("book {} disappeared while PDF reader {} remained open", reader.locator.content_hash(), reader.id.0);
                });
            }
            ReaderViewHandle::Cbz(reader) => {
                let _ = reader.update(cx, |reader, _| {
                    reader.unavailable = true;
                    log::warn!("book {} disappeared while CBZ reader {} remained open", reader.locator.content_hash(), reader.id.0);
                });
            }
            #[cfg(feature = "audiobooks")]
            ReaderViewHandle::Audiobook(reader) => {
                let _ = reader.update(cx, |_, _| log::warn!("audiobook disappeared while its reader remained open"));
            }
        }
    }
}

pub fn close_readers_for_library(library_id: &LibraryId, cx: &mut App) {
    let handles = cx.default_global::<ReaderWindowRegistry>().windows.values().filter(|record| record.locator.library_id() == library_id).map(|record| record.handle).collect::<Vec<_>>();
    for handle in handles {
        let _ = handle.update(cx, |_, window, _| {
            window.remove_window();
        });
    }
}

fn install_close_observer(cx: &mut App) {
    if cx.default_global::<ReaderWindowRegistry>().close_subscription.is_some() {
        return;
    }
    let subscription = cx.on_window_closed(|cx, window_id| {
        let registry = cx.default_global::<ReaderWindowRegistry>();
        registry.windows.retain(|_, record| record.handle.window_id() != window_id);
        #[cfg(feature = "kobo")]
        if registry.windows.is_empty()
            && let Err(error) = gpui_kobo::request_experience_mode(gpui_kobo::KoboExperienceMode::LibraryFast)
        {
            log::error!("failed to return to Kobo library fast mode: {error}");
        }
    });
    cx.default_global::<ReaderWindowRegistry>().close_subscription = Some(subscription);
}
