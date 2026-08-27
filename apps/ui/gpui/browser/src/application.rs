//! Universal single-window composition for the library and publication reader.

use std::rc::Rc;

use gpui::prelude::*;
use gpui::{AnyView, App, Context, Entity, Focusable, IntoElement, Render, SelectedFile, SharedString, Task, Window};
use gpui_component::WindowExt;
use gpui_component::notification::Notification;
use library_model::BookLocator;
use sync_common::{ContentHash, LibraryId};

use crate::{BrowserRoot, DismissSettings};

/// Called by reader chrome to restore the retained library surface.
pub type ReturnToLibrary = Rc<dyn Fn(&mut Window, &mut App)>;

struct ReaderSurface {
    locator: BookLocator,
    view: AnyView,
}

/// Where a successfully prepared book should be presented.
///
/// Publication readers replace the library surface. Audiobooks install their
/// dock in the retained browser and keep the library visible.
pub enum OpenedBook {
    Reader(AnyView),
    Library,
}

/// Identity of one user request to open a book.
///
/// The platform receives this before resolving the library or detecting the
/// format, so even an early failure supersedes every older request.
pub struct OpenRequest {
    generation: u64,
    locator: BookLocator,
}

/// Owns the one universal application window.
///
/// The browser entity remains alive while a publication reader temporarily
/// replaces it, so returning restores the exact library navigation and scroll
/// state. Platforms provide publication preparation, but do not own routing.
pub struct ApplicationRoot {
    browser: Entity<BrowserRoot>,
    reader: Option<ReaderSurface>,
    pending_open: Option<BookLocator>,
    open_request_generation: u64,
    application_title: SharedString,
}

impl ApplicationRoot {
    pub fn new(browser: Entity<BrowserRoot>, application_title: impl Into<SharedString>) -> Self {
        Self { browser, reader: None, pending_open: None, open_request_generation: 0, application_title: application_title.into() }
    }

    pub fn import_and_open_file(&mut self, file: SelectedFile, window: &mut Window, cx: &mut Context<Self>) {
        self.browser.update(cx, |browser, cx| browser.import_and_open_file(file, window, cx));
    }

    /// Starts the single request timeline before any fallible platform work.
    pub fn begin_open(&mut self, locator: BookLocator) -> OpenRequest {
        self.open_request_generation = self.open_request_generation.wrapping_add(1);
        self.pending_open = Some(locator.clone());
        OpenRequest { generation: self.open_request_generation, locator }
    }

    pub fn return_to_library_callback(&self, cx: &Context<Self>) -> ReturnToLibrary {
        let root = cx.entity().downgrade();
        Rc::new(move |window, cx| {
            let _ = root.update(cx, |root, cx| root.return_to_library(window, cx));
        })
    }

    /// Installs a reader page synchronously. Loading belongs to that page and
    /// does not delay the application-level navigation change.
    pub fn show_reader(&mut self, request: OpenRequest, view: AnyView, cx: &mut Context<Self>) {
        if self.open_request_generation != request.generation {
            return;
        }
        self.pending_open = None;
        self.reader = Some(ReaderSurface { locator: request.locator, view });
        cx.set_volume_button_capture(true);
        cx.set_keep_screen_awake(true);
        cx.notify();
    }

