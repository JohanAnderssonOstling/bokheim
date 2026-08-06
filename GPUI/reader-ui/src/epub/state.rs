use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use gpui::prelude::*;
use gpui::{App, Context, ScrollStrategy, Window};
use gpui_component::input::{InputEvent, InputState};
use gpui_component::tree::{TreeItem, TreeState};
use html_view_core::{InteractionPalette, RendererCommand, RendererEvent, RendererInitialConfig, SearchScope, TocEntry};
use html_view_gpui::HtmlView;

use crate::epub::audio::AudioMode;
use crate::epub::audio::AudioState;
use crate::epub::marks::{self, MarksState, Selection};
use crate::epub::progress::ProgressState;
use crate::epub::search::{SearchFilters, SearchState};
use crate::epub::toc::TocState;
use crate::epub::toolbar::{AudioAffordance, ReaderToolbar, ToolbarEvent, ToolbarModel};
use crate::epub::{LoadState, PendingReader, ReaderView};
use crate::invalidation::Component;
use crate::settings::{ReaderPreferences, ReaderServices, ReaderSettings, library_paint_palette};
use crate::shell::persistence::{self, PositionWriter};
use epub_svelte_lib::app::preferences::{COLUMN_COUNT_MAX, COLUMN_COUNT_MIN, FONT_SIZE_MAX, FONT_SIZE_MIN, LINE_HEIGHT_MAX, LINE_HEIGHT_MIN, LINE_WIDTH_MAX, LINE_WIDTH_MIN, ReaderFontFamily, ReaderTextAlignment, ZOOM_MAX, ZOOM_MIN};

/// How long typing settles before a search is applied.
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(150);

/// Narrowest column the fit-columns control may produce. Below the
/// hand-adjustable minimum on purpose: fitting four columns into a narrow
/// window needs columns the width slider would refuse.
const FITTED_COLUMN_WIDTH_MIN: f64 = 80.0;

/// How often read-aloud is checked for having finished.
const SPEECH_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// How often the current narration cue is checked for having finished. Tighter
/// than speech polling so cues run together without an audible gap.
const OVERLAY_POLL_INTERVAL: Duration = Duration::from_millis(50);

impl ReaderView {
    pub(crate) fn new(pending: PendingReader, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let reader_started = Instant::now();
        startup_phase!("phase=reader_view_begin");
        #[cfg(debug_assertions)]
        crate::invalidation::initialize_performance_measurements();
        let search_input = cx.new(|cx| InputState::new(window, cx));
        let search_subscription = cx.subscribe_in(&search_input, window, |this, input, event: &InputEvent, _window, cx| match event {
            InputEvent::Change => {
                let query = input.read(cx).value().to_string();
                this.queue_search(query, cx);
            }
            InputEvent::PressEnter { shift, .. } if !this.apply_pending_search(cx) => {
                this.navigate_search(if *shift { -1 } else { 1 }, cx);
            }
            _ => {}
        });
        let annotation_note_input = cx.new(|cx| InputState::new(window, cx).placeholder("Add a note").multi_line(true));
        let publication = pending.publication.clone();
        let media_overlay_cues = publication.media_overlay_cues.clone();
        let generation = 1;
        let load_publication = publication.clone();
        let callback_scheduled_at = Instant::now();
        let nav_state = pending.nav_state.clone();
        startup_phase!("phase=loading_frame_wait_begin");
        cx.on_next_frame(window, move |this, window, cx| {
            startup_phase!("phase=loading_frame_rendered queue_ms={}", callback_scheduled_at.elapsed().as_millis());
            this.finish_load(generation, Ok(load_publication), nav_state.as_deref(), window, cx);
        });

        let bounds_subscription = crate::shell::geometry::observe(window, cx);
        let focus = cx.focus_handle();
        let toc_tree = cx.new(|cx| TreeState::new(cx));
        let toolbar_view = cx.new(|_| ReaderToolbar::default());
        let toolbar_subscription = cx.subscribe_in(&toolbar_view, window, |this, _, event: &ToolbarEvent, window, cx| {
            this.handle_toolbar_event(*event, window, cx);
        });
        focus.focus(window, cx);
        let reading_book_id = pending.locator.book_id();
        let reading_positions =
            PositionWriter::new(cx, "save reading position", move |library, (cfi, progress): &(String, f32)| library.update_reading_position(reading_book_id, cfi, *progress).map_err(|error| error.to_string()), pending.library.clone());

        let reader = Self {
            id: pending.id,
            locator: pending.locator,
            library: pending.library,
            book_title: pending.title.clone(),
            title: pending.title,
            load_state: LoadState::Loading,
            load_generation: generation,
            desired_column_count: 2,
            toc: TocState::default(),
            toc_tree,
            toc_visible: ReaderSettings::preferences(cx).toc_visible,
            search_input,
            search: SearchState::default(),
            publication,
            current_cfi: None,
            progress: ProgressState::default(),
            toolbar_view,
            marks: MarksState::default(),
            reader_state_visible: false,
            reader_state_bookmarks: false,
            annotation_note_input,
            footnote: None,
            image_preview: None,
            audio: AudioState::new(media_overlay_cues),
            can_history_back: false,
            can_history_forward: false,
            unavailable: false,
            focus,
            renderer_subscription: None,
            reading_positions,
            _subscriptions: vec![bounds_subscription, search_subscription, toolbar_subscription],
        };
        startup_phase!("phase=reader_view_ready phase_ms={}", reader_started.elapsed().as_millis());
        reader
    }

