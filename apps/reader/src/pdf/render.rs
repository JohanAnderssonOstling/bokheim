mod actions;
mod annotations;
mod init;
mod input;
mod navigation;
mod search;
mod settings;
#[cfg(all(any(feature = "native-tts", target_arch = "wasm32"), not(feature = "kobo")))]
mod tts;

use dictionary::can_lookup;
use gpui::prelude::*;
use gpui::{App, ClickEvent, Context, Entity, FocusHandle, Focusable, Pixels, Point, Render, Subscription, Window, px};
use gpui_component::input::InputState;
use gpui_component::menu::{ContextMenuExt, OpenContextMenu};
use gpui_component::tree::TreeState;
use library_backend::LibraryClient;
use library_model::BookLocator;
use pdf_view_gpui::{PdfSelectionSnapshot, PdfView};
use ui_components as components;

use std::rc::Rc;

use crate::invalidation::Component;
use crate::pdf::search::SearchList;
use crate::pdf::state::{page_indicator, progress};
use crate::pdf::toc::PdfTocState;
use crate::settings::ReaderSettings;
use crate::shell::annotation_editor;
use crate::shell::bookmarks;
use crate::shell::chrome;
use crate::shell::context_sheet::{ContextActionTarget, ContextActions, SelectionCommands};
use crate::shell::dictionary::{Closed, DictionaryPopup, SelectElement};
use crate::shell::input as shell_input;
use crate::shell::marks::MarkSession;
use crate::shell::page::ReaderSurface;
use crate::shell::persistence::ReadingPositionWriter;
use crate::shell::sidebar_list::SidebarList;
use crate::shell::state_panel::{AnnotationRow, SidebarState, SidebarTab};
use crate::shell::toolbar::{self as sidebar_chrome, SidebarContent};

/// One zoom step, in pixels per PDF point.
const ZOOM_STEP: f32 = 0.1;

#[derive(Clone)]
pub(crate) enum PdfContextCommand {
    Lookup,
    Translate,
    Annotation,
    Copy(Entity<PdfView>),
    CopySelectionWithSource,
}

pub(crate) struct PdfReaderView {
    dictionary: Option<Entity<crate::shell::dictionary::DictionaryPopup>>,
    #[cfg(feature = "kobo")]
    device_controls: Option<gpui::AnyView>,
    locator: BookLocator,
    title: String,
    library: Rc<LibraryClient>,
    pdf: Entity<PdfView>,
    search_input: Entity<InputState>,
    /// Match position while search is open; `None` means the search bar is closed.
    search_match: Option<(usize, usize)>,
    search_list: SearchList,
    toc_tree: Entity<TreeState>,
    toc_filter: Entity<crate::shell::toc_filter::TocFilter>,
    toc: PdfTocState,
    _toc_load: Option<gpui::Task<()>>,
    sidebar: SidebarState,
    chrome: chrome::ChromeState,
    marks: MarkSession,
    recurring: super::recurring::RecurringState,
    annotation_load_generation: u64,
    _annotation_updates: Option<gpui::Task<()>>,
    _bookmark_updates: gpui::Task<()>,
    annotation_list: SidebarList<AnnotationRow>,
    bookmark_list: bookmarks::BookmarkList,
    bookmark_filter_input: Entity<InputState>,
    annotation_note_input: Entity<InputState>,
    annotation_popup_anchor: Option<Point<Pixels>>,
    pending_open_annotation: Option<book_model::PdfAnnotationAnchor>,
    context_sheet: Option<ContextActions<PdfContextCommand>>,
    current_page: usize,
    history: crate::shell::history::NavigationHistory<pdf_view_gpui::PdfSourcePosition>,
    #[cfg(all(any(feature = "native-tts", target_arch = "wasm32"), not(feature = "kobo")))]
    tts: crate::shell::tts::Narration,
    #[cfg(all(any(feature = "native-tts", target_arch = "wasm32"), not(feature = "kobo")))]
    tts_fragments: Vec<Vec<pdf_reader_core::NormalizedRect>>,
    #[cfg(all(any(feature = "native-tts", target_arch = "wasm32"), not(feature = "kobo")))]
    tts_request: u64,
    #[cfg(all(any(feature = "native-tts", target_arch = "wasm32"), not(feature = "kobo")))]
    tts_page: usize,
    #[cfg(all(any(feature = "native-tts", target_arch = "wasm32"), not(feature = "kobo")))]
    tts_navigation_revision: u64,
    page_count: usize,
    positions: ReadingPositionWriter,
    close_reader: crate::CloseReader,
    focus: FocusHandle,
    sidebar_focus: FocusHandle,
    rail_focus: FocusHandle,
    settings_navigation: crate::shell::settings_navigation::SettingsNavigation,
    _subscriptions: Vec<Subscription>,
}