    /// Completes preparation of any readable format through the request
    /// timeline established by [`Self::begin_open`].
    pub fn open_book<T: 'static>(&mut self, request: OpenRequest, prepared: Task<Result<T, String>>, activate: impl FnOnce(T, ReturnToLibrary, Entity<BrowserRoot>, &mut Window, &mut App) -> OpenedBook + 'static, cx: &mut Context<Self>) {
        let OpenRequest { generation, locator } = request;
        let content_hash = locator.content_hash();
        cx.spawn(async move |root, cx| {
            let result = prepared.await;
            let _ = root.update_in(cx, move |root, window, cx| {
                if root.open_request_generation != generation {
                    return;
                }
                root.pending_open = None;
                match result {
                    Ok(prepared) => {
                        let weak_root = cx.entity().downgrade();
                        let return_to_library: ReturnToLibrary = Rc::new(move |window, cx| {
                            let _ = weak_root.update(cx, |root, cx| root.return_to_library(window, cx));
                        });
                        match activate(prepared, return_to_library, root.browser.clone(), window, cx) {
                            OpenedBook::Reader(view) => {
                                root.reader = Some(ReaderSurface { locator, view });
                                cx.set_volume_button_capture(true);
                                cx.set_keep_screen_awake(true);
                                cx.notify();
                            }
                            OpenedBook::Library => root.show_library(window, cx),
                        }
                    }
                    Err(error) => {
                        log::error!("cannot prepare book {content_hash}: {error}");
                        root.report_open_error(format!("Could not open book: {error}"), window, cx);
                    }
                }
            });
        })
        .detach();
    }

    /// Finishes a request that failed before an asynchronous preparation
    /// receiver could be installed.
    pub fn fail_open(&mut self, request: OpenRequest, error: impl Into<SharedString>, window: &mut Window, cx: &mut App) {
        if self.open_request_generation != request.generation {
            return;
        }
        self.pending_open = None;
        self.report_open_error(error, window, cx);
    }

    pub fn return_to_library(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_request_generation = self.open_request_generation.wrapping_add(1);
        self.pending_open = None;
        self.show_library(window, cx);
    }

    fn show_library(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.reader = None;
        cx.set_volume_button_capture(self.browser.read(cx).captures_volume_buttons(cx));
        cx.set_keep_screen_awake(false);
        cx.set_system_bars_visible(false);
        #[cfg(feature = "kobo")]
        if let Err(error) = gpui_kobo::request_experience_mode(gpui_kobo::KoboExperienceMode::LibraryFast) {
            log::error!("failed to return to Kobo library fast mode: {error}");
        }
        window.set_window_title(&self.application_title);
        self.browser.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    pub fn report_open_error(&self, error: impl Into<SharedString>, window: &mut Window, cx: &mut App) {
        window.push_notification(Notification::error(error.into()).id1::<Self>("book-open-error").autohide(false), cx);
    }

    pub fn handle_book_removed(&mut self, library_id: &LibraryId, content_hash: ContentHash, window: &mut Window, cx: &mut Context<Self>) {
        let pending_matches = self.pending_open.as_ref().is_some_and(|locator| locator.library_id() == library_id && locator.content_hash() == content_hash);
        let active_matches = self.reader.as_ref().is_some_and(|reader| reader.locator.library_id() == library_id && reader.locator.content_hash() == content_hash);
        if pending_matches {
            self.open_request_generation = self.open_request_generation.wrapping_add(1);
            self.pending_open = None;
            log::warn!("book {content_hash} disappeared while it was opening; cancelling the open request");
        }
        if active_matches {
            log::warn!("book {content_hash} disappeared while it was opening or being read; returning to the library");
            self.return_to_library(window, cx);
        }
        #[cfg(feature = "audiobooks")]
        if self.browser.read(cx).audiobook_matches(library_id, content_hash) {
            log::warn!("audiobook {content_hash} disappeared while it was open; closing the player");
            self.browser.update(cx, |browser, cx| browser.dismiss_audiobook(cx));
        }
    }

    fn on_back(&mut self, _: &DismissSettings, window: &mut Window, cx: &mut Context<Self>) {
        if self.reader.is_some() {
            self.return_to_library(window, cx);
        } else {
            cx.propagate();
        }
    }
}

impl Render for ApplicationRoot {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = self.reader.as_ref().map(|reader| reader.view.clone()).unwrap_or_else(|| AnyView::from(self.browser.clone()));
        gpui::div().size_full().key_context("Application").on_action(cx.listener(Self::on_back)).child(content)
    }
}
