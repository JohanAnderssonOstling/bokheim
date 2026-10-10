//! Reader page lifecycle inside the application's single window.

use std::rc::Rc;
use std::sync::Arc;

use book_content::{Book, BookFormat as BookSourceFormat};
use gpui::prelude::*;
use gpui::{AnyView, App, AppContext, BackgroundExecutor, Context, EventEmitter, IntoElement, Render, SharedString, Task, Window};
use html_view_core::{RendererInitialConfig, RendererPreparation};
use library_backend::{BookFormat, BoxedBookReader, DocumentSource, LibraryClient, ResolvedBook};
use library_model::BookLocator;

use crate::book::NativeBook;
use crate::epub::ReaderView as EpubReaderView;
use crate::pdf::PdfReaderView;
use crate::settings::{ReaderPreferences, ReaderSettings, SaveReaderSettings};

use crate::CloseReader;

pub struct CloseRequested;
pub struct AudiobookOpened(pub gpui::Entity<audiobook_player::PlaybackSession>);

enum PreparedBook {
    Book { book: Arc<NativeBook>, preparation: Box<RendererPreparation> },
    Pdf { page_source: Option<Arc<dyn pdf_reader_core::PdfPageSource>>, metadata: Option<pdf_reader_core::PdfReaderMetadata>, reader: BoxedBookReader, initial_page: usize, initial_full_page_position: f32 },
    Audiobook { resolved: ResolvedBook, initial_target: Option<String> },
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
    _load_task: Task<()>,
}

pub fn configure(preferences: ReaderPreferences, save: SaveReaderSettings, cx: &mut App) {
    cx.set_global(ReaderSettings { preferences, save });
}

async fn prepare_pdf(locator: &BookLocator, content: ResolvedBook, library: &LibraryClient, initial_target: Option<String>) -> Result<PreparedBook, String> {
    let content_hash = locator.content_hash();
    let (stored_page, stored_position) = library.pdf_reading_position(content_hash).await.map_err(|error| format!("stage=reading_position: {error}"))?.unwrap_or((0, 0.0));
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
) -> Result<PreparedBook, String> {
    let book_format = match format {
        BookFormat::Epub => BookSourceFormat::Epub,
        BookFormat::Mobi => BookSourceFormat::Mobi,
        _ => return Err(format!("stage=prepare: {} is not a book format", format.canonical_extension())),
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
    let nav_state = library.reading_position(content_hash).await.map_err(|error| format!("stage=reading_position: {error}"))?;
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

async fn load_book(locator: &BookLocator, library: &LibraryClient, executor: &BackgroundExecutor, renderer_config: RendererInitialConfig, initial_target: Option<String>) -> Result<PreparedBook, String> {
    let content = library.resolve_book(locator.content_hash()).await.map_err(|error| format!("stage=book_resolve: {error}"))?;
    let format = content.format();
    match format {
        BookFormat::Pdf => prepare_pdf(locator, content, library, initial_target).await,
        BookFormat::M4b | BookFormat::Mp3Folder => Ok(PreparedBook::Audiobook { resolved: content, initial_target }),
        BookFormat::Epub | BookFormat::Mobi => prepare_book(locator, content, format, library, executor, renderer_config, initial_target).await,
    }
}

impl ReaderView {
    fn request_close(&mut self, cx: &mut Context<Self>) {
        cx.emit(CloseRequested);
    }

    pub fn new(locator: BookLocator, title: String, initial_target: Option<String>, app: app::AppClient, library: Rc<LibraryClient>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        window.set_window_title(&title);
        if ui_components::uses_mobile_navigation(window) {
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
                                cx.new(|cx| PdfReaderView::new(locator, title, app, library, reader, metadata, page_source, initial_page, initial_full_page_position, close_reader, window, cx)).into()
                            }
                            PreparedBook::Book { book, preparation } => cx.new(|cx| EpubReaderView::new(locator, title, app, library, book, *preparation, close_reader, window, cx)).into(),
                            PreparedBook::Audiobook { resolved, initial_target } => {
                                let session = crate::open_audiobook(locator, library.as_ref().clone(), resolved, initial_target, cx);
                                cx.emit(AudiobookOpened(session));
                                return;
                            }
                        };
                        reader_view.state = ReaderState::Ready(view);
                        cx.notify();
                    });
                }
                Err(error) => {
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        let library_id = *library.id();
                        let library_missing = match app.libraries().await {
                            Ok(libraries) => !libraries.iter().any(|entry| entry.library_id() == &library_id),
                            Err(check_error) => {
                                log::warn!("could not verify reader library {library_id} after load failure: {check_error}");
                                error.contains("library not found:")
                            }
                        };
                        if library_missing {
                            log::warn!("reader library {library_id} no longer exists; returning to the library browser");
                            let _ = reader_view.update(cx, Self::request_close);
                            return;
                        }
                    }
                    let _ = reader_view.update(cx, move |reader_view, cx| {
                        log::error!("cannot load reader page for {:?}: {error}", reader_view.title);
                        reader_view.state = ReaderState::Error(SharedString::from(error));
                        cx.notify();
                    });
                }
            }
        });
        #[cfg(feature = "kobo")]
        if let Err(error) = gpui_kobo::request_experience_mode(gpui_kobo::KoboExperienceMode::ReaderQuality) {
            log::error!("failed to enter Kobo reader quality mode: {error}");
        }
        Self { title: reader_title, state: ReaderState::Loading, _load_task: load_task }
    }
}

impl EventEmitter<CloseRequested> for ReaderView {}
impl EventEmitter<AudiobookOpened> for ReaderView {}

impl Render for ReaderView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let message = match &self.state {
            ReaderState::Loading => SharedString::from("Loading book…"),
            ReaderState::Ready(view) => return view.clone().into_any_element(),
            ReaderState::Error(error) => error.clone(),
        };
        let theme = ui_components::browser_theme(cx);
        let content = gpui::div().size_full().flex().items_center().justify_center().bg(theme.page_bg).text_color(theme.text).child(message).into_any_element();
        if ui_components::uses_mobile_navigation(window) {
            let back = ui_components::mobile_back_button(theme).on_click(cx.listener(|reader, _, _, cx| reader.request_close(cx)));
            gpui::div()
                .relative()
                .size_full()
                .child(content)
                .child(gpui::div().absolute().top_0().left_0().right_0().pt(window.insets().safe_area.top).bg(theme.page_bg).child(ui_components::mobile_navigation_bar(self.title.clone(), Some(back), theme)))
                .into_any_element()
        } else {
            content
        }
    }
}
