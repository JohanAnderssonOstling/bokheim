//! Reader page lifecycle inside the application's single window.

use std::rc::Rc;
use std::sync::Arc;

use book_content::{Book, BookFormat as BookSourceFormat};
use book_content::comic::{ComicBook, ComicReader};
use book_model::ReadingPosition;
use gpui::prelude::*;
use gpui::{AnyView, App, AppContext, BackgroundExecutor, Context, Entity, EventEmitter, FocusHandle, IntoElement, Render, SharedString, Task, Window};
use html_view_core::{RendererInitialConfig, RendererPreparation};
use library_backend::{BackendError, BookFormat, BoxedBookReader, DocumentSource, LibraryClient, ResolvedBook};
use library_model::BookLocator;

use crate::book::NativeBook;
use crate::comic::ComicReaderView;
use crate::epub::ReaderView as EpubReaderView;
use crate::pdf::PdfReaderView;
use crate::settings::{ReaderPreferences, ReaderSettings, SaveReaderSettings};

use crate::CloseReader;

pub struct CloseRequested;
pub struct BookOpenFailed(pub String);
pub struct AudiobookOpened(pub gpui::Entity<audiobook_player::PlaybackSession>);

enum PreparedBook {
    Book { book: Arc<NativeBook>, preparation: Box<RendererPreparation> },
    Pdf { page_source: Option<Arc<dyn pdf_reader_core::PdfPageSource>>, metadata: Option<pdf_reader_core::PdfReaderMetadata>, reader: BoxedBookReader, initial_page: usize, initial_full_page_position: f32 },
    Audiobook { resolved: ResolvedBook, initial_target: Option<String> },
    Comic { reader: ComicReader, initial_page: usize, offset: f32 },
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
    focus: FocusHandle,
    _load_task: Task<()>,
}

pub fn configure(preferences: ReaderPreferences, save: SaveReaderSettings, cx: &mut App) {
    if let Err(error) = ui_components::register_fonts(cx) {
        log::warn!("failed to install reader fonts: {error}");
    }
    cx.set_global(ReaderSettings { preferences, save });
}

async fn prepare_pdf(locator: &BookLocator, content: ResolvedBook, library: &LibraryClient, initial_target: Option<String>) -> Result<PreparedBook, BackendError> {
    let content_hash = locator.content_hash();
    let (stored_page, stored_position) = library.pdf_reading_position(content_hash).await.map_err(|error| error.context("stage=reading_position"))?.unwrap_or((0, 0.0));
    // A requested destination outranks the stored position: it is the reason
    // the book was opened. A contents row asks for `pdf-page:<n>`; anything
    // else is a target for another format and is ignored rather than guessed at.
    let (initial_page, initial_full_page_position) = match initial_target.as_deref().and_then(book_model::pdf_toc_page) {
        Some(page) => (page as u32, 0.0),
        None => (stored_page, stored_position),
    };
    let document = content.into_document()?;
    Ok(PreparedBook::Pdf { page_source: document.pdf_page_source, metadata: document.pdf_metadata, reader: document.source.into_reader()?, initial_page: initial_page as usize, initial_full_page_position })
}

async fn prepare_book(
    locator: &BookLocator, content: ResolvedBook, format: BookFormat, library: &LibraryClient, executor: &BackgroundExecutor, renderer_config: RendererInitialConfig, initial_target: Option<String>,
) -> Result<PreparedBook, BackendError> {
    let book_format = match format {
        BookFormat::Epub => BookSourceFormat::Epub,
        BookFormat::Mobi => BookSourceFormat::Mobi,
        BookFormat::Fb2 => BookSourceFormat::Fb2,
        _ => return Err(format!("stage=prepare: {} is not a book format", format.canonical_extension()).into()),
    };
    let content = content.into_document()?;
    let book = executor
        .spawn(async move {
            match content.source {
                DocumentSource::Async(source) => match Book::open(book_format, move || source.reader()).await {
                    Ok(book) => book.prepare_reading().await.map(NativeBook::new),
                    Err(error) => Err(error),
                },
                DocumentSource::Reader(reader) => NativeBook::from_reader(book_format, reader),
            }
            .map_err(|error| format!("stage=book_open: {error}"))
        })
        .await?;
    let content_hash = locator.content_hash();
    let nav_state = library.reading_position(content_hash).await.map_err(|error| error.context("stage=reading_position"))?;
    let book = Arc::new(book);
    let image_executor = executor.clone();
    let image_loader = book.image_loader(move |task| {
        image_executor.spawn(task).detach();
    });
    let prepare_book = book.clone();
    let preparation = executor
        .spawn(async move {
            RendererPreparation::from_provider_with_nav_and_target(prepare_book.provider.clone(), prepare_book.document_uris().to_vec(), 0, nav_state.as_deref(), initial_target.as_deref(), renderer_config)
                .map(|prepared| prepared.with_image_loader(image_loader))
                .map_err(|error| format!("stage=document_prepare: {error}"))
        })
        .await?;
    Ok(PreparedBook::Book { book, preparation: Box::new(preparation) })
}