impl Focusable for PdfReaderView {
    /// Focusing the reader means focusing the document.
    ///
    /// Navigation keys are bound in the `PdfReaderDocument` context, which only
    /// the page elements establish, so anything that focuses this view has to
    /// land on the document or the keyboard goes dead. Before a page has been
    /// laid out there is nothing to focus, so the shell stands in.
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        if self.page_count > 0 { self.pdf.read(cx).focus_handle() } else { self.focus.clone() }
    }
}

impl Render for PdfReaderView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(all(target_os = "android", feature = "native-tts", not(feature = "kobo")))]
        self.tts.sync_volume_buttons(false, cx);
        components::sync_reader_rem_size(window, cx);
        Self::install_directional_page_keys(window, cx);
        crate::invalidation::record_render(Component::PdfShell);
        let theme = components::browser_theme(cx);
        let entity = cx.entity();
        let outline_available = self.toc.has_entries();
        let mobile = components::uses_mobile_reader_controls(window);
        if mobile {
            self.settings_navigation.keyboard_visibility().set(false);
        }
        let chrome_visible = self.chrome.is_visible();
        let touch_input = cx.supports_touch_input() || mobile;
        let overlay = chrome::sidebar_overlays_document(window);
        let active_tab = if overlay { self.sidebar.effective(outline_available) } else { Some(self.sidebar.effective_or_default(outline_available)) };
        let sidebar_visible = chrome_visible && active_tab.is_some();
        // A saved Search panel starts searching when first opened; hiding
        // it afterwards preserves the query, matches, and highlights.
        if sidebar_visible && active_tab == Some(SidebarTab::Search) {
            self.open_search(cx);
        }
        let toc_width = chrome::sidebar_width(sidebar_visible, window, cx);
        let available_width = (f32::from(window.viewport_size().width) - toc_width).max(0.0);
        let preferences = ReaderSettings::preferences(cx);
        let horizontal_margin = preferences.synchronized.pdf_horizontal_margin_px().min(((available_width - 100.0) / 2.0).max(0.0));
        let vertical_margin = preferences.synchronized.pdf_vertical_margin_px().min(((f32::from(window.viewport_size().height) - 100.0) / 2.0).max(0.0));
        let page_gap = preferences.synchronized.pdf_page_gap_px();
        let ruler_style = preferences.pdf_reading_ruler;
        let ruler_lines = preferences.pdf_reading_ruler_lines;
        let selection_handles_visible = self.dictionary.is_none() && self.marks.selected().is_none() && !sidebar_visible;
        self.pdf.update(cx, |pdf, cx| {
            pdf.set_selection_handles_visible(selection_handles_visible, cx);
            pdf.set_reading_ruler(ruler_style, ruler_lines, theme.accent, cx);
            pdf.set_page_gap(page_gap, cx);
            pdf.set_available_width((available_width - 2.0 * horizontal_margin).max(1.0), cx);
        });
        let (visible_page_range, visible_column_count) = {
            let pdf = self.pdf.read(cx);
            (pdf.visible_page_range(), pdf.visible_page_count())
        };
        let page_indicator = page_indicator(visible_page_range, self.page_count);
        let (progress_fraction, progress_label) = progress(visible_page_range, self.page_count);
        let section_title = self.active_outline_title().unwrap_or(&self.title).to_owned();

        let context_target = entity.clone();
        let pdf_document = components::reader_content()
            .id("pdf-reader-document")
            .px(px(horizontal_margin))
            .py(px(vertical_margin))
            .bg(theme.page_bg)
            .child(self.pdf.clone())
            .on_click(cx.listener(|reader, event, window, cx| {
                if chrome::is_page_tap(event) {
                    reader.context_sheet = None;
                    reader.toggle_mobile_controls(window, cx);
                }
            }))
            .on_aux_click(cx.listener(|reader, event: &ClickEvent, window, cx| {
                if event.is_secondary() && components::uses_mobile_reader_controls(window) {
                    reader.open_context_sheet(cx);
                }
            }))
            .on_long_press(cx.listener(|reader, _, _, cx| {
                reader.open_context_sheet(cx);
                cx.stop_propagation();
            }));
        let pdf_document = if mobile {
            pdf_document.on_action(cx.listener(|reader, _: &OpenContextMenu, _, cx| reader.open_context_sheet(cx))).into_any_element()
        } else {
            pdf_document.context_menu(move |menu, _window, cx| context_target.read(cx).context_actions(cx).popup(menu, &context_target)).into_any_element()
        };
        let mut body = components::reader_document_body().id("pdf-reader-document-surface").child(pdf_document);
        if let Some(annotation) = self.marks.selected().cloned() {
            let viewport = window.viewport_size();
            // The passage as the page columns drew it, so the panel is placed
            // clear of the whole highlight rather than of the pointer that made
            // it — which is on its last line, and would leave the panel over
            // everything above. The pointer remains the fallback for a passage
            // that is not on screen.
            let passage = self.pdf.read(cx).passage_bounds().map(annotation_editor::Passage::from_bounds).or_else(|| self.annotation_popup_anchor.map(annotation_editor::Passage::from_point));
            #[cfg(all(any(feature = "native-tts", target_arch = "wasm32"), not(feature = "kobo")))]
            let dock_height = if mobile && self.tts.dock_open() { components::player_dock_height_px(window) } else { 0.0 };
            #[cfg(not(all(any(feature = "native-tts", target_arch = "wasm32"), not(feature = "kobo"))))]
            let dock_height = 0.0;
            let mut insets = window.insets().effective();
            insets.bottom = (insets.bottom - px(dock_height)).max(px(0.0));
            let toolbar_height = if mobile { 0.0 } else { components::reader_toolbar_size_px(window) };
            let geometry = annotation_editor::geometry(passage, f32::from(viewport.width), f32::from(viewport.height) - dock_height, toc_width, toolbar_height, visible_column_count, insets, f32::from(window.rem_size()) * components::ANNOTATION_EDITOR_WIDTH_REM);
            body = body.child(self.render_annotation_popup(annotation, geometry, window, cx));
        }
        self.chrome.sync_system_bars(chrome_visible, cx);
        #[cfg(all(any(feature = "native-tts", target_arch = "wasm32"), not(feature = "kobo")))]
        let dock = self.tts.dock_open().then(|| crate::shell::tts::dock(&self.tts, mobile, theme, cx));
        #[cfg(not(all(any(feature = "native-tts", target_arch = "wasm32"), not(feature = "kobo"))))]
        let dock: Option<gpui::AnyElement> = None;
        let sidebar = active_tab.filter(|_| sidebar_visible).map(|active| {
            let sidebar_body = match active {
                SidebarTab::Contents => self.render_toc_body(entity.clone(), theme, cx),
                SidebarTab::Notes => self.render_annotations_body(theme, cx),
                SidebarTab::Bookmarks => self.render_bookmarks_body(theme, cx),
                SidebarTab::Search => self.render_search(theme, window, cx),
                SidebarTab::Settings => self.render_settings(window, cx),
            };
            let sidebar_content = SidebarContent::new(sidebar_body);
            let sidebar_content = if active == SidebarTab::Bookmarks { sidebar_content.with_header_action(bookmarks::create_button(&entity, self.toc.is_loaded())) } else { sidebar_content };
            sidebar_chrome::reader_sidebar(self, &entity, sidebar_content, theme, window, cx)
        });

        // The page container is only focusable until the document has pages of
        // its own to hold the keyboard. A focusable container steals focus from
        // the document on any click that reaches it — the sidebar's background,
        // the margin around a page — and navigation is bound in the document's
        // key context, so the paging keys would go dead until the page was
        // clicked again. The container still sits on the dispatch path either
        // way, so `Reader` bindings keep firing.
        let document_ready = self.page_count > 0;
        let surface = ReaderSurface {
            document: self.marks.with_persistence_notice(self.positions.with_notice(body.into_any_element(), theme), theme),
            dock,
            sidebar,
            active_tab,
            chrome_visible,
            document_ready,
            loading_focus: self.focus.clone(),
            title: self.title.clone(),
            chapter: section_title,
            progress_label,
            progress_fraction,
            progress_detail: page_indicator,
            bottom_bar_id: "pdf-reader-bottom-bar",
        };
        let page = crate::shell::page::reader_surface(self, surface, theme, window, cx)
            .id("pdf-reader-page")
            // Restore page-key focus only within the active window;
            // hovering a background reader must not raise it.
            .on_mouse_move(cx.listener(|reader: &mut Self, _, window, cx| reader.reclaim_hover_focus(window, cx)));
        let interactions = chrome::surface_interactions("pdf", sidebar_visible, touch_input, cx, |reader: &mut Self, action, window, cx| match action {
            chrome::SurfaceAction::PreviousPage => {
                reader.turn_horizontal(false, window, cx);
            }
            chrome::SurfaceAction::NextPage => {
                reader.turn_horizontal(true, window, cx);
            }
            chrome::SurfaceAction::ToggleChrome => reader.chrome.toggle(cx),
        });
        let mut page = page.children(interactions);
        page = page.children(self.dictionary.clone());
        if let Some(actions) = &self.context_sheet {
            if let Some(selection) = self.pdf.read(cx).passage_bounds() {
                let bounds = crate::shell::context_sheet::selection_toolbar_bounds(selection, window);
                page = page.child(actions.toolbar(bounds, theme, &entity));
            }
        }
        page
    }
}

