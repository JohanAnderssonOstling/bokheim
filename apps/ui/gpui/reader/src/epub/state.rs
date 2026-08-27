mod annotations;
mod appearance;
mod events;
mod navigation;
mod search;

use std::{rc::Rc, sync::Arc};
use web_time::Instant;

use bokheim_backend::LibraryBackend;
use gpui::prelude::*;
use gpui::{App, Context, Window};
use gpui_component::input::{InputEvent, InputState};
use gpui_component::tree::TreeState;
use html_view_core::{InteractionPalette, RendererCommand, RendererEvent, RendererInitialConfig};
use html_view_gpui::HtmlView;
use library_model::BookLocator;

use crate::epub::toc::TocState;
use crate::epub::{LoadState, ProgressState, ReaderView, SearchState};
use crate::invalidation::Component;
use crate::settings::{ReaderSettings, library_paint_palette};
use crate::shell::marks::MarkSession;
use crate::shell::persistence::PositionWriter;
use crate::shell::search::SearchBarTarget;
use crate::shell::state_panel::SidebarState;
use app_preferences::LINE_WIDTH_MAX;

pub(crate) fn renderer_initial_config(cx: &App) -> RendererInitialConfig {
    let preferences = ReaderSettings::preferences(cx).clone();
    let column_width = if cfg!(feature = "kobo") { LINE_WIDTH_MAX } else { preferences.column_width };
    RendererInitialConfig {
        font_size: preferences.font_size,
        column_width,
        scale: preferences.scale,
        max_column_count: cfg!(feature = "kobo").then_some(1),
        style_overrides: preferences.style_overrides(),
        paint_palette: library_paint_palette(cx),
        interaction_palette: InteractionPalette::default(),
        ..Default::default()
    }
}

impl ReaderView {
    pub(crate) fn new(
        locator: BookLocator, title: String, library: Rc<LibraryBackend>, publication: Arc<crate::publication::NativePublication>, preparation: html_view_core::RendererPreparation, return_to_library: crate::ReturnToLibrary,
        window: &mut Window, cx: &mut Context<Self>,
    ) -> Self {
        let reader_started = Instant::now();
        startup_phase!("phase=reader_view_begin");
        let search_input = cx.new(|cx| InputState::new(window, cx));
        let search_subscription = cx.subscribe_in(&search_input, window, |this, input, event: &InputEvent, _window, cx| match event {
            InputEvent::Change => {
                let query = input.read(cx).value().to_string();
                this.queue_search(query, cx);
            }
            InputEvent::PressEnter { shift, .. } if !this.apply_pending_search(cx) => {
                this.search_navigate(if *shift { -1 } else { 1 }, cx);
            }
            _ => {}
        });
        let annotation_note_input = cx.new(|cx| crate::shell::annotation_editor::note_input(window, cx));
        let focus = cx.focus_handle();
        let toc_tree = cx.new(|cx| TreeState::new(cx));
        focus.focus(window, cx);
        let reading_content_hash = locator.content_hash();
        let reading_positions = PositionWriter::new(
            cx,
            "save reading position",
            move |library, (cfi, progress): &(String, f32)| {
                let content_hash = reading_content_hash.clone();
                let cfi = cfi.clone();
                let progress = *progress;
                crate::shell::persistence::run(library, move |library| library.update_reading_position(content_hash, &cfi, Some(progress)))
            },
            library.clone(),
        );
        let marks = MarkSession::new(cx, library.clone());

        let mut reader = Self {
            locator,
            library,
            book_title: title.clone(),
            title,
            load_state: LoadState::Loading,
            desired_column_count: if cfg!(feature = "kobo") { 1 } else { ReaderSettings::preferences(cx).max_column_count },
            toc: TocState::default(),
            toc_tree,
            sidebar: SidebarState::from_preferences(cx),
            chrome: crate::shell::chrome::ChromeState::new(Component::ReaderShell),
            search_input,
            search: SearchState::default(),
            publication,
            current_cfi: None,
            progress: ProgressState::default(),
            marks,
            selection: None,
            annotation_note_input,
            annotation_popup_anchor: None,
            footnote: None,
            image_preview: None,
            focus,
            renderer_subscription: None,
            reading_positions,
            return_to_library,
            _search_subscription: search_subscription,
        };
        reader.finish_load(preparation, window, cx);
        startup_phase!("phase=reader_view_ready phase_ms={}", reader_started.elapsed().as_millis());
        reader
    }

    fn finish_load(&mut self, preparation: html_view_core::RendererPreparation, window: &mut Window, cx: &mut Context<Self>) {
        let finish_started = Instant::now();
        startup_phase!("phase=finish_load_begin");
        let renderer_started = Instant::now();
        let renderer = cx.new(|cx| HtmlView::from_preparation(preparation, window, cx).expect("background-prepared renderer should finish shaping"));
        startup_phase!("phase=renderer_entity_ready elapsed_ms={} phase_ms={}", finish_started.elapsed().as_millis(), renderer_started.elapsed().as_millis());
        self.renderer_subscription = Some(cx.subscribe_in(&renderer, window, |this, _, event: &RendererEvent, window, cx| {
            this.handle_renderer_event(event, window, cx);
        }));
        let toc_started = Instant::now();
        let toc = renderer.read(cx).toc().unwrap_or_default();
        let resolutions = {
            let renderer = renderer.read(cx);
            TocState::resolve(&toc, |href| renderer.resolve_href(href))
        };
        let toc_anchors = TocState::anchor_strings_by_doc(&resolutions, self.publication.document_uris.len());
        self.set_toc(toc, resolutions, cx);
        renderer.update(cx, |renderer, cx| renderer.set_toc_anchor_strings_by_doc(toc_anchors, cx));
        startup_phase!("phase=toc_ready elapsed_ms={} phase_ms={}", finish_started.elapsed().as_millis(), toc_started.elapsed().as_millis());

        startup_phase!("phase=initial_preferences_embedded elapsed_ms={}", finish_started.elapsed().as_millis());
        self.load_state = LoadState::Ready(renderer);
        self.load_document_text_lengths(cx);
        // The window focused the page container before there was a document to
        // focus. Hand keyboard control to the document now that one exists, so
        // paging works without clicking the text first.
        self.focus_document(window, cx);
        self.load_reader_state(cx);
        startup_phase!("phase=reader_state_requested elapsed_ms={}", finish_started.elapsed().as_millis());
        crate::invalidation::notify(cx, Component::ReaderShell, "reader_ready");
        startup_phase!("phase=finish_load_complete elapsed_ms={}", finish_started.elapsed().as_millis());
    }

    /// Loads stable publication-wide progress weights without extending the
    /// reader's startup path. Until these arrive the renderer deliberately
    /// falls back to one unit per spine item, which is enough for navigation
    /// but cannot report movement inside a long chapter.
    fn load_document_text_lengths(&self, cx: &mut Context<Self>) {
        let provider = self.publication.provider.clone();
        let documents = self.publication.document_uris.clone();
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let lengths = executor.spawn(async move { html_view_core::publication_text_lengths(provider.as_ref(), &documents) }).await;
            let _ = this.update(cx, |this, cx| {
                this.with_renderer(cx, |renderer, cx| renderer.apply(RendererCommand::SetDocumentTextLengths(lengths), cx));
            });
        })
        .detach();
    }
}
