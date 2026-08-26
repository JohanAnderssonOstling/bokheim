use std::sync::Arc;
use std::time::Duration;
use web_time::Instant;

use app_contract::reader_state::{AnnotationAnchor, AnnotationStyle, ReaderAnnotation, ReaderBookmark};
use gpui::prelude::*;
use gpui::{App, Context, FocusHandle, ScrollStrategy, Window};
use gpui_component::input::{InputEvent, InputState};
use gpui_component::tree::{TreeItem, TreeState};
use html_view_core::{InteractionPalette, RendererCommand, RendererEvent, RendererInitialConfig, SearchScope, TocEntry};
use html_view_gpui::HtmlView;

use crate::epub::marks::{self, Selection};
use crate::epub::progress::ProgressState;
use crate::epub::search::{SearchFilters, SearchState};
use crate::epub::toc::TocState;
use crate::epub::toolbar::{ReaderToolbar, ToolbarEvent, ToolbarModel};
use crate::epub::{LoadState, PendingReader, ReaderView};
use crate::invalidation::Component;
use crate::settings::{ReaderPreferences, ReaderSettings, library_paint_palette};
use crate::shell::marks::MarksState;
use crate::shell::persistence::{self, MutationWriter, PositionWriter};
use app_contract::preferences::{COLUMN_COUNT_MAX, COLUMN_COUNT_MIN, FONT_SIZE_MAX, FONT_SIZE_MIN, LINE_HEIGHT_MAX, LINE_HEIGHT_MIN, LINE_WIDTH_MAX, LINE_WIDTH_MIN, ReaderFontFamily, ReaderTextAlignment, ZOOM_MAX, ZOOM_MIN};

/// How long typing settles before a search is applied.
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(150);

fn update_cfi_position(current: &mut Option<String>, next: &Option<String>, fraction: f32) -> Option<(String, f32)> {
    if current == next {
        return None;
    }
    current.clone_from(next);
    current.clone().map(|cfi| (cfi, fraction))
}

/// Narrowest column the fit-columns control may produce. Below the
/// hand-adjustable minimum on purpose: fitting four columns into a narrow
/// window needs columns the width slider would refuse.
const FITTED_COLUMN_WIDTH_MIN: f64 = 80.0;

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
        search_active: false,
        search_query: String::new(),
        ..Default::default()
    }
}