impl PdfReaderView {
    fn toggle_mobile_controls(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !components::uses_mobile_reader_controls(window) || self.sidebar.is_visible() && self.chrome.is_visible() {
            return;
        }
        if self.chrome.is_visible() {
            self.chrome.hide(cx);
            self.focus_document(window, cx);
        } else {
            self.sidebar.close(cx);
            self.chrome.show_from_touch(cx);
        }
    }
}

pub(crate) fn bind_pdf_keys(cx: &mut App) {
    shell_input::bind_document_keys(cx, "PdfReaderDocument");
}

impl PdfReaderView {
    fn lookup_selection(&mut self, selection: PdfSelectionSnapshot, window: &mut Window, cx: &mut Context<Self>) {
        self.open_selection_popup(selection, false, window, cx);
    }

    fn open_selection_popup(&mut self, selection: PdfSelectionSnapshot, translate: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !translate && !can_lookup(&selection.text) {
            return;
        }
        self.context_sheet = None;
        self.marks.clear_selection();
        self.pdf.update(cx, |pdf, cx| pdf.set_active_annotation(None, cx));
        let anchor = self.pdf.read(cx).passage_bounds().or_else(|| self.annotation_popup_anchor.map(|position| gpui::Bounds::new(position, gpui::size(px(1.0), px(1.0)))));
        let dictionary = cx.new(|cx| {
            let popup = if translate { DictionaryPopup::translate(self.locator.book_id(), selection.text.clone(), Vec::new(), anchor, window, cx) } else { DictionaryPopup::new(selection.text.clone(), Vec::new(), anchor, window, cx) };
            popup.with_reader(self.library.clone(), self.locator.clone(), false)
        });
        cx.subscribe_in(&dictionary, window, |this, _, _: &Closed, window, cx| {
            this.dictionary = None;
            this.pdf.update(cx, |pdf, cx| pdf.clear_text_selection(cx));
            this.focus_document(window, cx);
            cx.notify();
        })
        .detach();
        cx.subscribe_in(&dictionary, window, |this, popup, _: &SelectElement, window, cx| {
            if !this.pdf.update(cx, |pdf, cx| pdf.select_containing_paragraph(cx)) {
                return;
            }
            let Some(selection) = this.pdf.read(cx).selection_snapshot() else { return };
            // Painting publishes PassageBoundsChanged to replace these
            // previous-frame bounds with the expanded paragraph's geometry.
            let anchor = this.pdf.read(cx).passage_bounds();
            if !can_lookup(&selection.text) {
                this.dictionary = None;
                this.focus_document(window, cx);
                cx.notify();
                return;
            }
            popup.update(cx, |popup, cx| popup.show_selection(selection.text, anchor, cx));
        })
        .detach();
        self.dictionary = Some(dictionary);
        cx.notify();
    }

