//! Reader page lifecycle inside the application's single window.

use std::rc::Rc;
use std::sync::Arc;
use web_time::Instant;

use app_contract::backend::LibraryBackend;
#[cfg(feature = "pdf")]
use app_contract::library::BoxedBookReader;
use app_contract::library::{BookFormat, BookLocator};
use gpui::prelude::*;
use gpui::{AnyView, App, AppContext, Context, IntoElement, Render, SharedString, Task, Window};
use html_publication::PublicationFormat;

use crate::epub::{PendingReader, ReaderView as EpubReaderView};
#[cfg(feature = "pdf")]
use crate::pdf_reader::{PdfReaderView, PendingPdfReader};
use crate::settings::{ReaderPreferences, ReaderSettings, SaveReaderSettings};

/// Called by a reader's library button to restore the application library.
pub type ReturnToLibrary = Rc<dyn Fn(&mut Window, &mut App)>;

enum LoadedBook {
    Publication {
        publication: Arc<crate::publication::NativePublication>,
        preparation: html_view_core::RendererPreparation,
    },
    #[cfg(feature = "pdf")]
    Pdf {
        reader: BoxedBookReader,
        initial_page: usize,
        initial_full_page_position: f32,
    },
}

enum ReaderState {
    Loading(SharedString),
    Ready(AnyView),
    Error(SharedString),
}

/// The reader page installed directly in the application's single window.
/// It owns publication loading and replaces its loading state with the
/// format-specific reader when the book is ready.
pub struct ReaderView {
    state: ReaderState,
    _load_task: Option<Task<()>>,
}

pub fn configure(preferences: ReaderPreferences, save: SaveReaderSettings, cx: &mut App) {
    cx.set_global(ReaderSettings { preferences, save });
}

fn report_stage(progress: &async_channel::Sender<String>, stage: impl Into<String>) {
    let _ = progress.try_send(stage.into());
}

async fn load_book(
    locator: BookLocator, library: Rc<dyn LibraryBackend>, executor: gpui::BackgroundExecutor, renderer_config: html_view_core::RendererInitialConfig, initial_target: Option<String>, startup_started: Instant,
    progress: async_channel::Sender<String>,
) -> Result<LoadedBook, String> {
    let prepare_locator = locator;
    let phase_started = Instant::now();
    report_stage(&progress, "Reading book from browser storage…");
    let content = library.open_reader_content(prepare_locator.content_hash()).await.map_err(|error| format!("stage=book_resolve: {error}"))?;
    startup_phase!("phase=content_resolved content_hash={} elapsed_ms={} phase_ms={}", prepare_locator.content_hash(), startup_started.elapsed().as_millis(), phase_started.elapsed().as_millis());
    report_stage(&progress, "Book bytes loaded; detecting format…");
    let (format, reader) = content.into_parts();

    let prepared = match format {
        #[cfg(feature = "pdf")]
        BookFormat::Pdf => {
            let phase_started = Instant::now();
            report_stage(&progress, "Loading PDF reading position…");
            let (initial_page, initial_full_page_position) = library.pdf_reading_position(prepare_locator.content_hash()).await.map_err(|error| format!("stage=reading_position: {error}"))?.unwrap_or((0, 0.0));
            startup_phase!("phase=reading_position_ready content_hash={} elapsed_ms={} phase_ms={} format=pdf", prepare_locator.content_hash(), startup_started.elapsed().as_millis(), phase_started.elapsed().as_millis());
            LoadedBook::Pdf { reader, initial_page: initial_page as usize, initial_full_page_position }
        }
        #[cfg(not(feature = "pdf"))]
        BookFormat::Pdf => return Err("stage=prepare: PDF support is disabled".to_owned()),
        BookFormat::M4b => return Err("stage=prepare: audiobooks are played in the library".to_owned()),
        BookFormat::Epub | BookFormat::Mobi => {
            let phase_started = Instant::now();
            let publication_format = match format {
                BookFormat::Epub => PublicationFormat::Epub,
                BookFormat::Mobi => PublicationFormat::Mobi,
                _ => unreachable!("reader format was matched by the enclosing arm"),
            };
            report_stage(&progress, "Opening publication…");
            let publication = executor.spawn(async move { crate::publication::NativePublication::from_reader(publication_format, reader).map_err(|error| format!("stage=publication_open: {error}")) }).await?;
            startup_phase!("phase=publication_opened content_hash={} elapsed_ms={} phase_ms={}", prepare_locator.content_hash(), startup_started.elapsed().as_millis(), phase_started.elapsed().as_millis());
            report_stage(&progress, "Publication parsed; loading reading position…");
            let nav_state = library.reading_position(prepare_locator.content_hash()).await.map_err(|error| format!("stage=reading_position: {error}"))?;
            report_stage(&progress, "Preparing first document…");
            let publication = Arc::new(publication);
            let prepare_publication = publication.clone();
            let prepare_nav_state = nav_state.clone();
            let preparation = executor
                .spawn(async move {
                    html_view_core::RendererPreparation::from_provider_with_nav_and_target(
                        prepare_publication.provider.clone(),
                        prepare_publication.document_uris.clone(),
                        prepare_publication.start_index,
                        prepare_nav_state.as_deref(),
                        initial_target.as_deref(),
                        renderer_config,
                    )
                    .map_err(|error| format!("stage=document_prepare: {error}"))
                })
                .await?;
            LoadedBook::Publication { publication, preparation }
        }
        BookFormat::Fb2 | BookFormat::Cbz | BookFormat::Html | BookFormat::WebPub => {
            return Err(format!("stage=prepare: {} is not supported by the GPUI reader", format.canonical_extension()));
        }
    };
    Ok(prepared)
}

