use ui_text::text;

mod annotations;
mod appearance;
mod events;
mod navigation;
mod search;

use std::{rc::Rc, sync::Arc};

use gpui::prelude::*;
use gpui::{App, Context, Window};
use gpui_component::input::{InputEvent, InputState};
use gpui_component::tree::TreeState;
use html_view_gpui::{DocumentTap, HtmlView};
use library_backend::LibraryClient;
use library_model::BookLocator;
use skrift_column_paginator::{RendererCommand, RendererEvent, RendererInitialConfig, RendererPreparation};

use crate::epub::toc::TocState;
use crate::epub::{LoadState, ProgressState, ReaderView, SearchState};
use crate::book::NativeBook;
use crate::invalidation::{Component, notify};
use crate::opening_error::opening_error;
use crate::settings::{ReaderSettings, library_interaction_palette, library_link_foreground, library_paint_palette, library_theme_adaptation};
use crate::shell::chrome::ChromeState;
use crate::shell::marks::MarkSession;
use crate::shell::persistence::ReadingPositionWriter;
use crate::shell::search::SearchBarTarget;
use crate::shell::settings_navigation::SettingsNavigation;
use crate::shell::state_panel::SidebarState;
#[cfg(all(any(feature = "native-tts", target_arch = "wasm32"), not(feature = "kobo")))]
use crate::shell::tts::Narration;
use client_protocol::{BackendError, Failure};

#[cfg(all(any(feature = "native-tts", target_arch = "wasm32"), not(feature = "kobo")))]
fn document_narration(title: &str, book: &NativeBook, cx: &mut Context<ReaderView>) -> Narration {
    let preferences = ReaderSettings::preferences(cx);
    Narration::for_languages(&book.languages())
        .with_title(title)
        .with_rate(preferences.synchronized.audiobook_speed())
        .with_pauses(preferences.narration_pauses)
        .with_voice(preferences.narration_voice)
        .with_device_voice(preferences.narration_device_voice.clone())
        .with_voice_preferences(preferences)
        .with_availability(cx)
}

pub(crate) fn renderer_initial_config(window: &Window, cx: &App) -> RendererInitialConfig {
    let chrome = ChromeState::from_preferences(Component::ReaderShell, ui_components::uses_mobile_reader_controls(window), cx);
    renderer_config(window, chrome.is_visible(), u8::MAX, cx)
}

fn renderer_config(window: &Window, sidebar_visible: bool, desired_column_count: u8, cx: &App) -> RendererInitialConfig {
    let preferences = ReaderSettings::preferences(cx);
    let (_, _, available_width) = ReaderView::horizontal_layout(window, cx, sidebar_visible);
    // Prepare at the width the first render will use, so installing the
    // document does not immediately lay it out a second time.
    let (column_count, column_width) = ReaderView::column_layout(f64::from(available_width), desired_column_count, cx);
    RendererInitialConfig {
        font_size: preferences.synchronized.font_size_px(),
        column_width,
        scale: preferences.synchronized.zoom(),
        max_column_count: Some(column_count),
        style_overrides: preferences.style_overrides(Some(library_theme_adaptation(cx)), library_link_foreground(cx)),
        paint_palette: library_paint_palette(cx),
        interaction_palette: library_interaction_palette(cx),
        ..Default::default()
    }
}

impl ReaderView {
    pub(crate) fn document_config(&self, window: &Window, cx: &App) -> RendererInitialConfig {
        let sidebar_visible = self.chrome.is_visible() && self.active_sidebar_tab(window).is_some();
        let mut config = renderer_config(window, sidebar_visible, self.desired_column_count, cx);
        ReaderSettings::preferences(cx).apply_cjk_font(&mut config.style_overrides, self.cjk_font);
        config
    }

