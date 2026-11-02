//! Reader page lifecycle inside the application's single window.

use std::rc::Rc;
use std::sync::Arc;

use app::Failure;
use book_content::comic::{ComicBook, ComicReader};
use book_content::{Book, BookFormat as BookSourceFormat};
use book_model::{AnnotationAnchor, PdfAnnotationAnchor, ReadingPosition};
use gpui::prelude::*;
use gpui::{AnyView, App, AppContext, BackgroundExecutor, Context, Entity, EventEmitter, FocusHandle, IntoElement, Render, SharedString, Task, Window};
use gpui_component::{WindowExt, notification::Notification};
use library_backend::{BackendError, BookFormat, BoxedBookReader, DocumentSource, LibraryClient, ResolvedBook};
use library_model::BookLocator;
use html_view_gpui::ChapterPreparationWorker;
use skrift_column_paginator::{RendererInitialConfig, RendererPreparation};

use crate::book::NativeBook;
use crate::cjk::CjkFont;
use crate::comic::ComicReaderView;
use crate::epub::ReaderView as EpubReaderView;
use crate::opening_error::opening_error;
use crate::opening_recovery::OpeningRecovery;
use crate::pdf::PdfReaderView;
use crate::settings::{ReaderPreferences, ReaderSettings, SaveReaderSettings};
#[cfg(all(any(feature = "native-tts", target_arch = "wasm32"), not(feature = "kobo")))]
use crate::shell::tts::TtsReader;

use crate::CloseReader;

pub struct CloseRequested;
/// Preserve the failure classification through both reader and shell events.
pub struct BookOpenFailed(pub BackendError);
pub struct AudiobookOpened(pub gpui::Entity<audiobook_player::PlaybackSession>);

enum PreparedBook {
    Book { book: Arc<NativeBook>, cjk_font: Option<CjkFont>, preparation: Box<RendererPreparation>, recovery: OpeningRecovery },
    Pdf { metadata: Option<pdf_reader_core::PdfReaderMetadata>, reader: BoxedBookReader, initial_page: usize, initial_full_page_position: f32, initial_annotation: Option<PdfAnnotationAnchor>, recovery: OpeningRecovery },
    Audiobook { resolved: ResolvedBook, initial_target: Option<String> },
    Comic { reader: ComicReader, initial_page: usize, offset: f32, recovery: OpeningRecovery },
}

enum ReaderState {
    Loading,
    Ready(AnyView),
    Error(SharedString),
}

/// The reader page installed directly in the application's single window.
/// It owns book loading and replaces its loading state with the
/// format-specific reader when a document is ready. Audiobooks hand their
/// playback session to the application shell instead.
pub struct ReaderView {
    title: SharedString,
    state: ReaderState,
    pending_annotation: Option<AnnotationAnchor>,
    #[cfg(feature = "kobo")]
    device_controls: Option<AnyView>,
    focus: FocusHandle,
    _load_task: Task<()>,
}

pub fn configure(preferences: ReaderPreferences, save: SaveReaderSettings, cx: &mut App) {
    let loader = ui_components::bundled_font_loader(cx);
    html_view_gpui::configure_font_loader(loader, cx);
    cx.set_global(ReaderSettings { preferences, save });
}

async fn prepare_pdf(locator: &BookLocator, content: ResolvedBook, library: &LibraryClient, initial_target: Option<String>) -> Result<PreparedBook, BackendError> {
    let content_hash = locator.content_hash();
    let initial_annotation = initial_target.as_deref().and_then(AnnotationAnchor::from_open_target).and_then(|anchor| match anchor {
        AnnotationAnchor::Pdf(anchor) => Some(anchor),
        _ => None,
    });
    let requested_page = initial_annotation.as_ref().map(|anchor| anchor.page_index as usize).or_else(|| initial_target.as_deref().and_then(book_model::pdf_toc_page));
    let mut recovery = OpeningRecovery::default();
    let (stored_page, stored_position) = recovery.restore_position(library.pdf_reading_position(content_hash).await, requested_page.is_some()).unwrap_or((0, 0.0));
    // A requested destination outranks the stored position: it is the reason
    // the book was opened. A contents row asks for `pdf-page:<n>`; anything
    // else is a target for another format and is ignored rather than guessed at.
    let (initial_page, initial_full_page_position) = match requested_page {
        Some(page) => (page as u32, 0.0),
        None => (stored_page, stored_position),
    };
    let document = content.into_document().map_err(|error| opening_error(BackendError::message(error), "book_source", Failure::BookFileUnavailable))?;
    let reader = document.source.into_reader().map_err(|error| opening_error(BackendError::message(error), "book_source", Failure::BookFileUnavailable))?;
    Ok(PreparedBook::Pdf { metadata: document.pdf_metadata, reader, initial_page: initial_page as usize, initial_full_page_position, initial_annotation, recovery })
}