    fn finish_load(&mut self, generation: u64, result: Result<Arc<crate::publication::NativePublication>, String>, nav_state: Option<&str>, window: &mut Window, cx: &mut Context<Self>) {
        let finish_started = Instant::now();
        startup_phase!("phase=finish_load_begin");
        if generation != self.load_generation {
            return;
        }
        let publication = match result {
            Ok(publication) => publication,
            Err(error) => {
                log::error!("failed to load reader {} for book {}: {error}", self.id.0, self.locator.book_id());
                self.load_state = LoadState::Failed;
                crate::invalidation::notify(cx, Component::ReaderShell, "state_changed");
                return;
            }
        };

        let provider = publication.provider.clone();
        let documents = publication.document_uris.clone();
        let start_index = publication.start_index;
        let preferences = ReaderSettings::preferences(cx).clone();
        let initial_config = RendererInitialConfig {
            font_size: preferences.font_size,
            column_width: preferences.column_width,
            scale: preferences.scale,
            style_overrides: preferences.style_overrides(),
            paint_palette: library_paint_palette(cx),
            interaction_palette: InteractionPalette::default(),
            search_active: self.search.active,
            search_query: self.search.query().to_owned(),
            ..Default::default()
        };
        let renderer_started = Instant::now();
        let renderer = cx.new(|cx| HtmlView::from_provider(provider, documents, start_index, nav_state, initial_config, window, cx));
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
        self.set_toc(toc, resolutions, cx);
        startup_phase!("phase=toc_ready elapsed_ms={} phase_ms={}", finish_started.elapsed().as_millis(), toc_started.elapsed().as_millis());

        startup_phase!("phase=initial_preferences_embedded elapsed_ms={}", finish_started.elapsed().as_millis());
        self.load_state = LoadState::Ready(renderer);
        self.load_reader_state(cx);
        startup_phase!("phase=reader_state_requested elapsed_ms={}", finish_started.elapsed().as_millis());
        self.invalidate_toolbar("reader_ready", cx);
        crate::invalidation::notify(cx, Component::ReaderShell, "state_changed");
        startup_phase!("phase=finish_load_complete elapsed_ms={}", finish_started.elapsed().as_millis());
    }

    fn load_reader_state(&mut self, cx: &mut Context<Self>) {
        let book_id = self.locator.book_id();
        persistence::write(
            cx,
            self.library.clone(),
            "load reader state",
            move |library| {
                let annotations = library.annotations(book_id).map_err(|error| error.to_string())?;
                let bookmarks = library.bookmarks(book_id).map_err(|error| error.to_string())?;
                Ok((annotations, bookmarks))
            },
            |this, cx, (annotations, bookmarks)| {
                this.marks.annotations = annotations;
                this.marks.bookmarks = bookmarks;
                let overlays = this.marks.overlays();
                this.with_renderer(cx, |renderer, cx| renderer.apply(RendererCommand::SetAnnotations(overlays), cx));
                crate::invalidation::notify(cx, Component::ReaderShell, "state_changed");
            },
        );
    }