    fn context_actions(&self, cx: &App) -> ContextActions<PdfContextCommand> {
        let mut actions = ContextActions::default();
        if let Some(selection) = self.pdf.read(cx).selection_snapshot() {
            actions = ContextActions::selection(SelectionCommands {
                annotate: Some(PdfContextCommand::Annotation),
                lookup: can_lookup(&selection.text).then_some(PdfContextCommand::Lookup),
                translate: PdfContextCommand::Translate,
                copy: PdfContextCommand::Copy(self.pdf.clone()),
                copy_link: Some(PdfContextCommand::CopySelectionWithSource),
            });
        }
        actions
    }

    fn open_context_sheet(&mut self, cx: &mut Context<Self>) {
        let Some(selection) = self.pdf.read(cx).selection_snapshot() else { return };
        let actions = ContextActions::mobile_selection(SelectionCommands {
            annotate: Some(PdfContextCommand::Annotation),
            lookup: can_lookup(&selection.text).then_some(PdfContextCommand::Lookup),
            translate: PdfContextCommand::Translate,
            copy: PdfContextCommand::Copy(self.pdf.clone()),
            copy_link: None,
        });
        if !actions.is_empty() {
            self.context_sheet = Some(actions);
            self.chrome.hide(cx);
            cx.notify();
        }
    }
}

impl ContextActionTarget for PdfReaderView {
    type Command = PdfContextCommand;

    fn run_context_action(&mut self, command: Self::Command, window: &mut Window, cx: &mut Context<Self>) {
        self.context_sheet = None;
        cx.notify();
        match command {
            PdfContextCommand::Lookup => {
                let Some(selection) = self.pdf.read(cx).selection_snapshot() else { return };
                self.lookup_selection(selection, window, cx);
            }
            PdfContextCommand::Translate => {
                let Some(selection) = self.pdf.read(cx).selection_snapshot() else { return };
                self.open_selection_popup(selection, true, window, cx);
            }
            PdfContextCommand::Annotation => self.create_annotation(window, cx),
            PdfContextCommand::CopySelectionWithSource => self.copy_selection_with_source(cx),
            PdfContextCommand::Copy(pdf) => {
                pdf.update(cx, |pdf, cx| {
                    pdf.copy_selection(cx);
                    pdf.clear_text_selection(cx);
                });
            }
        }
    }

    fn dismiss_context_actions(&mut self, cx: &mut Context<Self>) {
        self.context_sheet = None;
        cx.notify();
    }
}