async fn prepare_book(
    locator: &BookLocator, content: ResolvedBook, format: BookFormat, library: &LibraryClient, executor: &BackgroundExecutor, chapter_preparation: &ChapterPreparationWorker, mut renderer_config: RendererInitialConfig, preferences: &ReaderPreferences, initial_target: Option<String>,
) -> Result<PreparedBook, BackendError> {
    let book_format = match format {
        BookFormat::Epub => BookSourceFormat::Epub,
        BookFormat::Mobi => BookSourceFormat::Mobi,
        BookFormat::Fb2 => BookSourceFormat::Fb2,
        _ => return Err(format!("stage=prepare: {} is not a book format", format.canonical_extension()).into()),
    };
    let content = content.into_document().map_err(|error| opening_error(BackendError::message(error), "book_source", Failure::BookFileUnavailable))?;
    let text_target = initial_target.as_deref().and_then(book_model::BookTextTarget::from_open_target);
    let is_text_search = text_target.is_some();
    let (book, cjk_font, text_cfi) = executor
        .spawn(async move {
            let book = match content.source {
                DocumentSource::Async(source) => Book::open(book_format, move || source.reader()).await,
                DocumentSource::Reader(reader) => Book::open_reader(book_format, reader),
            }
            .map_err(|error| opening_error(BackendError::operation(error), "book_open", Failure::InvalidBookContainer))?;
            let reading = book.prepare_reading().await.map_err(|error| opening_error(BackendError::operation(error), "content_prepare", Failure::InvalidBookContent))?;
            let book = NativeBook::new(reading);
            let cjk_font = CjkFont::for_book(&book);
            let text_cfi = text_target.as_ref().map(|target| book.text_search_cfi(target).ok_or_else(|| BackendError::message("text search location could not be resolved"))).transpose()?;
            Ok::<_, BackendError>((Arc::new(book), cjk_font, text_cfi))
        })
        .await?;
    preferences.apply_cjk_font(&mut renderer_config.style_overrides, cjk_font);
    let content_hash = locator.content_hash();
    let mut recovery = OpeningRecovery::default();
    let saved_position = library.reading_position(content_hash).await.and_then(|stored| stored.map(|stored| book.restore_cfi(&stored).ok_or_else(|| BackendError::message("saved EPUB location could not be resolved"))).transpose());
    let requested_annotation = initial_target.as_deref().and_then(AnnotationAnchor::from_open_target);
    let requested_cfi = text_cfi.or(requested_annotation.as_ref().map(|anchor| book.annotation_cfi(anchor).ok_or_else(|| BackendError::message("annotation location could not be resolved"))).transpose()?);
    let nav_state = requested_cfi.or_else(|| recovery.restore_position(saved_position, initial_target.is_some()));
    let initial_target = if requested_annotation.is_some() || is_text_search { None } else { initial_target };
    renderer_config.publication_cfi_bases = book.publication_cfi_bases.clone();
    let image_executor = executor.clone();
    let image_loader = book.image_loader(move |task| {
        image_executor.spawn(task).detach();
    });
    let prepare_book = book.clone();
    let (preparation, recovery) = chapter_preparation
        .prepare(move || {
            recovery
                .prepare_document(prepare_book.provider.clone(), prepare_book.document_uris(), nav_state.as_deref(), initial_target.as_deref(), renderer_config)
                .map(|prepared| (prepared.with_image_loader(image_loader), recovery))
                .map_err(|error| opening_error(BackendError::message(error), "document_prepare", Failure::BookDocumentFailed))
        })
        .recv()
        .await
        .map_err(|error| opening_error(BackendError::message(error.to_string()), "document_prepare", Failure::BookDocumentFailed))??;
    Ok(PreparedBook::Book { book, cjk_font, preparation: Box::new(preparation), recovery })
}