async fn load_book(locator: &BookLocator, library: &LibraryClient, executor: &BackgroundExecutor, renderer_config: RendererInitialConfig, initial_target: Option<String>) -> Result<PreparedBook, BackendError> {
    let content = library.resolve_book(locator.content_hash()).await.map_err(|error| error.context("stage=book_resolve"))?;
    let format = content.format();
    match format {
        BookFormat::Pdf => prepare_pdf(locator, content, library, initial_target).await,
        BookFormat::M4b | BookFormat::Mp3Folder => Ok(PreparedBook::Audiobook { resolved: content, initial_target }),
        BookFormat::Epub | BookFormat::Mobi | BookFormat::Fb2 => prepare_book(locator, content, format, library, executor, renderer_config, initial_target).await,
        BookFormat::Cbz => {
            let stored = library.reading_position(locator.content_hash()).await?;
            let (initial_page, offset) = match stored.as_deref().and_then(|value| ReadingPosition::parse(value).ok()) {
                Some(ReadingPosition::Cbz { page, full_page_position }) => (page as usize, full_page_position),
                _ => (0, 0.0),
            };
            let (initial_page, offset) = initial_target.as_deref().and_then(book_model::cbz_toc_page).map(|page| (page, 0.0)).unwrap_or((initial_page, offset));
            let source = content.into_document()?.source;
            let reader = executor.spawn(async move {
                let book = match source {
                    DocumentSource::Async(source) => ComicBook::open(move || source.reader()).await?,
                    DocumentSource::Reader(reader) => ComicBook::open_reader(reader)?,
                };
                book.into_reader().await
            }).await.map_err(BackendError::operation)?;
            Ok(PreparedBook::Comic { reader, initial_page, offset })
        }
    }
}

impl ReaderView {
    pub fn native_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let ReaderState::Ready(view) = &self.state {
            let view = view.clone();
            // Closing a format view calls back into this lifecycle owner.
            // Release its current lease before running that transition.
            window.defer(cx, move |window, cx| match view.downcast::<EpubReaderView>() {
                Ok(reader) => reader.update(cx, |reader, cx| reader.native_back(window, cx)),
                Err(view) => {
                    match view.downcast::<PdfReaderView>() {
                        Ok(reader) => reader.update(cx, |reader, cx| reader.native_back(window, cx)),
                        Err(view) => if let Ok(reader) = view.downcast::<ComicReaderView>() { reader.update(cx, |reader, cx| reader.native_back(window, cx)); },
                    }
                }
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
        cx.emit(BookOpenFailed(error.to_string()));
        cx.notify();
    }

    fn watch_open_failure<T: EventEmitter<BookOpenFailed> + 'static>(&self, reader: &Entity<T>, cx: &mut Context<Self>) {
        // Format readers report their own failures as text; they stay unclassified.
        cx.subscribe(reader, |this, _, error: &BookOpenFailed, cx| this.opening_failed(BackendError::message(error.0.clone()), cx)).detach();
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
        let renderer_config = crate::epub::state::renderer_initial_config(cx);
        let load_task = cx.spawn(async move |reader_view, cx| {
            let result = load_book(&locator, &library, &executor, renderer_config, initial_target).await;
            match result {
                Ok(book) => {
                    let _ = reader_view.update_in(cx, move |reader_view, window, cx| {
                        let view: AnyView = match book {
                            PreparedBook::Pdf { reader, metadata, page_source, initial_page, initial_full_page_position } => {
                                let reader = cx.new(|cx| PdfReaderView::new(locator, title, app, library, reader, metadata, page_source, initial_page, initial_full_page_position, close_reader, window, cx));
                                reader_view.watch_open_failure(&reader, cx);
                                reader.into()
                            }
                            PreparedBook::Book { book, preparation } => {
                                let reader = cx.new(|cx| EpubReaderView::new(locator, title, app, library, book, *preparation, close_reader, window, cx));
                                reader_view.watch_open_failure(&reader, cx);
                                reader.into()
                            }
                            PreparedBook::Audiobook { resolved, initial_target } => {
                                let session = crate::open_audiobook(locator, library.as_ref().clone(), resolved, initial_target, cx);
                                cx.emit(AudiobookOpened(session));
                                return;
                            }
                            PreparedBook::Comic { reader, initial_page, offset } => {
                                cx.new(|cx| ComicReaderView::new(locator, title, reader, initial_page, offset, app, library, close_reader, window, cx)).into()
                            }
                        };
                        reader_view.state = ReaderState::Ready(view);
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
        Self { title: reader_title, state: ReaderState::Loading, focus, _load_task: load_task }
    }
}

impl EventEmitter<CloseRequested> for ReaderView {}
impl EventEmitter<BookOpenFailed> for ReaderView {}
impl EventEmitter<BookOpenFailed> for EpubReaderView {}
impl EventEmitter<BookOpenFailed> for PdfReaderView {}
impl EventEmitter<AudiobookOpened> for ReaderView {}

impl Render for ReaderView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
            let back = Some(ui_components::mobile_back_button(theme).on_click(cx.listener(|reader, _, _, cx| reader.request_close(cx))));
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
            ReaderView { title: "Loading test".into(), state: ReaderState::Loading, focus, _load_task: cx.spawn(async |_, _| std::future::pending::<()>().await) }
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
}
