use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use bokheim_ui_runtime::BlockingWorker;
use epub_svelte_lib::library::Library;
use epub_svelte_lib::{BookId, BookLocator};
use gpui::{AnyWindowHandle, App, AppContext, Bounds, Global, SharedString, Subscription, WeakEntity, WindowBounds, WindowOptions, px, size};
use gpui_component::Root;

use crate::model::{PendingReader, ReaderPreferences, ReaderSettings, ReaderView, ReaderWindowGeometry, ReaderWindowId, SaveReaderSettings};
use crate::pdf_reader::{PdfReaderView, PendingPdfReader};

enum ReaderViewHandle {
    Epub(WeakEntity<ReaderView>),
    Pdf(WeakEntity<PdfReaderView>),
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
    cx.set_global(ReaderSettings { preferences, geometry, save: Some(save), blocking_worker: Some(blocking_worker), persistence_worker: Some(persistence_worker) });
}

pub fn open_reader_window(locator: BookLocator, title: String, library: Arc<Library>, cx: &mut App) {
    let prepare_locator = locator.clone();
    let Some(blocking_worker) = cx.global::<ReaderSettings>().blocking_worker.clone() else {
        log::error!("reader blocking worker is not configured");
        return;
    };
    let prepare = blocking_worker.dispatch(move || {
        let path = library.try_book_path(prepare_locator.book_id()).map_err(|error| error.to_string())?.ok_or_else(|| format!("file path is unavailable for book {}", prepare_locator.book_id()))?;
        let bytes = std::fs::read(&path).map_err(|error| error.to_string())?;
        let (nav_state, pdf_page) = if pdf_reader_core::is_pdf_path(&path) {
            (None, library.get_pdf_reading_page(prepare_locator.book_id()).map_err(|error| error.to_string())?.unwrap_or(0) as usize)
        } else {
            (library.get_book_reading_position(prepare_locator.book_id()).map_err(|error| error.to_string())?, 0)
        };
        library.mark_book_opened(prepare_locator.book_id()).map_err(|error| error.to_string())?;
        Ok::<_, String>((library, path, bytes, nav_state, pdf_page))
    });
    cx.spawn(async move |cx| {
        let Ok(result) = prepare.recv().await else {
            return;
        };
        let _ = cx.update(|cx| {
            let (library, path, bytes, nav_state, pdf_page) = match result {
                Ok(prepared) => prepared,
                Err(error) => {
                    log::error!("cannot open book {} in library {}: {error}", locator.book_id(), locator.library_id());
                    return;
                }
            };
            open_prepared_reader_window(locator, title, library, path, bytes, nav_state, pdf_page, cx);
        });
    })
    .detach();
}

fn open_prepared_reader_window(locator: BookLocator, title: String, library: Arc<Library>, path: std::path::PathBuf, bytes: Vec<u8>, nav_state: Option<String>, pdf_page: usize, cx: &mut App) {
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
    let is_pdf = pdf_reader_core::is_pdf_path(&path);
    let handle = cx
        .open_window(WindowOptions { window_bounds: Some(bounds), titlebar: Some(gpui::TitlebarOptions { title: Some(SharedString::from(title.clone())), ..Default::default() }), focus: true, ..Default::default() }, move |window, cx| {
            if is_pdf {
                let reader = cx.new(|cx| PdfReaderView::new(PendingPdfReader { id, locator: locator_for_window, title, library: library_for_window, path, bytes, initial_page: pdf_page }, window, cx));
                *reader_for_window.borrow_mut() = Some(ReaderViewHandle::Pdf(reader.downgrade()));
                cx.new(|cx| Root::new(reader, window, cx))
            } else {
                let reader = cx.new(|cx| ReaderView::new(PendingReader { id, locator: locator_for_window, title, library: library_for_window, bytes, nav_state }, window, cx));
                *reader_for_window.borrow_mut() = Some(ReaderViewHandle::Epub(reader.downgrade()));
                cx.new(|cx| Root::new(reader, window, cx))
            }
        })
        .expect("failed to open reader window");
    let reader = reader_out.borrow_mut().take().expect("reader window always constructs its view");
    cx.default_global::<ReaderWindowRegistry>().windows.insert(id, ReaderRecord { locator, handle: handle.into(), view: reader });
}

pub fn handle_book_removed(library_id: &str, book_id: BookId, cx: &mut App) {
    let readers = cx
        .default_global::<ReaderWindowRegistry>()
        .windows
        .values()
        .filter(|record| record.locator.library_id().as_str() == library_id && record.locator.book_id() == book_id)
        .map(|record| match &record.view {
            ReaderViewHandle::Epub(view) => ReaderViewHandle::Epub(view.clone()),
            ReaderViewHandle::Pdf(view) => ReaderViewHandle::Pdf(view.clone()),
        })
        .collect::<Vec<_>>();
    for reader in readers {
        match reader {
            ReaderViewHandle::Epub(reader) => {
                let _ = reader.update(cx, |reader, _| {
                    reader.unavailable = true;
                    log::warn!("book {} disappeared while reader {} remained open", reader.locator.book_id(), reader.id.0);
                });
            }
            ReaderViewHandle::Pdf(reader) => {
                let _ = reader.update(cx, |reader, _| {
                    reader.unavailable = true;
                    log::warn!("book {} disappeared while PDF reader {} remained open", reader.locator.book_id(), reader.id.0);
                });
            }
        }
    }
}

pub fn close_readers_for_library(library_id: &str, cx: &mut App) {
    let handles = cx.default_global::<ReaderWindowRegistry>().windows.values().filter(|record| record.locator.library_id().as_str() == library_id).map(|record| record.handle).collect::<Vec<_>>();
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
        cx.default_global::<ReaderWindowRegistry>().windows.retain(|_, record| record.handle.window_id() != window_id);
    });
    cx.default_global::<ReaderWindowRegistry>().close_subscription = Some(subscription);
}
