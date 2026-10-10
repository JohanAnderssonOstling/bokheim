//! EPUB implementations of shared navigation commands.

use crate::invalidation::notify;
use gpui::{Context, Focusable, Window};
use ui_components as components;

use crate::epub::ReaderView;
use crate::invalidation::Component;
use crate::shell::input::CommandTarget;
use crate::shell::search::SearchBarTarget;
use crate::shell::state_panel::{self, SidebarTab};
use crate::shell::toolbar::SidebarTarget;
use crate::{
    NextSidebarTab, PreviousSidebarTab,
    CloseSearch, DecreaseColumnWidth, DecreaseFont, DecreaseScale, FocusReaderSearch, IncreaseColumnWidth, IncreaseFont, IncreaseScale, NextLine, NextPage, NextSection, PreviousLine, PreviousPage, PreviousSection, ReturnToLibrary,
    ToggleFullscreen, ToggleSearch, ToggleSidebar, ToggleToc,
};

impl CommandTarget<PreviousPage> for ReaderView {
    fn execute(&mut self, _: &PreviousPage, window: &mut Window, cx: &mut Context<Self>) {
        if components::uses_mobile_navigation(window) {
            self.chrome.hide(cx);
        }
        // Renderer position is authoritative. Progress is an asynchronous
        // projection and may still contain its initial value on the first key.
        self.with_renderer(cx, |renderer, cx| {
            renderer.previous_page(cx);
        });
    }
}

impl CommandTarget<NextPage> for ReaderView {
    fn execute(&mut self, _: &NextPage, window: &mut Window, cx: &mut Context<Self>) {
        if components::uses_mobile_navigation(window) {
            self.chrome.hide(cx);
        }
        self.with_renderer(cx, |renderer, cx| {
            renderer.next_page(cx);
        });
    }
}

impl CommandTarget<PreviousLine> for ReaderView {
    fn execute(&mut self, _: &PreviousLine, window: &mut Window, cx: &mut Context<Self>) {
        if components::uses_mobile_navigation(window) {
            self.chrome.hide(cx);
        }
        self.with_renderer(cx, |renderer, cx| renderer.previous_line(cx));
    }
}

impl CommandTarget<NextLine> for ReaderView {
    fn execute(&mut self, _: &NextLine, window: &mut Window, cx: &mut Context<Self>) {
        if components::uses_mobile_navigation(window) {
            self.chrome.hide(cx);
        }
        self.with_renderer(cx, |renderer, cx| renderer.next_line(cx));
    }
}

impl CommandTarget<PreviousSection> for ReaderView {
    fn execute(&mut self, _: &PreviousSection, window: &mut Window, cx: &mut Context<Self>) {
        if components::uses_mobile_navigation(window) {
            self.chrome.hide(cx);
        }
        self.with_renderer(cx, |renderer, cx| renderer.previous_section(cx));
    }
}

impl CommandTarget<NextSection> for ReaderView {
    fn execute(&mut self, _: &NextSection, window: &mut Window, cx: &mut Context<Self>) {
        if components::uses_mobile_navigation(window) {
            self.chrome.hide(cx);
        }
        self.with_renderer(cx, |renderer, cx| renderer.next_section(cx));
    }
}

impl CommandTarget<NextSidebarTab> for ReaderView {
    fn execute(&mut self, _: &NextSidebarTab, window: &mut Window, cx: &mut Context<Self>) {
        let tab = state_panel::adjacent_sidebar_tab(self.sidebar_tab(), self.contents_available(), 1);
        self.chrome.show(cx);
        self.select_sidebar_tab(tab, window, cx);
    }
}

impl CommandTarget<PreviousSidebarTab> for ReaderView {
    fn execute(&mut self, _: &PreviousSidebarTab, window: &mut Window, cx: &mut Context<Self>) {
        let tab = state_panel::adjacent_sidebar_tab(self.sidebar_tab(), self.contents_available(), -1);
        self.chrome.show(cx);
        self.select_sidebar_tab(tab, window, cx);
    }
}