impl ReaderView {
    pub(crate) fn new(pending: PendingReader, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let reader_started = Instant::now();
        startup_phase!("phase=reader_view_begin");
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

        let focus = cx.focus_handle();
        let toc_tree = cx.new(|cx| TreeState::new(cx));
        let toolbar_view = cx.new(|_| ReaderToolbar::default());
        let toolbar_subscription = cx.subscribe_in(&toolbar_view, window, |this, _, event: &ToolbarEvent, window, cx| {
            this.handle_toolbar_event(*event, window, cx);
        });
        focus.focus(window, cx);
        let reading_content_hash = pending.locator.content_hash();
        let reading_positions =
            PositionWriter::new(cx, "save reading position", move |library, (cfi, progress): &(String, f32)| library.update_reading_position(reading_content_hash.clone(), cfi.clone(), Some(*progress)), pending.library.clone());
        let mark_writes = MutationWriter::new(cx, pending.library.clone());

        let mut reader = Self {
            locator: pending.locator,
            library: pending.library,
            book_title: pending.title.clone(),
            title: pending.title,
            load_state: LoadState::Loading,
            desired_column_count: if cfg!(feature = "kobo") { 1 } else { ReaderSettings::preferences(cx).max_column_count },
            toc: TocState::default(),
            toc_tree,
            toc_visible: ReaderSettings::preferences(cx).toc_visible,
            chrome_hidden: false,
            search_input,
            search: SearchState::default(),
            publication: publication.clone(),
            current_cfi: None,
            progress: ProgressState::default(),
            toolbar_view,
            marks: MarksState::default(),
            selection: None,
            reader_state_visible: false,
            reader_state_bookmarks: false,
            annotation_note_input,
            annotation_popup_anchor: None,
            footnote: None,
            image_preview: None,
            can_history_back: false,
            can_history_forward: false,
            focus,
            renderer_subscription: None,
            reading_positions,
            mark_writes,
            return_to_library: pending.return_to_library,
            _subscriptions: vec![search_subscription, toolbar_subscription],
        };
        reader.finish_load(pending.preparation, window, cx);
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
        self.sync_toolbar("reader_ready", cx);
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

    fn load_reader_state(&mut self, cx: &mut Context<Self>) {
        let content_hash = self.locator.content_hash();
        persistence::write(
            cx,
            self.library.clone(),
            "load reader state",
            move |library| {
                let annotations = library.annotations(content_hash.clone());
                let bookmarks = library.bookmarks(content_hash);
                Box::pin(async move { Ok((annotations.await?, bookmarks.await?)) })
            },
            |this, cx, (annotations, bookmarks)| {
                this.marks.annotations = annotations;
                this.marks.bookmarks = bookmarks;
                let overlays = marks::overlays(&this.marks);
                this.with_renderer(cx, |renderer, cx| renderer.apply(RendererCommand::SetAnnotations(overlays), cx));
                crate::invalidation::notify(cx, Component::ReaderShell, "marks_loaded");
            },
        );
    }

    pub(crate) fn highlight_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(selection) = self.selection.clone() else { return };
        self.annotation_note_input.update(cx, |input, cx| input.set_value(String::new(), window, cx));
        let now = crate::shell::state_panel::now_millis();
        let annotation = ReaderAnnotation {
            id: uuid::Uuid::new_v4().to_string(),
            content_hash: self.locator.content_hash(),
            anchor: AnnotationAnchor::epub_cfi(selection.cfi_range),
            exact_text: selection.text,
            style: AnnotationStyle::Highlight,
            color: "#f6c945".to_owned(),
            note: String::new(),
            created_at: now,
            modified_at: now,
        };
        let overlay = marks::renderer_annotation(&annotation).expect("new EPUB annotation has an EPUB anchor");
        self.marks.selected_id = Some(annotation.id.clone());
        self.marks.annotations.push(annotation.clone());
        self.selection = None;
        self.with_renderer(cx, |renderer, cx| renderer.apply(RendererCommand::UpsertAnnotation(overlay), cx));
        crate::invalidation::notify(cx, Component::ReaderShell, "annotation_created");

        self.mark_writes.record("save reader annotation", move |library| library.upsert_annotation(annotation));
    }

    pub(crate) fn copy_selection_citation(&self, cx: &mut Context<Self>) {
        let title = self.display_title();
        let cfi = self.selection.as_ref().map(|selection| selection.cfi_range.clone()).or_else(|| self.current_cfi.clone());
        self.with_renderer(cx, |renderer, cx| renderer.apply(RendererCommand::CopySelectionCitation { title, cfi }, cx));
    }

    pub(crate) fn select_annotation(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let note = self.marks.annotation(&id).map(|annotation| annotation.note.clone()).unwrap_or_default();
        self.marks.selected_id = Some(id);
        self.annotation_note_input.update(cx, |input, cx| input.set_value(note, window, cx));
        crate::invalidation::notify(cx, Component::ReaderShell, "annotation_selected");
    }

    pub(crate) fn close_annotation_popup(&mut self, cx: &mut Context<Self>) {
        if self.marks.selected_id.take().is_some() {
            self.annotation_popup_anchor = None;
            crate::invalidation::notify(cx, Component::ReaderShell, "annotation_closed");
        }
    }

    pub(crate) fn navigate_to_cfi(&self, cfi: String, cx: &mut Context<Self>) {
        self.with_renderer(cx, |renderer, cx| renderer.apply(RendererCommand::SetCfiPosition(Some(cfi)), cx));
    }

