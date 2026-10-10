//! PDF reader action dispatch and chrome/sidebar transitions.

use crate::invalidation::notify;
use gpui::{Context, Window};

use crate::invalidation::Component;
use crate::shell::state_panel::{self, SidebarTab};
use crate::shell::toolbar::SidebarTarget;

use super::PdfReaderView;

impl PdfReaderView {
    pub(super) fn open_search(&mut self, cx: &mut Context<Self>) {
        if self.search_match.is_none() {
            self.search_match = Some((0, 0));
            notify(cx, Component::PdfShell, "search_opened");
        }
    }

    /// The contents/annotations/settings tab strip shared with the EPUB reader.
    pub(super) fn render_sidebar_tabs(&self, active: SidebarTab, outline_available: bool, theme: ui_components::BrowserTheme, cx: &mut Context<Self>) -> gpui::AnyElement {
        state_panel::reader_sidebar_tabs("pdf-sidebar-tabs", &cx.entity(), active, outline_available, theme, |tab, this, cx| {
            state_panel::select_tab(&mut this.sidebar, tab, cx);
            notify(cx, Component::PdfShell, "sidebar_tab_changed");
        })
    }
}

impl SidebarTarget for PdfReaderView {
    #[cfg(feature = "kobo")]
    fn brightness_controls(&self) -> gpui::Entity<crate::shell::brightness::BrightnessControls> {
        self.brightness.clone()
    }

    fn close_reader(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        (self.close_reader)(window, cx);
    }

    fn hide_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if ui_components::uses_mobile_navigation(window) {
            if self.sidebar.close() {
                notify(cx, Component::PdfShell, "sidebar_closed");
            }
        } else {
            self.chrome.hide(cx);
        }
        self.focus_document(window, cx);
    }

    fn sidebar_focus(&self) -> gpui::FocusHandle {
        self.sidebar_focus.clone()
    }

    fn sidebar_tab(&self) -> SidebarTab {
        self.sidebar.effective_or_default(self.contents_available())
    }

    fn contents_available(&self) -> bool {
        self.toc.has_entries()
    }

    fn select_sidebar_tab(&mut self, tab: SidebarTab, window: &mut Window, cx: &mut Context<Self>) {
        crate::shell::state_panel::select_tab(&mut self.sidebar, tab, cx);
        self.sidebar_focus.focus(window, cx);
        cx.notify();
    }

    fn toggle_sidebar_tab(&mut self, tab: SidebarTab, window: &mut Window, cx: &mut Context<Self>) {
        let outline_available = self.toc.has_entries();
        state_panel::toggle_tab(&mut self.sidebar, tab, outline_available, cx);
        self.chrome.show(cx);
        if self.sidebar.is_visible() {
            self.focus_document(window, cx);
        }
        notify(cx, Component::PdfShell, "mobile_sidebar_tab_changed");
    }

    fn focus_sidebar_body(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.sidebar_tab() == SidebarTab::Contents {
            self.toc_tree.update(cx, |tree, cx| {
                if tree.selected_index().is_none() {
                    tree.set_selected_index(Some(0), cx);
                }
                tree.focus(window, cx);
            });
        } else {
            window.focus_next(cx);
        }
    }

    fn settings_key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.handle_settings_key(key, window, cx)
    }

    fn search_is_open(&self) -> bool {
        self.search_match.is_some()
    }

    fn toggle_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        crate::shell::input::CommandTarget::execute(self, &crate::ToggleSearch, window, cx);
    }
}