    pub(crate) fn new(
        locator: BookLocator, title: String, library: Rc<LibraryClient>, book: Arc<NativeBook>, cjk_font: Option<crate::cjk::CjkFont>, preparation: RendererPreparation, save_progress: bool, close_reader: crate::CloseReader,
        #[cfg(feature = "kobo")] device_controls: Option<gpui::AnyView>, window: &mut Window, cx: &mut Context<Self>,
    ) -> Self {
        // Every open book observes changes made in other reader windows and
        // changes received through preference synchronization.
        cx.observe_global::<ReaderSettings>(|_: &mut Self, cx| cx.notify()).detach();
        let search_input = cx.new(|cx| {
            let input = InputState::new(window, cx);
            #[cfg(feature = "kobo")]
            let input = input.placeholder(text!(reader::search_current_chapter));
            input
        });
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
        crate::shell::bookmarks::watch_name_validity(&annotation_note_input, cx, |this| this.marks.selected().is_some_and(library_backend::Annotation::is_bookmark));
        let font_picker = cx.new(|cx| super::font_picker::FontPicker::new(window, cx));
        let cjk_font_picker = cx.new(|cx| {
            let mut picker = super::font_picker::FontPicker::new(window, cx);
            picker.set_cjk(true);
            picker
        });
        for picker in [&font_picker, &cjk_font_picker] {
            cx.subscribe_in(picker, window, |this, _, event: &super::font_picker::FontSelected, _, cx| {
                this.set_custom_font_family(event.role, event.family.clone(), cx);
            })
            .detach();
        }
        let bookmark_filter_input = crate::shell::bookmarks::filter_input(window, cx);
        let annotation_filter_input = cx.new(|cx| InputState::new(window, cx).placeholder(text!(reader::filter_annotations)));
        let annotation_filter_subscription = cx.subscribe(&annotation_filter_input, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                notify(cx, Component::ReaderShell, "annotation_filter_changed");
            }
        });
        let focus = cx.focus_handle();
        let toc_tree = cx.new(|cx| TreeState::new(cx));
        let reset_reader = cx.weak_entity();
        let toc_filter = cx.new(|cx| crate::shell::toc_filter::TocFilter::new(toc_tree.clone(), window, cx).on_reset(Rc::new(move |_, _, cx| {
            let _ = reset_reader.update(cx, |reader, cx| {
                reader.reading_positions.reset_read_status();
                reader.toc.read_status.reset();
                cx.notify();
            });
        })));
        focus.focus(window, cx);
        let reading_positions = ReadingPositionWriter::new(cx, locator.book_id(), library.clone()).with_saving_enabled(save_progress && !book.is_transient());
        reading_positions.watch(cx);
        let marks = MarkSession::new(cx, library.clone());
        let bookmark_updates = if book.is_transient() { gpui::Task::ready(()) } else { marks.watch_bookmarks(cx, Self::update_bookmarks) };

        #[cfg(all(any(feature = "native-tts", target_arch = "wasm32"), not(feature = "kobo")))]
        let tts = document_narration(&title, &book, cx);

        let mut toc = TocState::default();
        toc.cfi = book.cfi.clone();
        let image_canvas = cx.new(|_| ui_components::InfiniteCanvas::new(0.1, 16.0));
        cx.observe(&image_canvas, |_, _, cx| cx.notify()).detach();
        let mut reader = Self {
            dictionary: None,
            #[cfg(feature = "kobo")]
            device_controls,
            locator,
            library,
            book_title: title.clone(),
            title,
            load_state: LoadState::Loading,
            applied_palette: None,
            applied_font_size: None,
            applied_scale: None,
            applied_style_overrides: None,
            last_column_viewport_plan: None,
            desired_column_count: u8::MAX,
            toc,
            toc_tree,
            toc_filter,
            annotation_tree: Default::default(),
            bookmark_tree: Default::default(),
            sidebar: SidebarState::from_preferences(cx),
            chrome: ChromeState::from_preferences(Component::ReaderShell, ui_components::uses_mobile_reader_controls(window), cx),
            touchpad: ui_components::TouchpadNavigation::default(),
            touch_swipe: None,
            swipe_preview: ui_components::SwipePreview::default(),
            page_turn: None,
            last_wheel_turn: None,
            page_size: None,
            touch_swipe_from_edge: false,
            search_input,
            font_picker,
            cjk_font_picker,
            search: SearchState::default(),
            book,
            cjk_font,
            reading_position: None,
            history: Default::default(),
            document_history: None,
            #[cfg(all(any(feature = "native-tts", target_arch = "wasm32"), not(feature = "kobo")))]
            tts,
            #[cfg(all(any(feature = "native-tts", target_arch = "wasm32"), not(feature = "kobo")))]
            tts_fragments: Vec::new(),
            #[cfg(all(any(feature = "native-tts", target_arch = "wasm32"), not(feature = "kobo")))]
            tts_doc: 0,
            #[cfg(all(any(feature = "native-tts", target_arch = "wasm32"), not(feature = "kobo")))]
            tts_navigation_revision: 0,
            progress: ProgressState::default(),
            marks,
            recurring: Default::default(),
            annotation_load_generation: 0,
            _annotation_updates: None,
            _bookmark_updates: bookmark_updates,
            selection: None,
            touch_selection: Default::default(),
            annotation_note_input,
            annotation_filter_input,
            bookmark_filter_input,
            annotation_popup_anchor: None,
            footnote: None,
            image_preview: None,
            image_backdrop: None,
            save_opened_image: false,
            image_canvas,
            context_sheet: None,
            focus,
            sidebar_focus: cx.focus_handle().tab_index(0).tab_stop(true),
            rail_focus: cx.focus_handle().tab_stop(true),
            settings_navigation: SettingsNavigation::new(window, cx),
            renderer_subscription: None,
            document_tap_subscription: None,
            reading_positions,
            close_reader,
            _search_subscription: search_subscription,
            _annotation_filter_subscription: annotation_filter_subscription,
        };
        #[cfg(all(feature = "native-tts", target_os = "android"))]
        if !reader.book.is_transient() {
            crate::shell::tts::load_media_metadata(reader.library.clone(), reader.locator.clone(), cx);
        }
        reader.refresh_bookmark_rows(cx);
        reader.finish_load(preparation, window, cx);
        reader
    }

    fn clear_navigation_overlays(&mut self) {
        self.context_sheet = None;
        self.page_turn = None;
        self.swipe_preview.clear();
        self.footnote = None;
        self.image_preview = None;
    }

    /// Standalone articles share the reader and native caches across navigation.
    pub(crate) fn replace_document(&mut self, locator: BookLocator, title: String, book: Arc<NativeBook>, preparation: RendererPreparation, window: &mut Window, cx: &mut Context<Self>) -> Result<RendererPreparation, html::Error> {
        if !self.book.is_transient() || !book.is_transient() {
            return Err("document replacement requires a standalone reader".into());
        }
        let LoadState::Ready(renderer) = &self.load_state else { return Err("document reader unavailable".into()) };
        let previous = renderer.update(cx, |view, cx| view.replace_document(preparation, cx))?;
        #[cfg(all(any(feature = "native-tts", target_arch = "wasm32"), not(feature = "kobo")))]
        {
            self.tts.close();
            self.tts = document_narration(&title, &book, cx);
            self.tts_fragments.clear();
            self.tts_doc = 0;
            self.tts_navigation_revision = 0;
        }
        self.locator = locator;
        self.book_title = title.clone();
        self.title = title;
        self.book = book;
        self.dictionary = None;
        self.selection = None;
        self.touch_selection = Default::default();
        self.clear_navigation_overlays();
        self.annotation_popup_anchor = None;
        self.reading_position = None;
        self.progress = ProgressState::default();
        self.recurring = Default::default();
        self.search.clear();
        self.search_input.update(cx, |input, cx| input.set_value(String::new(), window, cx));
        self.touchpad = Default::default();
        self.touch_swipe = None;
        self.touch_swipe_from_edge = false;
        self.toc = TocState::default();
        self.toc.cfi = self.book.cfi.clone();
        self.toc_filter.update(cx, |filter, cx| filter.set_items(Vec::new(), cx));
        self.install_document_progress_weights(cx);
        self.load_database_toc(cx);
        self.focus_document(window, cx);
        cx.notify();
        Ok(previous)
    }

    fn finish_load(&mut self, preparation: RendererPreparation, window: &mut Window, cx: &mut Context<Self>) {
        let renderer = match HtmlView::create_from_preparation(preparation, window, cx) {
            Ok(renderer) => renderer,
            Err(error) => {
                log::error!("could not initialize EPUB renderer: {error}");
                let failure = opening_error(BackendError::operation(error), "renderer_initialize", Failure::BookRendererFailed);
                let message = ui_components::user_facing_error(&failure, text!(reader::couldn_t_display_this_book_reopen_the_app_and_retry)).to_owned();
                // The parent installs its failure subscription after this
                // constructor returns, before deferred events are delivered.
                cx.defer_in(window, move |_, _, cx| cx.emit(crate::BookOpenFailed(failure)));
                self.load_state = LoadState::Error(message);
                cx.notify();
                return;
            }
        };
        renderer.update(cx, |view, _| {
            // The wheel turns pages rather than scrolling lines, and page
            // turning is reader navigation, so the document must not consume
            // the event first.
            view.set_wheel_scroll_enabled(false);
        });
        self.renderer_subscription = Some(cx.subscribe_in(&renderer, window, |this, _, event: &RendererEvent, window, cx| {
            this.handle_renderer_event(event, window, cx);
        }));
        self.document_tap_subscription = Some(cx.subscribe_in(&renderer, window, |this, _, _: &DocumentTap, window, cx| {
            this.toggle_mobile_controls(window, cx);
        }));
        self.load_state = LoadState::Ready(renderer);
        self.install_document_progress_weights(cx);
        // The window focused the page container before there was a document to
        // focus. Hand keyboard control to the document now that one exists, so
        // paging works without clicking the text first.
        self.focus_document(window, cx);
        self.load_reader_state(cx);
        self.watch_annotation_updates(cx);
        self.load_database_toc(cx);
        notify(cx, Component::ReaderShell, "reader_ready");
    }

    /// Installs weights obtained from container metadata. For EPUB these are
    /// uncompressed ZIP entry sizes, so progress is available without opening
    /// and parsing every chapter. The renderer command retains its historical
    /// name, but consumes generic relative weights.
    fn install_document_progress_weights(&mut self, cx: &mut Context<Self>) {
        let weights = self.book.document_weights().to_vec();
        self.with_renderer(cx, |renderer, cx| renderer.apply(RendererCommand::SetDocumentTextLengths(weights), cx));
    }
}