    /// Writes the open annotation back, optionally restyling or recolouring it.
    ///
    /// The editor stays open afterwards so several styles or colours can be
    /// tried against the text; [`Self::close_annotation_popup`] is what
    /// dismisses it.
    pub(crate) fn save_selected_annotation(&mut self, style: Option<AnnotationStyle>, color: Option<&'static str>, cx: &mut Context<Self>) {
        let Some(id) = self.marks.selected_id.clone() else { return };
        let Some(mut annotation) = self.marks.annotation(&id).cloned() else { return };
        annotation.note = self.annotation_note_input.read(cx).value().to_string();
        if let Some(style) = style {
            annotation.style = style;
        }
        if let Some(color) = color {
            annotation.color = color.to_owned();
        }
        annotation.modified_at = crate::shell::state_panel::now_millis();
        if let Some(overlay) = marks::renderer_annotation(&annotation) {
            self.with_renderer(cx, |renderer, cx| renderer.apply(RendererCommand::UpsertAnnotation(overlay), cx));
        }
        self.marks.replace(annotation.clone());
        crate::invalidation::notify(cx, Component::ReaderShell, "annotation_updated");

        self.mark_writes.record("update reader annotation", move |library| library.upsert_annotation(annotation));
    }

    pub(crate) fn delete_annotation(&mut self, id: String, cx: &mut Context<Self>) {
        let modified_at = crate::shell::state_panel::now_millis();
        self.marks.remove_annotation(&id);
        self.annotation_popup_anchor = None;
        self.with_renderer(cx, |renderer, cx| renderer.apply(RendererCommand::RemoveAnnotation(id.clone()), cx));
        crate::invalidation::notify(cx, Component::ReaderShell, "annotation_deleted");

        self.mark_writes.record("delete reader annotation", move |library| library.delete_annotation(id, modified_at));
    }

    pub(crate) fn add_bookmark(&mut self, cx: &mut Context<Self>) {
        let Some(cfi) = self.current_cfi.clone() else { return };
        let now = crate::shell::state_panel::now_millis();
        let bookmark = ReaderBookmark { id: uuid::Uuid::new_v4().to_string(), content_hash: self.locator.content_hash(), cfi, label: self.display_title(), created_at: now, modified_at: now };
        self.marks.bookmarks.push(bookmark.clone());
        self.reader_state_visible = true;
        self.reader_state_bookmarks = true;
        crate::invalidation::notify(cx, Component::ReaderShell, "bookmark_created");

        self.mark_writes.record("save bookmark", move |library| library.create_bookmark(bookmark));
    }

    pub(crate) fn delete_bookmark(&mut self, id: String, cx: &mut Context<Self>) {
        let deleted_at = crate::shell::state_panel::now_millis();
        self.marks.remove_bookmark(&id);
        crate::invalidation::notify(cx, Component::ReaderShell, "bookmark_deleted");

        self.mark_writes.record("delete bookmark", move |library| library.delete_bookmark(id, deleted_at));
    }