impl ReaderView {
    pub fn new(locator: BookLocator, title: String, initial_target: Option<String>, library: Rc<dyn LibraryBackend>, return_to_library: ReturnToLibrary, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let startup_started = Instant::now();
        startup_phase!("phase=request content_hash={} elapsed_ms=0", locator.content_hash());
        #[cfg(feature = "kobo")]
        if let Err(error) = gpui_kobo::begin_book_launch_profile(startup_started) {
            log::error!("failed to start Kobo book launch profile: {error}");
        }
        #[cfg(not(target_family = "wasm"))]
        window.set_window_title(&title);
        let mut reader = Self { state: ReaderState::Loading(SharedString::from("Starting book load…")), _load_task: None };
        reader.start_load(locator, title, initial_target, library, return_to_library, startup_started, window, cx);
        reader
    }

    pub fn fail_load(&mut self, error: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.state = ReaderState::Error(error.into());
        cx.notify();
    }

    fn start_load(&mut self, locator: BookLocator, title: String, initial_target: Option<String>, library: Rc<dyn LibraryBackend>, return_to_library: ReturnToLibrary, startup_started: Instant, _window: &mut Window, cx: &mut Context<Self>) {
        let load_locator = locator.clone();
        let executor = cx.background_executor().clone();
        let renderer_config = crate::epub::state::renderer_initial_config(cx);
        let (progress_tx, progress_rx) = async_channel::unbounded::<String>();
        cx.spawn(async move |reader_view, cx| {
            while let Ok(stage) = progress_rx.recv().await {
                let _ = reader_view.update(cx, |reader_view, cx| {
                    if matches!(&reader_view.state, ReaderState::Loading(_)) {
                        reader_view.state = ReaderState::Loading(SharedString::from(stage));
                        cx.notify();
                    }
                });
            }
        })
        .detach();
        let load_task = cx.spawn(async move |reader_view, cx| {
            let result = load_book(load_locator, library.clone(), executor, renderer_config, initial_target, startup_started, progress_tx).await;
            match result {
                Ok(book) => {
                    let _ = reader_view.update_in(cx, move |reader_view, window, cx| {
                        reader_view.state = ReaderState::Loading(SharedString::from("Building reader interface…"));
                        let view: AnyView = match book {
                            #[cfg(feature = "pdf")]
                            LoadedBook::Pdf { reader, initial_page, initial_full_page_position } => {
                                cx.new(|cx| PdfReaderView::new(PendingPdfReader { locator, title, library, reader, initial_page, initial_full_page_position, return_to_library }, window, cx)).into()
                            }
                            LoadedBook::Publication { publication, preparation } => cx.new(|cx| EpubReaderView::new(PendingReader { locator, title, library, publication, preparation, return_to_library }, window, cx)).into(),
                        };
                        startup_phase!("phase=reader_ready elapsed_ms={}", startup_started.elapsed().as_millis());
                        reader_view.state = ReaderState::Ready(view);
                        cx.notify();
                    });
                }
                Err(error) => {
                    let _ = reader_view.update(cx, move |reader_view, cx| {
                        log::error!("cannot load reader page: {error}");
                        reader_view.state = ReaderState::Error(SharedString::from(error));
                        cx.notify();
                    });
                }
            }
        });
        self._load_task = Some(load_task);
        #[cfg(feature = "kobo")]
        if let Err(error) = gpui_kobo::request_experience_mode(gpui_kobo::KoboExperienceMode::ReaderQuality) {
            log::error!("failed to enter Kobo reader quality mode: {error}");
        }
    }
}

impl Render for ReaderView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        match &self.state {
            ReaderState::Loading(stage) => gpui::div().size_full().flex().flex_col().items_center().justify_center().child("Loading book…").child(stage.clone()).into_any_element(),
            ReaderState::Ready(view) => view.clone().into_any_element(),
            ReaderState::Error(error) => gpui::div().size_full().flex().items_center().justify_center().child(error.clone()).into_any_element(),
        }
    }
}
