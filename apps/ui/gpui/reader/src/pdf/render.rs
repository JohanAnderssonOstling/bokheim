mod actions;
mod annotations;
mod init;
mod input;
mod navigation;
mod search;
mod settings;

use bokheim_backend::LibraryBackend;
use gpui::prelude::*;
use gpui::{App, Context, Entity, FocusHandle, Focusable, Pixels, Point, Render, Subscription, Window};
use gpui_component::input::InputState;
use gpui_component::tree::TreeState;
use library_model::BookLocator;
use pdf_view_gpui::PdfView;
use ui_components as components;

use std::rc::Rc;

use crate::invalidation::Component;
use crate::pdf::state::{page_indicator, progress};
use crate::pdf::toc::PdfTocState;
use crate::shell::annotation_editor;
use crate::shell::chrome;
use crate::shell::input::{self as shell_input, CommandTarget};
use crate::shell::marks::MarkSession;
use crate::shell::persistence::PositionWriter;
use crate::shell::state_panel::{SidebarPanel, SidebarState};
use crate::shell::toolbar::{self, ToolbarContent};
use crate::{NextPage, PreviousPage};

/// One zoom step, in pixels per PDF point.
const ZOOM_STEP: f32 = 0.1;

pub(crate) struct PdfReaderView {
    locator: BookLocator,
    title: String,
    library: Rc<LibraryBackend>,
    pdf: Entity<PdfView>,
    search_input: Entity<InputState>,
    /// Match position while search is open; `None` means the search bar is closed.
    search_match: Option<(usize, usize)>,
    toc_tree: Entity<TreeState>,
    toc: PdfTocState,
    sidebar: SidebarState,
    chrome: chrome::ChromeState,
    marks: MarkSession,
    annotation_note_input: Entity<InputState>,
    annotation_popup_anchor: Option<Point<Pixels>>,
    current_page: usize,
    page_count: usize,
    positions: PositionWriter<(usize, f32)>,
    return_to_library: crate::ReturnToLibrary,
    focus: FocusHandle,
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
        crate::invalidation::record_render(Component::PdfShell);
        let theme = components::browser_theme(cx);
        let entity = cx.entity();
        let pdf_document_focus = self.pdf.read(cx).focus_handle();
        let outline_available = self.pdf.read(cx).info().is_some_and(|info| !info.outline().is_empty());
        let sidebar_visible = self.sidebar.shows(SidebarPanel::Annotations) || (self.sidebar.shows(SidebarPanel::Contents) && outline_available);
        let toc_width = chrome::sidebar_width(sidebar_visible, window, cx);
        let available_width = (f32::from(window.viewport_size().width) - toc_width).max(0.0);
        self.pdf.update(cx, |pdf, cx| {
            pdf.set_available_width(available_width, cx);
        });
        let (visible_page_range, selection_available, visible_column_count) = {
            let pdf = self.pdf.read(cx);
            (pdf.visible_page_range(), pdf.selection_snapshot().is_some(), pdf.visible_page_count())
        };
        let page_indicator = page_indicator(visible_page_range, self.page_count);
        let progress = progress(visible_page_range, self.page_count);
        let section_title = self.active_outline_title().unwrap_or(&self.title).to_owned();

        let mut body = components::reader_document_body().child(components::reader_content().child(self.pdf.clone()));
        if let Some(annotation) = self.marks.selected().cloned() {
            let viewport = window.viewport_size();
            let geometry = annotation_editor::geometry(self.annotation_popup_anchor, f32::from(viewport.width), f32::from(viewport.height), toc_width, components::READER_TOOLBAR_HEIGHT, visible_column_count);
            body = body.child(self.render_annotation_popup(annotation, geometry, cx));
        }
        if let Some((current_match, total_matches)) = self.search_match {
            body = body.child(crate::shell::search::search_bar(&entity, &self.search_input, (current_match, total_matches), Vec::new(), theme));
        }
        let content = match self.sidebar.active() {
            Some(SidebarPanel::Annotations) => chrome::with_sidebar(self.render_annotations(outline_available, cx), body.into_any_element(), window, cx),
            Some(SidebarPanel::Contents) if outline_available => chrome::with_sidebar(self.render_toc(entity.clone(), theme, cx), body.into_any_element(), window, cx),
            _ => body.into_any_element(),
        };
        let settings = self.render_settings(window, cx);
        // The PDF view element is what establishes `PdfReaderDocument`, so its
        // handle — not this page's — is the one the page keys need.
        let mut toolbar_content = ToolbarContent::new(section_title, progress, page_indicator, Some(pdf_document_focus), settings).contents_available(outline_available);
        if selection_available {
            toolbar_content = toolbar_content
                .right(components::reader_control_button("pdf-highlight-selection", "Highlight", theme).tooltip("Save selected PDF text as a highlight").on_click(cx.listener(|this, _, window, cx| this.create_annotation(false, window, cx))))
                .right(
                    components::reader_control_button("pdf-note-selection", "Note", theme).tooltip("Highlight selected PDF text and open annotations").on_click(cx.listener(|this, _, window, cx| this.create_annotation(true, window, cx))),
                );
        }
        let toolbar = toolbar::reader_toolbar(&entity, toolbar_content, theme);

        // The page container is only focusable until the document has pages of
        // its own to hold the keyboard. A focusable container steals focus from
        // the document on any click that reaches it — the toolbar's background,
        // the sidebar, the margin around a page — and navigation is bound in
        // the document's key context, so the paging keys would go dead until
        // the page was clicked again. The container still sits on the dispatch
        // path either way, so `Reader` bindings keep firing.
        let document_ready = self.page_count > 0;
        let page = crate::shell::page::reader_page(window, cx, theme, document_ready, &self.focus, self.chrome.is_visible().then_some(toolbar), content);
        let interactions = chrome::surface_interactions("pdf", self.chrome.is_visible(), sidebar_visible, components::READER_TOOLBAR_HEIGHT, cx.supports_touch_input(), cx, |reader: &mut Self, action, window, cx| match action {
            chrome::SurfaceAction::PreviousPage => {
                reader.execute(&PreviousPage, window, cx);
            }
            chrome::SurfaceAction::NextPage => {
                reader.execute(&NextPage, window, cx);
            }
            chrome::SurfaceAction::ToggleChrome => reader.chrome.toggle(cx),
            chrome::SurfaceAction::ShowChrome => reader.chrome.show(cx),
        });
        page.children(interactions)
    }
}

pub(crate) fn bind_pdf_keys(cx: &mut App) {
    shell_input::bind_document_keys(cx, "PdfReaderDocument");
}