    fn handle_renderer_event(&mut self, event: &RendererEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            RendererEvent::CfiChanged(cfi) => {
                if self.current_cfi != *cfi {
                    let bookmark_availability_changed = self.current_cfi.is_some() != cfi.is_some();
                    if let Some(position) = update_cfi_position(&mut self.current_cfi, cfi, self.progress.fraction) {
                        self.reading_positions.record(position, "reading-position");
                    }
                    if bookmark_availability_changed {
                        crate::invalidation::notify(cx, Component::ReaderShell, "bookmark_availability_changed");
                    }
                }
            }
            RendererEvent::TitleChanged(title) => {
                let title = title.clone().unwrap_or_else(|| self.book_title.clone());
                if self.title != title {
                    self.title = title;
                    self.sync_toolbar("title_changed", cx);
                }
            }
            RendererEvent::TocChanged(_) => {}
            RendererEvent::NavAnchorChanged { doc, anchor } => {
                let next = self.toc.link_at(*doc, anchor);
                if self.toc.active_link != next {
                    self.toc_tree.update(cx, |tree, cx| {
                        if let Some(link) = next.as_ref() {
                            let selected = TreeItem::new(link.clone(), link.clone());
                            tree.set_selected_item(Some(&selected), cx);
                            tree.reveal_item(&selected.id, ScrollStrategy::Center, cx);
                        } else {
                            tree.set_selected_item(None, cx);
                        }
                    });
                    self.toc.active_link = next;
                    self.sync_toolbar("toc_position_changed", cx);
                }
            }
            RendererEvent::HistoryAvailability { back, forward } => {
                if self.can_history_back != *back || self.can_history_forward != *forward {
                    self.can_history_back = *back;
                    self.can_history_forward = *forward;
                    self.sync_toolbar("history_availability_changed", cx);
                }
            }
            RendererEvent::SearchActiveChanged(active) => {
                if self.search.active != *active {
                    self.search.active = *active;
                    crate::invalidation::notify(cx, Component::ReaderShell, "search_visibility_changed");
                }
            }
            RendererEvent::FontSizeChanged(font_size) => {
                if ReaderSettings::update(cx, |preferences| preferences.font_size = *font_size) {
                    self.sync_toolbar("font_size_changed", cx);
                }
            }
            RendererEvent::ColumnWidthChanged(column_width) => {
                if ReaderSettings::update(cx, |preferences| preferences.column_width = *column_width) {
                    self.sync_toolbar("column_width_changed", cx);
                }
            }
            RendererEvent::ScaleChanged(scale) => {
                if ReaderSettings::update(cx, |preferences| preferences.scale = *scale) {
                    self.sync_toolbar("scale_changed", cx);
                }
            }
            RendererEvent::ReadingProgress { fraction, location, total_locations, doc, doc_count, .. } => {
                // TODO(2026-08-19): wire this to the renderer's `doc_fraction`
                // field once HtmlViewCore has been updated to emit it.
                if self.progress.update(*fraction, 0.0, *location, *total_locations, *doc, *doc_count) {
                    self.record_current_reading_position();
                    self.sync_toolbar("reading_progress_changed", cx);
                }
            }
            RendererEvent::SelectionFinished { cfi_range, exact_text, .. } => {
                if marks::set_selection(&mut self.selection, Selection { cfi_range: cfi_range.clone(), text: exact_text.clone() }) {
                    self.annotation_popup_anchor = Some(window.mouse_position());
                    crate::invalidation::notify(cx, Component::ReaderShell, "selection_finished");
                }
            }
            RendererEvent::AnnotationActivated { id } => {
                self.annotation_popup_anchor = Some(window.mouse_position());
                self.select_annotation(id.clone(), window, cx);
            }
            RendererEvent::FootnoteOpened(preview) => {
                let prepared = match &self.load_state {
                    LoadState::Ready(renderer) => renderer.update(cx, |view, _| view.prepare_note()),
                    LoadState::Loading => None,
                };
                self.footnote = prepared.map(|prepared| (preview.clone(), prepared));
                crate::invalidation::notify(cx, Component::ReaderShell, "footnote_opened");
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
                crate::invalidation::notify(cx, Component::ReaderShell, "image_preview_opened");
            }
            RendererEvent::MatchInfo { current, total } => {
                let next = Some((*current, *total));
                if self.search.match_position != next {
                    self.search.match_position = next;
                    crate::invalidation::notify(cx, Component::ReaderShell, "search_match_changed");
                }
            }
            RendererEvent::SearchResults(_) => {}
            RendererEvent::OperationFailed { operation, message } => {
                log::warn!("renderer failed to {operation}: {message}");
            }
            _ => {}
        }
    }

    /// CFI and progress are emitted as separately deduplicated renderer
    /// signals. Persist whenever either half changes: tying the durable CFI to
    /// progress alone loses every page turn whose rounded/synthetic progress
    /// happens to stay constant.
    fn record_current_reading_position(&self) {
        if let Some(cfi) = self.current_cfi.clone() {
            self.reading_positions.record((cfi, self.progress.fraction), "reading-position");
        }
    }

    /// Synchronizes the toolbar's projection from the authoritative reader
    /// state. The toolbar performs an equality check before repainting.
    pub(crate) fn sync_toolbar(&self, reason: &'static str, cx: &mut Context<Self>) {
        let model = self.toolbar_model(cx);
        self.toolbar_view.update(cx, |toolbar, cx| toolbar.set_model(model, reason, cx));
    }

    fn toolbar_model(&self, cx: &App) -> ToolbarModel {
        ToolbarModel {
            title: self.spine_title(),
            can_history_back: self.can_history_back,
            can_history_forward: self.can_history_forward,
            progress: self.progress.summary(),
            column_count: self.desired_column_count,
            preferences: ReaderSettings::preferences(cx).clone(),
            document_focus: self.document_focus(cx),
        }
    }

    fn handle_toolbar_event(&mut self, event: ToolbarEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            ToolbarEvent::ReturnToLibrary => (self.return_to_library)(window, cx),
            ToolbarEvent::NavigateHistory(-1) if self.can_history_back => self.navigate_history(-1, cx),
            ToolbarEvent::NavigateHistory(1) if self.can_history_forward => self.navigate_history(1, cx),
            ToolbarEvent::NavigateHistory(_) => {}
            ToolbarEvent::ToggleSidebar => {
                if self.toc_visible || self.reader_state_visible {
                    self.set_toc_visible(false, cx);
                    self.reader_state_visible = false;
                } else {
                    self.set_toc_visible(true, cx);
                }
                crate::invalidation::notify(cx, Component::ReaderShell, "sidebar_visibility_changed");
            }
            ToolbarEvent::AdjustColumnWidth(delta) => self.adjust_column_width(delta, cx),
            ToolbarEvent::AdjustColumnCount(delta) => self.adjust_desired_column_count(delta, window, cx),
            ToolbarEvent::AdjustFontSize(delta) => self.adjust_font_size(delta, cx),
            ToolbarEvent::AdjustScale(delta) => self.adjust_scale(delta, cx),
            ToolbarEvent::AdjustLineHeight(delta) => self.adjust_line_height(delta, cx),
            ToolbarEvent::SetFontFamily(family) => self.set_reader_font_family(family, cx),
            ToolbarEvent::SetTextAlignment(alignment) => self.set_reader_text_alignment(alignment, cx),
        }
    }

    fn set_toc(&mut self, entries: Vec<TocEntry>, resolutions: Vec<crate::epub::toc::ResolvedTocEntry>, cx: &mut Context<Self>) {
        let tree_items = Self::toc_tree_items(&entries);
        self.toc_tree.update(cx, |tree, cx| {
            tree.set_items(tree_items, cx);
        });
        self.toc.set(entries, resolutions);
    }

    fn toc_tree_items(entries: &[TocEntry]) -> Vec<TreeItem> {
        entries.iter().map(|entry| TreeItem::new(entry.link.clone(), entry.title.clone()).children(Self::toc_tree_items(&entry.children))).collect()
    }

    /// The document's focus handle, once the publication has loaded.
    ///
    /// Reader navigation is bound in the `ReaderDocument` key context, which
    /// the HTML view establishes while it holds focus, so this is the handle
    /// that has to be focused for the keyboard to work.
    pub(crate) fn document_focus(&self, cx: &App) -> Option<FocusHandle> {
        match &self.load_state {
            LoadState::Ready(renderer) => Some(renderer.read(cx).focus_handle()),
            _ => None,
        }
    }

    /// Returns keyboard control to the document.
    ///
    /// Called after any interaction that moves focus elsewhere — closing
    /// search, following a table-of-contents entry — so that paging keys work
    /// again without the reader having to click the text first.
    pub(crate) fn focus_document(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(focus) = self.document_focus(cx) {
            focus.focus(window, cx);
        }
    }

    pub(crate) fn with_renderer(&self, cx: &mut Context<Self>, update: impl FnOnce(&mut HtmlView, &mut Context<HtmlView>)) {
        if let LoadState::Ready(renderer) = &self.load_state {
            renderer.update(cx, update);
        }
    }

    pub(crate) fn previous_page(&self, cx: &mut Context<Self>) {
        // Progress is an asynchronously delivered projection used for display
        // and persistence. It can still contain its initial 1-of-1 value when
        // the first hardware key arrives, so it must not gate navigation. The
        // renderer owns the authoritative boundary checks and safely no-ops at
        // the beginning of the publication.
        // Kobo reader pages deliberately stay on the normal grayscale refresh
        // path. DU is faster, but it cannot preserve the cream page background
        // or the antialiased edges of text.
        self.with_renderer(cx, |renderer, cx| {
            renderer.previous_page(cx);
        });
    }

    pub(crate) fn next_page(&self, cx: &mut Context<Self>) {
        // See `previous_page`: renderer position is authoritative, while the
        // progress model may lag behind renderer creation or a page turn.
        // Keep the same quality refresh behavior as `previous_page`.
        self.with_renderer(cx, |renderer, cx| {
            renderer.next_page(cx);
        });
    }

    pub(crate) fn copy_preview_image(&self, cx: &mut Context<Self>) {
        if let Some((_, image)) = &self.image_preview {
            cx.write_to_clipboard(gpui::ClipboardItem::new_image(image));
        }
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
        self.sync_toolbar("preferences_changed", cx);
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

    pub(crate) fn reset_appearance(&mut self, cx: &mut Context<Self>) {
        let defaults = ReaderPreferences::default();
        let default_column_count = if cfg!(feature = "kobo") { 1 } else { defaults.max_column_count };
        let desired_columns_changed = self.desired_column_count != default_column_count;
        self.desired_column_count = default_column_count;
        let changed = ReaderSettings::update(cx, |preferences| {
            preferences.font_size = defaults.font_size;
            preferences.column_width = defaults.column_width;
            preferences.scale = defaults.scale;
            preferences.font_family = defaults.font_family;
            preferences.text_alignment = defaults.text_alignment;
            preferences.line_height = defaults.line_height;
            preferences.max_column_count = defaults.max_column_count;
        });
        if !changed && !desired_columns_changed {
            return;
        }
        let preferences = ReaderSettings::preferences(cx).clone();
        self.with_renderer(cx, |renderer, cx| {
            renderer.apply(RendererCommand::SetFontSize(preferences.font_size), cx);
            renderer.apply(RendererCommand::SetColumnWidth(if cfg!(feature = "kobo") { LINE_WIDTH_MAX } else { preferences.column_width }), cx);
            renderer.apply(RendererCommand::SetMaxColumnCount(cfg!(feature = "kobo").then_some(1)), cx);
            renderer.apply(RendererCommand::SetScale(preferences.scale), cx);
            renderer.apply(RendererCommand::SetReaderStyleOverrides(preferences.style_overrides()), cx);
        });
        self.sync_toolbar("preferences_reset", cx);
    }

    pub(crate) fn adjust_line_height(&self, delta: f32, cx: &mut Context<Self>) {
        let line_height = (ReaderSettings::preferences(cx).line_height + delta).clamp(LINE_HEIGHT_MIN, LINE_HEIGHT_MAX);
        self.apply_style_override(cx, |preferences| preferences.line_height = line_height);
    }

    pub(crate) fn adjust_desired_column_count(&mut self, delta: i8, window: &mut Window, cx: &mut Context<Self>) {
        if cfg!(feature = "kobo") {
            return;
        }
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
        self.sync_toolbar("column_count_changed", cx);
    }

    pub(crate) fn navigate_to(&self, href: String, cx: &mut Context<Self>) {
        self.with_renderer(cx, |renderer, cx| {
            renderer.apply(RendererCommand::NavigateToHref(href), cx);
        });
    }

    pub(crate) fn navigate_from_footnote(&mut self, cx: &mut Context<Self>) {
        let Some((preview, _)) = self.footnote.take() else { return };
        self.navigate_to(preview.href, cx);
        crate::invalidation::notify(cx, Component::ReaderShell, "footnote_navigated");
    }

    pub(crate) fn navigate_history(&self, direction: i8, cx: &mut Context<Self>) {
        self.with_renderer(cx, |renderer, cx| {
            renderer.apply(RendererCommand::NavigateHistory(direction), cx);
        });
    }

    pub(crate) fn adjust_column_width(&self, delta: f64, cx: &mut Context<Self>) {
        if cfg!(feature = "kobo") {
            return;
        }
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
            renderer.apply(if active { RendererCommand::ActivateSearch } else { RendererCommand::DeactivateSearch }, cx);
        });
        crate::invalidation::notify(cx, Component::ReaderShell, "search_visibility_changed");
    }

    pub(crate) fn close_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search.clear();
        self.search_input.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
        self.with_renderer(cx, |renderer, cx| {
            renderer.apply(RendererCommand::SetSearchQuery(String::new()), cx);
            renderer.apply(RendererCommand::DeactivateSearch, cx);
        });
        // Focus is still on the search input that is about to disappear.
        self.focus_document(window, cx);
        crate::invalidation::notify(cx, Component::ReaderShell, "search_closed");
    }

    pub(crate) fn display_title(&self) -> String {
        let chapter = self.toc.active_title();
        let chapter = chapter.or_else(|| (self.title != self.book_title).then(|| self.title.clone()));
        match chapter {
            Some(chapter) => format!("{} • {chapter}", self.book_title),
            None => self.book_title.clone(),
        }
    }

    fn spine_title(&self) -> String {
        self.toc.active_title().or_else(|| (self.title != self.book_title).then(|| self.title.clone())).unwrap_or_default()
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
        let executor = cx.background_executor().clone();
        self.with_renderer(cx, |renderer, cx| {
            renderer.apply(RendererCommand::SetSearchResults { query: renderer_query.clone(), options, results: Vec::new() }, cx);
        });
        self.search.index_task = Some(cx.spawn(async move |this, cx| {
            let query = renderer_query.clone();
            let (batch_tx, batch_rx) = async_channel::unbounded::<Vec<html_view_core::BookSearchMatch>>();
            let result = executor.spawn(async move {
                let results = html_view_core::search_publication_streaming(
                    provider.as_ref(),
                    &documents,
                    &query,
                    options,
                    || cancellation.load(std::sync::atomic::Ordering::Acquire) != generation,
                    |batch| {
                        let _ = batch_tx.try_send(batch.to_vec());
                    },
                );
                results.len()
            });
            while let Ok(batch) = batch_rx.recv().await {
                let _ = this.update(cx, |this, cx| {
                    if this.search.still_wants(generation, &renderer_query) {
                        this.with_renderer(cx, |renderer, cx| renderer.apply(RendererCommand::AppendSearchResults { query: renderer_query.clone(), options, results: batch }, cx));
                    }
                });
            }
            let _ = result.await;
        }));
        true
    }

    pub(crate) fn update_search_options(&mut self, update: impl FnOnce(&mut SearchFilters), cx: &mut Context<Self>) {
        self.search.update_filters(update);
        self.apply_pending_search(cx);
        crate::invalidation::notify(cx, Component::ReaderShell, "search_options_changed");
    }

    pub(crate) fn navigate_search(&self, direction: i8, cx: &mut Context<Self>) {
        self.with_renderer(cx, |renderer, cx| {
            renderer.apply(RendererCommand::NavigateSearch(direction), cx);
        });
    }
}

#[cfg(test)]
mod position_tests {
    use super::update_cfi_position;

    #[test]
    fn a_changed_cfi_is_persisted_even_when_progress_is_unchanged() {
        let mut current = Some("epubcfi(/6/14!/4/2[ch03]/2/2/2/1:0)".to_owned());
        let next = Some("epubcfi(/6/14!/4/2[ch03]/54/1:121)".to_owned());

        let update = update_cfi_position(&mut current, &next, 0.1875);

        assert_eq!(update, Some((next.clone().unwrap(), 0.1875)));
        assert_eq!(current, next);
    }

    #[test]
    fn an_identical_cfi_does_not_queue_another_write() {
        let mut current = Some("epubcfi(/6/14!/4/2[ch03]/54/1:121)".to_owned());
        let next = current.clone();

        assert_eq!(update_cfi_position(&mut current, &next, 0.1875), None);
    }
}
