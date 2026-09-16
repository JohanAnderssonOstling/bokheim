//! PDF reader action dispatch and chrome/sidebar transitions.

use gpui::{Context, Focusable, Window};

use crate::invalidation::Component;
use crate::shell::state_panel::{self, SidebarPanel};
use crate::shell::toolbar::ToolbarTarget;

use super::PdfReaderView;

impl PdfReaderView {
    pub(super) fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.search_match.is_none() {
            self.search_match = Some((0, 0));
            crate::invalidation::notify(cx, Component::PdfShell, "search_opened");
        }
        self.search_input.read(cx).focus_handle(cx).focus(window, cx);
    }

    /// The contents/annotations sidebar shared with the EPUB reader.
    pub(super) fn render_sidebar_tabs(&self, show_annotations: bool, outline_available: bool, cx: &mut Context<Self>) -> gpui::AnyElement {
        crate::shell::state_panel::reader_sidebar_tabs("pdf-sidebar-tabs", &cx.entity(), show_annotations, outline_available, cx, |annotations, this, cx| {
            let panel = if annotations { SidebarPanel::Annotations } else { SidebarPanel::Contents };
            state_panel::select_panel(&mut this.sidebar, panel, cx);
            crate::invalidation::notify(cx, Component::PdfShell, "sidebar_tab_changed");
        })
    }
}

impl ToolbarTarget for PdfReaderView {
    #[cfg(feature = "kobo")]
    fn brightness_controls(&self) -> gpui::Entity<crate::shell::brightness::BrightnessControls> {
        self.brightness.clone()
    }

    fn close_reader(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        (self.close_reader)(window, cx);
    }

    fn toggle_sidebar(&mut self, contents_available: bool, cx: &mut Context<Self>) {
        state_panel::toggle_sidebar(&mut self.sidebar, contents_available, cx);
        crate::invalidation::notify(cx, Component::PdfShell, "sidebar_visibility_changed");
    }

    fn search_is_open(&self) -> bool {
        self.search_match.is_some()
    }

    fn toggle_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        crate::shell::input::CommandTarget::execute(self, &crate::ToggleSearch, window, cx);
    }

    /// An annotation the PDF itself carries is not ours to discard, so the
    /// toolbar makes no offer to.
    fn has_selected_annotation(&self) -> bool {
        self.marks.selected().is_some_and(|annotation| !library_model::ReaderAnnotation::is_external_pdf_annotation(annotation))
    }

    fn delete_selected_annotation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.marks.selected().map(|annotation| annotation.id.clone()) else { return };
        self.delete_annotation(id, cx);
        self.focus_document(window, cx);
    }
}
