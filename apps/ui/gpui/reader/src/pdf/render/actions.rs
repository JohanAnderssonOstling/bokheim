//! PDF reader action dispatch and chrome/sidebar transitions.

use gpui::{Context, Focusable, Window};

use crate::invalidation::Component;
use crate::shell::state_panel::{self, SidebarPanel};
use crate::shell::toolbar::ToolbarTarget;

use super::PdfReaderView;

impl PdfReaderView {
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
    fn return_to_library(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        (self.return_to_library)(window, cx);
    }

    fn toggle_sidebar(&mut self, contents_available: bool, cx: &mut Context<Self>) {
        state_panel::toggle_sidebar(&mut self.sidebar, contents_available, cx);
        crate::invalidation::notify(cx, Component::PdfShell, "sidebar_visibility_changed");
    }

    fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.search_match.is_none() {
            self.search_match = Some((0, 0));
            crate::invalidation::notify(cx, Component::PdfShell, "search_opened");
        }
        self.search_input.read(cx).focus_handle(cx).focus(window, cx);
    }
}