async fn load_book(
    locator: &BookLocator, library: &LibraryClient, executor: &BackgroundExecutor, chapter_preparation: &ChapterPreparationWorker, renderer_config: RendererInitialConfig, preferences: &ReaderPreferences, initial_target: Option<String>,
) -> Result<PreparedBook, BackendError> {
    let content = library.resolve_book(locator.content_hash()).await.map_err(|error| opening_error(error, "book_resolve", Failure::BookFileUnavailable))?;
    let format = content.format();
    match format {
        BookFormat::Pdf => prepare_pdf(locator, content, library, initial_target).await,
        BookFormat::M4b | BookFormat::Mp3Folder => Ok(PreparedBook::Audiobook { resolved: content, initial_target }),
        BookFormat::Epub | BookFormat::Mobi | BookFormat::Fb2 => prepare_book(locator, content, format, library, executor, chapter_preparation, renderer_config, preferences, initial_target).await,
        BookFormat::Cbz => {
            let mut recovery = OpeningRecovery::default();
            let stored = recovery.restore_position(library.reading_position(locator.content_hash()).await, initial_target.as_deref().and_then(book_model::cbz_toc_page).is_some());
            let (initial_page, offset) = match stored.as_deref().and_then(|value| ReadingPosition::parse(value).ok()) {
                Some(ReadingPosition::Cbz { page, full_page_position }) => (page as usize, full_page_position),
                _ => (0, 0.0),
            };
            let (initial_page, offset) = initial_target.as_deref().and_then(book_model::cbz_toc_page).map(|page| (page, 0.0)).unwrap_or((initial_page, offset));
            let source = content.into_document().map_err(|error| opening_error(BackendError::message(error), "book_source", Failure::BookFileUnavailable))?.source;
            let reader = executor
                .spawn(async move {
                    let book = match source {
                        DocumentSource::Async(source) if source.supports_prefetch() => {
                            let prefetch = source.clone();
                            ComicBook::open_with_prefetch(
                                move || source.reader(),
                                initial_page,
                                move |ranges| {
                                    let source = prefetch.clone();
                                    async move { source.prefetch(ranges).await }
                                },
                            )
                            .await
                        }
                        DocumentSource::Async(source) => ComicBook::open(move || source.reader()).await,
                        DocumentSource::Reader(reader) => ComicBook::open_reader(reader),
                    }
                    .map_err(|error| opening_error(BackendError::operation(error), "comic_open", Failure::InvalidBookContainer))?;
                    book.into_reader().await.map_err(|error| opening_error(BackendError::operation(error), "comic_prepare", Failure::InvalidBookContent))
                })
                .await?;
            Ok(PreparedBook::Comic { reader, initial_page, offset, recovery })
        }
    }
}

impl ReaderView {
    pub fn open_annotation_anchor(&mut self, anchor: AnnotationAnchor, window: &mut Window, cx: &mut Context<Self>) -> Result<(), String> {
        match &self.state {
            ReaderState::Loading => {
                self.pending_annotation = Some(anchor);
                self.focus.focus(window, cx);
                Ok(())
            }
            ReaderState::Ready(view) => match view.clone().downcast::<EpubReaderView>() {
                Ok(reader) => reader.update(cx, |reader, cx| reader.open_annotation_anchor(&anchor, window, cx)),
                Err(view) => {
                    let reader = view.downcast::<PdfReaderView>().map_err(|_| "This reader cannot open the linked passage.".to_owned())?;
                    let anchor = anchor.pdf_anchor().cloned().ok_or_else(|| "The linked passage does not refer to a PDF location.".to_owned())?;
                    reader.update(cx, |reader, cx| reader.follow_annotation_link(anchor, window, cx));
                    Ok(())
                }
            },
            ReaderState::Error(_) => Err("This reader could not load the book. Reopen the book to follow this link.".to_owned()),
        }
    }