impl CommandTarget<ToggleSidebar> for ReaderView {
    fn execute(&mut self, _: &ToggleSidebar, window: &mut Window, cx: &mut Context<Self>) {
        let chrome_visible = self.chrome.is_visible() || (!components::uses_mobile_navigation(window) && self.search.query.is_some());
        let visible = chrome_visible && (!crate::shell::chrome::sidebar_overlays_document(window) || self.sidebar.is_visible());
        if visible {
            self.search_close(window, cx);
            self.sidebar.close();
            self.chrome.hide(cx);
            self.focus_document(window, cx);
        } else {
            self.sidebar.reopen();
            self.chrome.show(cx);
            self.sidebar_focus.focus(window, cx);
        }
        notify(cx, Component::ReaderShell, "sidebar_toggled");
    }
}

impl CommandTarget<ToggleToc> for ReaderView {
    fn execute(&mut self, _: &ToggleToc, _: &mut Window, cx: &mut Context<Self>) {
        self.chrome.show(cx);
        state_panel::select_tab(&mut self.sidebar, SidebarTab::Contents, cx);
        notify(cx, Component::ReaderShell, "toc_visibility_changed");
    }
}

impl CommandTarget<FocusReaderSearch> for ReaderView {
    fn execute(&mut self, _: &FocusReaderSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.chrome.show(cx);
        if components::uses_mobile_navigation(window) && self.sidebar.close() {
            notify(cx, Component::ReaderShell, "sidebar_closed_for_search");
        }
        if self.search.query.is_none() {
            self.open_search(cx);
        }
        self.search_input.read(cx).focus_handle(cx).focus(window, cx);
    }
}

impl CommandTarget<ToggleSearch> for ReaderView {
    fn execute(&mut self, _: &ToggleSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.chrome.show(cx);
        if self.search.query.is_some() {
            self.search_close(window, cx);
        } else {
            self.open_search(cx);
            if !components::uses_mobile_navigation(window) || !self.sidebar.is_visible() {
                self.search_input.read(cx).focus_handle(cx).focus(window, cx);
            }
        }
    }
}

impl CommandTarget<CloseSearch> for ReaderView {
    fn execute(&mut self, _: &CloseSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.search_close(window, cx);
    }
}

impl CommandTarget<IncreaseFont> for ReaderView {
    fn execute(&mut self, _: &IncreaseFont, _: &mut Window, cx: &mut Context<Self>) {
        self.adjust_font_size(1.0, cx);
    }
}

impl CommandTarget<DecreaseFont> for ReaderView {
    fn execute(&mut self, _: &DecreaseFont, _: &mut Window, cx: &mut Context<Self>) {
        self.adjust_font_size(-1.0, cx);
    }
}

// EPUB has no zoom control: font size covers reflowable text, and
// CommonReaderCommands still requires these two be implemented.
impl CommandTarget<IncreaseScale> for ReaderView {
    fn execute(&mut self, _: &IncreaseScale, _: &mut Window, _: &mut Context<Self>) {}
}

impl CommandTarget<DecreaseScale> for ReaderView {
    fn execute(&mut self, _: &DecreaseScale, _: &mut Window, _: &mut Context<Self>) {}
}

impl CommandTarget<IncreaseColumnWidth> for ReaderView {
    fn execute(&mut self, _: &IncreaseColumnWidth, window: &mut Window, cx: &mut Context<Self>) {
        self.adjust_column_width(40.0, window, cx);
    }
}

impl CommandTarget<DecreaseColumnWidth> for ReaderView {
    fn execute(&mut self, _: &DecreaseColumnWidth, window: &mut Window, cx: &mut Context<Self>) {
        self.adjust_column_width(-40.0, window, cx);
    }
}

impl CommandTarget<ToggleFullscreen> for ReaderView {
    fn execute(&mut self, _: &ToggleFullscreen, window: &mut Window, _: &mut Context<Self>) {
        window.toggle_fullscreen();
    }
}

/// Leaving the book is one behaviour with two entry points: this and the
/// toolbar's library button both go through `close_reader`.
impl CommandTarget<ReturnToLibrary> for ReaderView {
    fn execute(&mut self, _: &ReturnToLibrary, window: &mut Window, cx: &mut Context<Self>) {
        self.close_reader(window, cx);
    }
}