    pub(crate) fn highlight_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(selection) = self.marks.selection.clone() else { return };
        self.annotation_note_input.update(cx, |input, cx| input.set_value(String::new(), window, cx));
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|duration| duration.as_millis() as i64).unwrap_or(0);
        let annotation = epub_svelte_lib::library::marks::ReaderAnnotation {
            id: uuid::Uuid::new_v4().to_string(),
            book_id: self.locator.book_id().canonical(),
            anchor: epub_svelte_lib::library::marks::AnnotationAnchor::epub_cfi(selection.cfi_range).expect("renderer selection returns a valid EPUB CFI"),
            exact_text: selection.text,
            prefix: selection.prefix,
            suffix: selection.suffix,
            style: epub_svelte_lib::library::marks::AnnotationStyle::Highlight,
            color: "#f6c945".to_owned(),
            note: String::new(),
            created_at: now,
            modified_at: now,
        };
        let stored = annotation.clone();
        persistence::write(
            cx,
            self.library.clone(),
            "save reader annotation",
            move |library| library.upsert_annotation(&stored).map_err(|error| error.to_string()),
            move |this, cx, ()| {
                let overlay = marks::renderer_annotation(&annotation).expect("new EPUB annotation has an EPUB anchor");
                this.marks.selected_id = Some(annotation.id.clone());
                this.marks.annotations.push(annotation);
                this.marks.clear_selection();
                this.with_renderer(cx, |renderer, cx| renderer.apply(RendererCommand::UpsertAnnotation(overlay), cx));
                this.invalidate_toolbar("selection_cleared", cx);
                crate::invalidation::notify(cx, Component::ReaderShell, "state_changed");
            },
        );
    }

    pub(crate) fn copy_selection_citation(&self, cx: &mut Context<Self>) {
        let title = self.display_title();
        let cfi = self.marks.selection.as_ref().map(|selection| selection.cfi_range.clone()).or_else(|| self.current_cfi.clone());
        self.with_renderer(cx, |renderer, cx| renderer.apply(RendererCommand::CopySelectionCitation { title, cfi }, cx));
    }

    fn now_millis() -> i64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map(|duration| duration.as_millis() as i64).unwrap_or(0)
    }

    pub(crate) fn select_annotation(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let note = self.marks.annotation(&id).map(|annotation| annotation.note.clone()).unwrap_or_default();
        self.marks.selected_id = Some(id);
        self.annotation_note_input.update(cx, |input, cx| input.set_value(note, window, cx));
        crate::invalidation::notify(cx, Component::ReaderShell, "state_changed");
    }

    pub(crate) fn close_annotation_popup(&mut self, cx: &mut Context<Self>) {
        if self.marks.selected_id.take().is_some() {
            crate::invalidation::notify(cx, Component::ReaderShell, "state_changed");
        }
    }

    pub(crate) fn navigate_to_cfi(&self, cfi: String, cx: &mut Context<Self>) {
        self.with_renderer(cx, |renderer, cx| renderer.apply(RendererCommand::SetCfiPosition(Some(cfi)), cx));
    }

    pub(crate) fn save_selected_annotation(&mut self, style: Option<epub_svelte_lib::library::marks::AnnotationStyle>, cx: &mut Context<Self>) {
        let Some(id) = self.marks.selected_id.clone() else { return };
        let Some(mut annotation) = self.marks.annotation(&id).cloned() else { return };
        annotation.note = self.annotation_note_input.read(cx).value().to_string();
        if let Some(style) = style {
            annotation.style = style;
        }
        annotation.modified_at = Self::now_millis();
        let stored = annotation.clone();
        persistence::write(
            cx,
            self.library.clone(),
            "update reader annotation",
            move |library| library.upsert_annotation(&stored).map_err(|error| error.to_string()),
            move |this, cx, ()| {
                if let Some(overlay) = marks::renderer_annotation(&annotation) {
                    this.with_renderer(cx, |renderer, cx| renderer.apply(RendererCommand::UpsertAnnotation(overlay), cx));
                }
                this.marks.replace(annotation);
                this.marks.selected_id = None;
                crate::invalidation::notify(cx, Component::ReaderShell, "state_changed");
            },
        );
    }

    pub(crate) fn delete_annotation(&mut self, id: String, cx: &mut Context<Self>) {
        let modified_at = Self::now_millis();
        let stored_id = id.clone();
        persistence::write(
            cx,
            self.library.clone(),
            "delete reader annotation",
            move |library| library.delete_annotation(&stored_id, modified_at).map_err(|error| error.to_string()),
            move |this, cx, ()| {
                this.marks.remove_annotation(&id);
                this.with_renderer(cx, |renderer, cx| renderer.apply(RendererCommand::RemoveAnnotation(id), cx));
                crate::invalidation::notify(cx, Component::ReaderShell, "state_changed");
            },
        );
    }

    pub(crate) fn add_bookmark(&mut self, cx: &mut Context<Self>) {
        let Some(cfi) = self.current_cfi.clone() else { return };
        let now = Self::now_millis();
        let bookmark = epub_svelte_lib::library::marks::ReaderBookmark { id: uuid::Uuid::new_v4().to_string(), book_id: self.locator.book_id().canonical(), cfi, label: self.display_title(), created_at: now, modified_at: now };
        let stored = bookmark.clone();
        persistence::write(
            cx,
            self.library.clone(),
            "save bookmark",
            move |library| library.upsert_bookmark(&stored).map_err(|error| error.to_string()),
            move |this, cx, ()| {
                this.marks.bookmarks.push(bookmark);
                this.reader_state_visible = true;
                this.reader_state_bookmarks = true;
                crate::invalidation::notify(cx, Component::ReaderShell, "state_changed");
            },
        );
    }

    pub(crate) fn delete_bookmark(&mut self, id: String, cx: &mut Context<Self>) {
        let modified_at = Self::now_millis();
        let stored_id = id.clone();
        persistence::write(
            cx,
            self.library.clone(),
            "delete bookmark",
            move |library| library.delete_bookmark(&stored_id, modified_at).map_err(|error| error.to_string()),
            move |this, cx, ()| {
                this.marks.remove_bookmark(&id);
                crate::invalidation::notify(cx, Component::ReaderShell, "state_changed");
            },
        );
    }

    fn handle_renderer_event(&mut self, event: &RendererEvent, window: &mut Window, cx: &mut Context<Self>) {
        let should_notify = match event {
            RendererEvent::CfiChanged(cfi) => {
                if self.current_cfi == *cfi {
                    false
                } else {
                    let availability_changed = self.current_cfi.is_some() != cfi.is_some();
                    self.current_cfi.clone_from(cfi);
                    availability_changed
                }
            }
            RendererEvent::TitleChanged(Some(title)) => {
                if self.title == *title {
                    false
                } else {
                    self.title = title.clone();
                    true
                }
            }
            RendererEvent::TocChanged(_) => false,
            RendererEvent::NavAnchorChanged { doc, anchor } => {
                let next = self.toc.link_at(*doc, anchor);
                if self.toc.active_link == next {
                    false
                } else {
                    if let Some(link) = next.as_ref() {
                        let selected = TreeItem::new(link.clone(), link.clone());
                        self.toc_tree.update(cx, |tree, cx| {
                            tree.set_selected_item(Some(&selected), cx);
                            tree.reveal_item(&selected.id, ScrollStrategy::Center, cx);
                        });
                    }
                    self.toc.active_link = next;
                    true
                }
            }
            RendererEvent::HistoryAvailability { back, forward } => {
                if self.can_history_back == *back && self.can_history_forward == *forward {
                    false
                } else {
                    self.can_history_back = *back;
                    self.can_history_forward = *forward;
                    true
                }
            }
            RendererEvent::SearchActiveChanged(active) => {
                if self.search.active == *active {
                    false
                } else {
                    self.search.active = *active;
                    true
                }
            }
            RendererEvent::FontSizeChanged(font_size) => ReaderSettings::update(cx, |preferences| {
                preferences.font_size = *font_size;
            }),
            RendererEvent::ColumnWidthChanged(column_width) => ReaderSettings::update(cx, |preferences| {
                preferences.column_width = *column_width;
            }),
            RendererEvent::ScaleChanged(scale) => ReaderSettings::update(cx, |preferences| {
                preferences.scale = *scale;
            }),
            RendererEvent::ReadingProgress { fraction, location, total_locations, doc, doc_count } => {
                if !self.progress.update(*fraction, *location, *total_locations, *doc, *doc_count) {
                    return;
                }
                if let Some(cfi) = self.current_cfi.clone() {
                    self.reading_positions.record((cfi, *fraction), "reading-position");
                }
                false
            }
            RendererEvent::SelectionFinished { cfi_range, exact_text, prefix, suffix, .. } => {
                if !self.marks.set_selection(Selection { cfi_range: cfi_range.clone(), text: exact_text.clone(), prefix: prefix.clone(), suffix: suffix.clone() }) {
                    return;
                }
                true
            }
            RendererEvent::AnnotationActivated { id } => {
                if self.marks.selected_id.as_deref() == Some(id.as_str()) {
                    return;
                }
                self.select_annotation(id.clone(), window, cx);
                false
            }
            RendererEvent::FootnoteOpened(preview) => {
                self.footnote = Some(preview.clone());
                true
            }
            RendererEvent::SpeakableText(text) => {
                match ReaderServices::get(cx).speech.speak(text) {
                    Ok(()) => {
                        let generation = self.audio.begin_speech();
                        self.poll_tts_completion(generation, cx);
                    }
                    Err(error) => log::warn!("{error}"),
                }
                true
            }
            RendererEvent::ImageOpened { uri, bytes } => {
                let extension = uri.rsplit('.').next().unwrap_or_default().split(['?', '#']).next().unwrap_or_default().to_ascii_lowercase();
                let format = match extension.as_str() {
                    "jpg" | "jpeg" => gpui::ImageFormat::Jpeg,
                    "webp" => gpui::ImageFormat::Webp,
                    "gif" => gpui::ImageFormat::Gif,
                    "svg" | "svgz" => gpui::ImageFormat::Svg,
                    "bmp" => gpui::ImageFormat::Bmp,
                    "tif" | "tiff" => gpui::ImageFormat::Tiff,
                    "ico" => gpui::ImageFormat::Ico,
                    "pnm" | "ppm" | "pgm" | "pbm" => gpui::ImageFormat::Pnm,
                    _ => gpui::ImageFormat::Png,
                };
                self.image_preview = Some((uri.clone(), Arc::new(gpui::Image::from_bytes(format, bytes.clone()))));
                true
            }
            RendererEvent::MatchInfo { current, total } => {
                let next = Some((*current, *total));
                if self.search.match_position == next {
                    false
                } else {
                    self.search.match_position = next;
                    true
                }
            }
            RendererEvent::SearchResults(results) => self.search.set_results(results),
            RendererEvent::OperationFailed { operation, message } => {
                log::warn!("renderer failed to {operation}: {message}");
                false
            }
            _ => false,
        };
        if should_notify {
            match event {
                RendererEvent::TitleChanged(_)
                | RendererEvent::NavAnchorChanged { .. }
                | RendererEvent::HistoryAvailability { .. }
                | RendererEvent::FontSizeChanged(_)
                | RendererEvent::ColumnWidthChanged(_)
                | RendererEvent::ScaleChanged(_)
                | RendererEvent::SelectionFinished { .. }
                | RendererEvent::SpeakableText(_) => self.invalidate_toolbar("renderer_state_changed", cx),
                _ => crate::invalidation::notify(cx, Component::ReaderShell, "renderer_state_changed"),
            }
        }
    }

    /// Pushes the toolbar everything it draws. Cheap to call after any event:
    /// the toolbar compares the model and repaints only on a real change.
    pub(crate) fn invalidate_toolbar(&self, reason: &'static str, cx: &mut Context<Self>) {
        let model = self.toolbar_model(cx);
        self.toolbar_view.update(cx, |toolbar, cx| toolbar.set_model(model, reason, cx));
    }

    fn toolbar_model(&self, cx: &App) -> ToolbarModel {
        ToolbarModel {
            title: self.display_title(),
            can_history_back: self.can_history_back,
            can_history_forward: self.can_history_forward,
            audio: self.audio_affordance(cx),
            column_count: self.desired_column_count,
            preferences: ReaderSettings::preferences(cx).clone(),
        }
    }

    fn audio_affordance(&self, cx: &App) -> AudioAffordance {
        match self.audio.mode() {
            AudioMode::MediaOverlay if self.audio.is_paused() => AudioAffordance::NarrationPaused,
            AudioMode::MediaOverlay if self.audio.is_playing() => AudioAffordance::NarrationPlaying,
            AudioMode::MediaOverlay => AudioAffordance::NarrationIdle,
            AudioMode::ReadAloud if !ReaderServices::get(cx).speech.is_available() => AudioAffordance::Unavailable,
            AudioMode::ReadAloud if self.audio.is_speaking() => AudioAffordance::Speaking,
            AudioMode::ReadAloud => AudioAffordance::ReadAloud,
        }
    }

    fn handle_toolbar_event(&mut self, event: ToolbarEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            ToolbarEvent::ReturnToLibrary => window.remove_window(),
            ToolbarEvent::NavigateHistory(-1) if self.can_history_back => self.navigate_history(-1, cx),
            ToolbarEvent::NavigateHistory(1) if self.can_history_forward => self.navigate_history(1, cx),
            ToolbarEvent::NavigateHistory(_) => {}
            ToolbarEvent::ToggleToc => {
                self.set_toc_visible(!self.toc_visible, cx);
                self.reader_state_visible = false;
                crate::invalidation::notify(cx, Component::ReaderShell, "state_changed");
            }
            ToolbarEvent::ToggleAnnotations => {
                self.reader_state_visible = !self.reader_state_visible;
                if self.reader_state_visible {
                    self.toc_visible = false;
                }
                crate::invalidation::notify(cx, Component::ReaderShell, "state_changed");
            }
            ToolbarEvent::ToggleAudio => match self.audio.mode() {
                AudioMode::MediaOverlay => self.toggle_media_overlay(cx),
                AudioMode::ReadAloud if self.audio.is_speaking() => self.stop_reading(cx),
                AudioMode::ReadAloud => self.read_visible_page(cx),
            },
            ToolbarEvent::AdjustColumnWidth(delta) => self.adjust_column_width(delta, cx),
            ToolbarEvent::AdjustColumnCount(delta) => self.adjust_desired_column_count(delta, window, cx),
            ToolbarEvent::AdjustFontSize(delta) => self.adjust_font_size(delta, cx),
            ToolbarEvent::AdjustScale(delta) => self.adjust_scale(delta, cx),
            ToolbarEvent::AdjustLineHeight(delta) => self.adjust_line_height(delta, cx),
            ToolbarEvent::SetFontFamily(family) => self.set_reader_font_family(family, cx),
            ToolbarEvent::SetTextAlignment(alignment) => self.set_reader_text_alignment(alignment, cx),
        }
    }

    fn set_toc(&mut self, entries: Vec<TocEntry>, resolutions: std::collections::HashMap<String, crate::epub::toc::Resolution>, cx: &mut Context<Self>) {
        let tree_items = Self::toc_tree_items(&entries);
        self.toc_tree.update(cx, |tree, cx| {
            tree.set_items(tree_items, cx);
        });
        self.toc.set(entries, resolutions);
    }

    fn toc_tree_items(entries: &[TocEntry]) -> Vec<TreeItem> {
        entries.iter().map(|entry| TreeItem::new(entry.link.clone(), entry.title.clone()).children(Self::toc_tree_items(&entry.children))).collect()
    }

    pub(crate) fn with_renderer(&self, cx: &mut Context<Self>, update: impl FnOnce(&mut HtmlView, &mut Context<HtmlView>)) {
        if let LoadState::Ready(renderer) = &self.load_state {
            renderer.update(cx, update);
        }
    }

    pub(crate) fn previous_page(&self, cx: &mut Context<Self>) {
        if self.progress.at_start() {
            return;
        }
        #[cfg(feature = "kobo")]
        if let Err(error) = gpui_kobo::request_reader_page_repaint(gpui::px(58.0)) {
            log::error!("failed to request Kobo reader page repaint: {error}");
        }
        self.with_renderer(cx, |renderer, cx| {
            renderer.previous_page(cx);
        });
    }

    pub(crate) fn next_page(&self, cx: &mut Context<Self>) {
        if self.progress.at_end() {
            return;
        }
        #[cfg(feature = "kobo")]
        if let Err(error) = gpui_kobo::request_reader_page_repaint(gpui::px(58.0)) {
            log::error!("failed to request Kobo reader page repaint: {error}");
        }
        self.with_renderer(cx, |renderer, cx| {
            renderer.next_page(cx);
        });
    }

    pub(crate) fn read_visible_page(&self, cx: &mut Context<Self>) {
        self.with_renderer(cx, |renderer, cx| renderer.apply(RendererCommand::RequestVisibleText, cx));
    }

    pub(crate) fn stop_reading(&mut self, cx: &mut Context<Self>) {
        if !self.audio.stop_speech() {
            return;
        }
        ReaderServices::get(cx).speech.stop();
        self.invalidate_toolbar("audio_state_changed", cx);
    }

    fn poll_tts_completion(&self, generation: u64, cx: &mut Context<Self>) {
        let timer = cx.background_executor().timer(SPEECH_POLL_INTERVAL);
        cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| {
                if !this.audio.speech_poll_is_current(generation) {
                    return;
                }
                match ReaderServices::get(cx).speech.status() {
                    crate::TtsState::Speaking => this.poll_tts_completion(generation, cx),
                    crate::TtsState::Idle | crate::TtsState::Unavailable => {
                        this.audio.speech_finished();
                        this.invalidate_toolbar("audio_state_changed", cx);
                    }
                }
            });
        })
        .detach();
    }

    pub(crate) fn copy_preview_image(&self, cx: &mut Context<Self>) {
        if let Some((_, image)) = &self.image_preview {
            cx.write_to_clipboard(gpui::ClipboardItem::new_image(image));
        }
    }

    pub(crate) fn play_media_overlay(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(cue) = self.audio.cues.get(index).cloned() else { return };
        let generation = self.audio.begin_cue();
        let provider = self.publication.provider.clone();
        let worker = ReaderServices::get(cx).blocking.clone();
        cx.spawn(async move |this, cx| {
            let audio_href = cue.audio_href.split('#').next().unwrap_or(&cue.audio_href).to_owned();
            let result = worker.dispatch(move || provider.read_bytes(&audio_href).map_err(|error| error.to_string())).recv().await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(Ok(audio)) => {
                    let backend = ReaderServices::get(cx).audio.clone();
                    match backend.play(&cue.audio_href, &audio, cue.clip_begin_seconds, cue.clip_end_seconds) {
                        Ok(()) => {
                            this.audio.cue_started(index);
                            this.with_renderer(cx, |renderer, cx| {
                                renderer.apply(RendererCommand::NavigateToHref(cue.text_href.clone()), cx);
                                renderer.apply(RendererCommand::SetMediaOverlayTarget(Some(cue.text_href.clone())), cx);
                            });
                            this.poll_media_overlay_completion(index, generation, cx);
                            this.invalidate_toolbar("audio_state_changed", cx);
                        }
                        Err(error) => log::warn!("media-overlay playback failed: {error}"),
                    }
                }
                Ok(Err(error)) => log::warn!("media-overlay resource failed: {error}"),
                Err(_) => log::warn!("media-overlay worker stopped"),
            });
        })
        .detach();
    }

    fn poll_media_overlay_completion(&self, index: usize, generation: u64, cx: &mut Context<Self>) {
        let timer = cx.background_executor().timer(OVERLAY_POLL_INTERVAL);
        cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| {
                if !this.audio.cue_poll_is_current(index, generation) {
                    return;
                }
                let state = ReaderServices::get(cx).audio.status().state;
                if state == crate::NativeAudioState::Stopped {
                    if this.audio.has_cue_after(index) {
                        this.play_media_overlay(index + 1, cx);
                    } else {
                        this.stop_media_overlay(cx);
                    }
                } else {
                    this.poll_media_overlay_completion(index, generation, cx);
                }
            });
        })
        .detach();
    }

    pub(crate) fn stop_media_overlay(&mut self, cx: &mut Context<Self>) {
        if !self.audio.stop_overlay() {
            return;
        }
        ReaderServices::get(cx).audio.stop();
        self.with_renderer(cx, |renderer, cx| renderer.apply(RendererCommand::SetMediaOverlayTarget(None), cx));
        self.invalidate_toolbar("audio_state_changed", cx);
    }

    pub(crate) fn toggle_media_overlay(&mut self, cx: &mut Context<Self>) {
        if !self.audio.is_playing() {
            self.play_media_overlay(self.audio.cue_index, cx);
            return;
        }
        let backend = ReaderServices::get(cx).audio.clone();
        let result = if self.audio.is_paused() { backend.resume() } else { backend.pause() };
        if let Err(error) = result {
            log::warn!("media-overlay state change failed: {error}");
            return;
        }
        let paused = !self.audio.is_paused();
        self.audio.set_paused(paused);
        self.invalidate_toolbar("audio_state_changed", cx);
    }

    /// Applies a preference change and tells the renderer about it.
    ///
    /// Every appearance setting follows the same path: write the preference,
    /// which persists it and reports whether it actually moved, then hand the
    /// renderer the matching command and refresh the toolbar. `command` is
    /// only consulted when something changed, so a setting already at its
    /// limit costs nothing.
    fn apply_appearance(&self, cx: &mut Context<Self>, update: impl FnOnce(&mut ReaderPreferences), command: impl FnOnce(&ReaderPreferences) -> RendererCommand) {
        if !ReaderSettings::update(cx, update) {
            return;
        }
        let command = command(ReaderSettings::preferences(cx));
        self.with_renderer(cx, |renderer, cx| renderer.apply(command, cx));
        self.invalidate_toolbar("preferences_changed", cx);
    }

    /// Applies a change to the reader stylesheet, which the renderer takes as
    /// a whole rather than field by field.
    fn apply_style_override(&self, cx: &mut Context<Self>, update: impl FnOnce(&mut ReaderPreferences)) {
        self.apply_appearance(cx, update, |preferences| RendererCommand::SetReaderStyleOverrides(preferences.style_overrides()));
    }

    pub(crate) fn adjust_font_size(&mut self, delta: f32, cx: &mut Context<Self>) {
        let font_size = (ReaderSettings::preferences(cx).font_size + delta).clamp(FONT_SIZE_MIN, FONT_SIZE_MAX);
        self.apply_appearance(cx, |preferences| preferences.font_size = font_size, |_| RendererCommand::SetFontSize(font_size));
    }

    pub(crate) fn set_reader_font_family(&self, font_family: ReaderFontFamily, cx: &mut Context<Self>) {
        self.apply_style_override(cx, |preferences| preferences.font_family = font_family);
    }

    pub(crate) fn set_reader_text_alignment(&self, text_alignment: ReaderTextAlignment, cx: &mut Context<Self>) {
        self.apply_style_override(cx, |preferences| preferences.text_alignment = text_alignment);
    }

    pub(crate) fn adjust_line_height(&self, delta: f32, cx: &mut Context<Self>) {
        let line_height = (ReaderSettings::preferences(cx).line_height + delta).clamp(LINE_HEIGHT_MIN, LINE_HEIGHT_MAX);
        self.apply_style_override(cx, |preferences| preferences.line_height = line_height);
    }

    pub(crate) fn adjust_desired_column_count(&mut self, delta: i8, window: &mut Window, cx: &mut Context<Self>) {
        const MIN_COLUMN_GAP: f64 = 20.0;
        let count = (i16::from(self.desired_column_count) + i16::from(delta)).clamp(i16::from(COLUMN_COUNT_MIN), i16::from(COLUMN_COUNT_MAX)) as u8;
        if count == self.desired_column_count {
            return;
        }
        self.desired_column_count = count;
        let preferences = ReaderSettings::preferences(cx);
        let split_sidebar_visible = !cfg!(feature = "kobo") && (self.reader_state_visible || self.toc_visible);
        let sidebar_width = if split_sidebar_visible { f64::from(preferences.toc_width) } else { 0.0 };
        let viewport_width = (f64::from(f32::from(window.viewport_size().width)) - sidebar_width).max(1.0) / preferences.scale.max(0.1);
        // Fitting more columns can need a narrower column than the user is
        // allowed to dial in by hand, so this floor is deliberately below
        // LINE_WIDTH_MIN.
        let width = (((viewport_width + MIN_COLUMN_GAP) / f64::from(count)) - MIN_COLUMN_GAP - 0.01).clamp(FITTED_COLUMN_WIDTH_MIN, LINE_WIDTH_MAX);
        self.apply_appearance(cx, |preferences| preferences.column_width = width, |_| RendererCommand::SetColumnWidth(width));
    }

    pub(crate) fn navigate_to(&self, href: String, cx: &mut Context<Self>) {
        self.with_renderer(cx, |renderer, cx| {
            renderer.apply(RendererCommand::NavigateToHref(href), cx);
        });
    }

    pub(crate) fn navigate_from_footnote(&mut self, cx: &mut Context<Self>) {
        let Some(preview) = self.footnote.take() else { return };
        self.navigate_to(preview.href, cx);
        crate::invalidation::notify(cx, Component::ReaderShell, "state_changed");
    }

    pub(crate) fn navigate_history(&self, direction: i8, cx: &mut Context<Self>) {
        self.with_renderer(cx, |renderer, cx| {
            renderer.apply(RendererCommand::NavigateHistory(direction), cx);
        });
    }

    pub(crate) fn adjust_column_width(&self, delta: f64, cx: &mut Context<Self>) {
        let width = (ReaderSettings::preferences(cx).column_width + delta).clamp(LINE_WIDTH_MIN, LINE_WIDTH_MAX);
        self.apply_appearance(cx, |preferences| preferences.column_width = width, |_| RendererCommand::SetColumnWidth(width));
    }

    pub(crate) fn adjust_scale(&self, delta: f64, cx: &mut Context<Self>) {
        let scale = (ReaderSettings::preferences(cx).scale + delta).clamp(ZOOM_MIN, ZOOM_MAX);
        self.apply_appearance(cx, |preferences| preferences.scale = scale, |_| RendererCommand::SetScale(scale));
    }

    pub(crate) fn set_search_active(&mut self, active: bool, cx: &mut Context<Self>) {
        if self.search.active == active {
            return;
        }
        self.search.active = active;
        self.with_renderer(cx, |renderer, cx| {
            renderer.apply(RendererCommand::SetSearchActive(active), cx);
        });
        crate::invalidation::notify(cx, Component::ReaderShell, "state_changed");
    }

    pub(crate) fn close_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search.clear();
        self.search_input.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
        self.with_renderer(cx, |renderer, cx| {
            renderer.apply(RendererCommand::SetSearchQuery(String::new()), cx);
            renderer.apply(RendererCommand::SetSearchActive(false), cx);
        });
        crate::invalidation::notify(cx, Component::ReaderShell, "state_changed");
    }

    pub(crate) fn display_title(&self) -> String {
        let chapter = self.toc.active_title();
        let chapter = chapter.or_else(|| (self.title != self.book_title).then(|| self.title.clone()));
        match chapter {
            Some(chapter) => format!("{} • {chapter}", self.book_title),
            None => self.book_title.clone(),
        }
    }

    pub(crate) fn queue_search(&mut self, query: String, cx: &mut Context<Self>) {
        let generation = self.search.set_query(query);
        let timer = cx.background_executor().timer(SEARCH_DEBOUNCE);
        self.search.debounce = Some(cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| {
                if this.search.is_current(generation) {
                    this.apply_pending_search(cx);
                }
            });
        }));
    }

    fn apply_pending_search(&mut self, cx: &mut Context<Self>) -> bool {
        let Some((renderer_query, options)) = self.search.take_pending() else {
            return false;
        };
        if options.scope == SearchScope::CurrentDocument || renderer_query.is_empty() {
            self.with_renderer(cx, move |renderer, cx| renderer.apply(RendererCommand::SetSearch { query: renderer_query, options }, cx));
            return true;
        }
        let generation = self.search.generation();
        let provider = self.publication.provider.clone();
        let documents = self.publication.document_uris.clone();
        let cancellation = self.search.cancellation();
        let worker = ReaderServices::get(cx).blocking.clone();
        self.with_renderer(cx, |renderer, cx| {
            renderer.apply(RendererCommand::SetSearchResults { query: renderer_query.clone(), options, results: Vec::new() }, cx);
        });
        self.search.index_task = Some(cx.spawn(async move |this, cx| {
            let query = renderer_query.clone();
            let (batch_tx, batch_rx) = async_channel::unbounded::<Vec<html_view_core::BookSearchMatch>>();
            let result_rx = worker.dispatch(move || {
                let results = html_view_core::search_publication_streaming(
                    provider.as_ref(),
                    &documents,
                    &query,
                    options,
                    || cancellation.load(std::sync::atomic::Ordering::Acquire) != generation,
                    |batch| {
                        let _ = batch_tx.send_blocking(batch.to_vec());
                    },
                );
                Ok::<_, String>(results.len())
            });
            while let Ok(batch) = batch_rx.recv().await {
                let _ = this.update(cx, |this, cx| {
                    if this.search.still_wants(generation, &renderer_query) {
                        this.with_renderer(cx, |renderer, cx| renderer.apply(RendererCommand::AppendSearchResults { query: renderer_query.clone(), options, results: batch }, cx));
                    }
                });
            }
            let result = result_rx.recv().await;
            let _ = this.update(cx, |this, _cx| {
                if !this.search.still_wants(generation, &renderer_query) {
                    return;
                }
                match result {
                    Ok(Ok(_)) => {}
                    Ok(Err(error)) => log::warn!("whole-book search failed: {error}"),
                    Err(_) => log::warn!("whole-book search worker stopped"),
                }
            });
        }));
        true
    }

    pub(crate) fn update_search_options(&mut self, update: impl FnOnce(&mut SearchFilters), cx: &mut Context<Self>) {
        self.search.update_filters(update);
        self.apply_pending_search(cx);
        crate::invalidation::notify(cx, Component::ReaderShell, "state_changed");
    }

    pub(crate) fn navigate_search(&self, direction: i8, cx: &mut Context<Self>) {
        self.with_renderer(cx, |renderer, cx| {
            renderer.apply(RendererCommand::NavigateSearch(direction), cx);
        });
    }
}