    /// Reuse the shell's device controls in this reader's Settings header.
    #[cfg(feature = "kobo")]
    pub fn with_device_controls(mut self, controls: impl Into<AnyView>) -> Self {
        self.device_controls = Some(controls.into());
        self
    }

    /// Returns whether an active narration session handled the playback key.
    #[cfg(all(any(feature = "native-tts", target_arch = "wasm32"), not(feature = "kobo")))]
    pub fn toggle_narration(&mut self, cx: &mut Context<Self>) -> bool {
        let ReaderState::Ready(view) = &self.state else { return false };
        match view.clone().downcast::<EpubReaderView>() {
            Ok(reader) => reader.update(cx, |reader, cx| reader.toggle_active_narration(cx)),
            Err(view) => view.downcast::<PdfReaderView>().is_ok_and(|reader| reader.update(cx, |reader, cx| reader.toggle_active_narration(cx))),
        }
    }

    #[cfg(not(all(any(feature = "native-tts", target_arch = "wasm32"), not(feature = "kobo"))))]
    pub fn toggle_narration(&mut self, _: &mut Context<Self>) -> bool {
        false
    }

    pub fn native_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let ReaderState::Ready(view) = &self.state {
            let view = view.clone();
            // Closing a format view calls back into this lifecycle owner.
            // Release its current lease before running that transition.
            window.defer(cx, move |window, cx| match view.downcast::<EpubReaderView>() {
                Ok(reader) => reader.update(cx, |reader, cx| reader.native_back(window, cx)),
                Err(view) => match view.downcast::<PdfReaderView>() {
                    Ok(reader) => reader.update(cx, |reader, cx| reader.native_back(window, cx)),
                    Err(view) => {
                        if let Ok(reader) = view.downcast::<ComicReaderView>() {
                            reader.update(cx, |reader, cx| reader.native_back(window, cx));
                        }
                    }
                },
            });
        } else {
            self.request_close(cx);
        }
    }

    fn request_close(&mut self, cx: &mut Context<Self>) {
        cx.emit(CloseRequested);
    }

    fn opening_failed(&mut self, error: BackendError, cx: &mut Context<Self>) {
        log::error!("cannot load reader page for {:?}: {error}", self.title);
        self.state = ReaderState::Error(ui_components::user_facing_error(&error, "Could not open this book. Try opening it again, or download another copy.").into());
        cx.emit(BookOpenFailed(error));
        cx.notify();
    }

    fn watch_open_failure<T: EventEmitter<BookOpenFailed> + 'static>(&self, reader: &Entity<T>, cx: &mut Context<Self>) {
        cx.subscribe(reader, |this, _, error: &BookOpenFailed, cx| this.opening_failed(error.0.clone(), cx)).detach();
    }

    pub fn new(locator: BookLocator, title: String, initial_target: Option<String>, app: app::AppClient, library: Rc<LibraryClient>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        window.set_window_title(&title);
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        if ui_components::uses_mobile_reader_controls(window) {
            cx.set_system_bars_visible(true);
        }
        let reader_view = cx.entity().downgrade();
        let close_reader: CloseReader = Rc::new(move |_, cx| {
            let _ = reader_view.update(cx, Self::request_close);
        });
        let reader_title = title.clone().into();
        let executor = cx.background_executor().clone();
        let chapter_preparation = ChapterPreparationWorker::for_app(cx);
        let renderer_config = crate::epub::state::renderer_initial_config(cx);
        let preferences = ReaderSettings::preferences(cx).clone();
        let fonts_ready = cx.try_global::<crate::SystemFontsLoading>().map(|loading| loading.0.clone());
        let load_task = cx.spawn(async move |reader_view, cx| {
            let (locator, result) = match library.resolve_book_identity(locator.content_hash()).await {
                Ok(identity) => {
                    let locator = BookLocator::new(*locator.library_id(), identity);
                    let result = load_book(&locator, &library, &executor, &chapter_preparation, renderer_config, &preferences, initial_target).await;
                    (locator, result)
                }
                Err(error) => (locator, Err(error)),
            };
            // Preparing book data can run alongside font registration. Only
            // attaching its GPUI shaper needs the completed font catalog.
            if matches!(&result, Ok(PreparedBook::Book { .. })) {
                if let Some(fonts_ready) = fonts_ready {
                    let _ = fonts_ready.recv().await;
                }
                #[cfg(target_arch = "wasm32")]
                if let Err(error) = ui_components::ensure_cjk_fonts(cx).await {
                    log::error!("failed to install CJK glyph fallback: {error}");
                }
            }
            match result {
                Ok(book) => {
                    let _ = reader_view.update_in(cx, move |reader_view, window, cx| {
                        let recovery = match &book {
                            PreparedBook::Book { recovery, .. } | PreparedBook::Pdf { recovery, .. } | PreparedBook::Comic { recovery, .. } => Some(recovery),
                            PreparedBook::Audiobook { .. } => None,
                        };
                        let save_progress = recovery.is_none_or(OpeningRecovery::save_progress);
                        window.remove_notification1::<Self>("book-opening-recovered", cx);
                        if let Some(message) = recovery.and_then(OpeningRecovery::notice) {
                            window.push_notification(Notification::warning(message).id1::<Self>("book-opening-recovered").autohide(false).close_button_always_visible(true), cx);
                        }
                        #[cfg(feature = "kobo")]
                        let device_controls = reader_view.device_controls.clone();
                        let view: AnyView = match book {
                            PreparedBook::Pdf { reader, metadata, initial_page, initial_full_page_position, initial_annotation, .. } => {
                                let reader = cx.new(|cx| {
                                    PdfReaderView::new(
                                        locator,
                                        title,
                                        app,
                                        library,
                                        reader,
                                        metadata,
                                        initial_page,
                                        initial_full_page_position,
                                        save_progress,
                                        close_reader,
                                        #[cfg(feature = "kobo")]
                                        device_controls,
                                        window,
                                        cx,
                                    )
                                });
                                if let Some(anchor) = initial_annotation {
                                    reader.update(cx, |reader, cx| reader.open_annotation_anchor(anchor, cx));
                                }
                                reader_view.watch_open_failure(&reader, cx);
                                reader.into()
                            }
                            PreparedBook::Book { book, cjk_font, preparation, .. } => {
                                let reader = cx.new(|cx| {
                                    EpubReaderView::new(
                                        locator,
                                        title,
                                        app,
                                        library,
                                        book,
                                        cjk_font,
                                        *preparation,
                                        save_progress,
                                        close_reader,
                                        #[cfg(feature = "kobo")]
                                        device_controls,
                                        window,
                                        cx,
                                    )
                                });
                                reader_view.watch_open_failure(&reader, cx);
                                reader.into()
                            }
                            PreparedBook::Audiobook { resolved, initial_target } => {
                                let session = crate::open_audiobook(locator, library.as_ref().clone(), resolved, initial_target, cx);
                                cx.emit(AudiobookOpened(session));
                                return;
                            }
                            PreparedBook::Comic { reader, initial_page, offset, .. } => cx
                                .new(|cx| {
                                    ComicReaderView::new(
                                        locator,
                                        title,
                                        reader,
                                        initial_page,
                                        offset,
                                        save_progress,
                                        app,
                                        library,
                                        close_reader,
                                        #[cfg(feature = "kobo")]
                                        device_controls,
                                        window,
                                        cx,
                                    )
                                })
                                .into(),
                        };
                        reader_view.state = ReaderState::Ready(view);
                        if let Some(anchor) = reader_view.pending_annotation.take() {
                            if let Err(message) = reader_view.open_annotation_anchor(anchor, window, cx) {
                                window.push_notification(Notification::error(message), cx);
                            }
                        }
                        cx.notify();
                    });
                }
                Err(error) => {
                    let _ = reader_view.update(cx, move |reader_view, cx| reader_view.opening_failed(error, cx));
                }
            }
        });
        #[cfg(feature = "kobo")]
        if let Err(error) = gpui_kobo::request_experience_mode(gpui_kobo::KoboExperienceMode::ReaderQuality) {
            log::error!("failed to enter Kobo reader quality mode: {error}");
        }
        Self {
            title: reader_title,
            state: ReaderState::Loading,
            pending_annotation: None,
            #[cfg(feature = "kobo")]
            device_controls: None,
            focus,
            _load_task: load_task,
        }
    }
}

impl EventEmitter<CloseRequested> for ReaderView {}
impl EventEmitter<BookOpenFailed> for ReaderView {}
impl EventEmitter<BookOpenFailed> for EpubReaderView {}
impl EventEmitter<BookOpenFailed> for PdfReaderView {}
impl EventEmitter<AudiobookOpened> for ReaderView {}

impl Render for ReaderView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        ui_components::ensure_text_fonts(&self.title, cx);
        let message = match &self.state {
            ReaderState::Loading => SharedString::from("Loading book…"),
            ReaderState::Ready(view) => return view.clone().into_any_element(),
            ReaderState::Error(error) => error.clone(),
        };
        let theme = ui_components::browser_theme(cx);
        let content = gpui::div()
            .size_full()
            .flex()
            .flex_col()
            .gap(gpui::rems(1.0))
            .items_center()
            .justify_center()
            .bg(theme.page_bg)
            .text_color(theme.text)
            .child(message)
            .child(ui_components::outlined_text_button("cancel-reader-loading", "Back").on_click(cx.listener(|reader, _, _, cx| reader.request_close(cx))))
            .into_any_element();
        let content = if ui_components::uses_mobile_reader_controls(window) {
            let back = Some(ui_components::mobile_back_button().on_click(cx.listener(|reader, _, _, cx| reader.request_close(cx))));
            gpui::div()
                .relative()
                .size_full()
                .child(content)
                .child(gpui::div().absolute().top_0().left_0().right_0().pt(window.insets().safe_area.top).bg(theme.page_bg).child(ui_components::mobile_navigation_bar(self.title.clone(), back, theme)))
                .into_any_element()
        } else {
            content
        };
        gpui::div()
            .size_full()
            .key_context("Reader")
            .track_focus(&self.focus)
            .on_action(cx.listener(|reader, _: &crate::ReturnToLibrary, _, cx| reader.request_close(cx)))
            .on_action(cx.listener(|reader, _: &crate::CloseSearch, _, cx| reader.request_close(cx)))
            .child(content)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;
    use std::cell::Cell;

    #[gpui::test]
    fn native_back_and_escape_work_before_the_document_is_ready(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            ui_components::register_fonts(cx).unwrap();
            crate::bind_keys(cx);
        });
        let (reader, cx) = cx.add_window_view(|window, cx| {
            let focus = cx.focus_handle();
            focus.focus(window, cx);
            ReaderView {
                title: "Loading test".into(),
                state: ReaderState::Loading,
                pending_annotation: None,
                #[cfg(feature = "kobo")]
                device_controls: None,
                focus,
                _load_task: cx.spawn(async |_, _| std::future::pending::<()>().await),
            }
        });
        let closed = Rc::new(Cell::new(0));
        let count = closed.clone();
        let _subscription = cx.update(|_, cx| cx.subscribe(&reader, move |_, _: &CloseRequested, _| count.set(count.get() + 1)));
        cx.run_until_parked();
        cx.simulate_keystrokes("back");
        cx.run_until_parked();
        assert_eq!(closed.get(), 1);
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert_eq!(closed.get(), 2);
    }
    #[gpui::test]
    fn reader_link_during_loading_keeps_the_reader_and_uses_the_latest_destination(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            ui_components::register_fonts(cx).unwrap();
        });
        let window = cx.add_window(|_, cx| ReaderView {
            title: "Loading linked book".into(),
            state: ReaderState::Loading,
            pending_annotation: None,
            #[cfg(feature = "kobo")]
            device_controls: None,
            focus: cx.focus_handle(),
            _load_task: cx.spawn(async |_, _| std::future::pending::<()>().await),
        });
        let first = AnnotationAnchor::epub_cfi("epubcfi(/6/2!/4/2/1:0)");
        let latest = AnnotationAnchor::epub_cfi("epubcfi(/6/4!/4/2/1:0)");
        window
            .update(cx, |reader, window, cx| {
                reader.open_annotation_anchor(first, window, cx).unwrap();
                reader.open_annotation_anchor(latest.clone(), window, cx).unwrap();
                assert!(matches!(reader.state, ReaderState::Loading));
                assert_eq!(reader.pending_annotation.as_ref(), Some(&latest));
                assert!(reader.focus.is_focused(window));
            })
            .unwrap();
    }
}
