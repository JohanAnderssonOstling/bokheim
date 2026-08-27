//! EPUB implementations of shared navigation commands.

use gpui::{Context, Focusable, Window};

use crate::epub::ReaderView;
use crate::invalidation::Component;
use crate::shell::input::CommandTarget;
use crate::shell::search::SearchBarTarget;
use crate::shell::state_panel::{self, SidebarPanel};
use crate::{
    CloseSearch, DecreaseColumnWidth, DecreaseFont, DecreaseScale, FocusReaderSearch, IncreaseColumnWidth, IncreaseFont, IncreaseScale, NextLine, NextPage, NextSection, PreviousLine, PreviousPage, PreviousSection, ToggleFullscreen,
    ToggleSearch, ToggleToc,
};

impl CommandTarget<PreviousPage> for ReaderView {
    fn execute(&mut self, _: &PreviousPage, _: &mut Window, cx: &mut Context<Self>) {
        self.chrome.hide(cx);
        // Renderer position is authoritative. Progress is an asynchronous
        // projection and may still contain its initial value on the first key.
        self.with_renderer(cx, |renderer, cx| {
            renderer.previous_page(cx);
        });
    }
}

impl CommandTarget<NextPage> for ReaderView {
    fn execute(&mut self, _: &NextPage, _: &mut Window, cx: &mut Context<Self>) {
        self.chrome.hide(cx);
        self.with_renderer(cx, |renderer, cx| {
            renderer.next_page(cx);
        });
    }
}

impl CommandTarget<PreviousLine> for ReaderView {
    fn execute(&mut self, _: &PreviousLine, _: &mut Window, cx: &mut Context<Self>) {
        self.chrome.hide(cx);
        self.with_renderer(cx, |renderer, cx| renderer.previous_line(cx));
    }
}

impl CommandTarget<NextLine> for ReaderView {
    fn execute(&mut self, _: &NextLine, _: &mut Window, cx: &mut Context<Self>) {
        self.chrome.hide(cx);
        self.with_renderer(cx, |renderer, cx| renderer.next_line(cx));
    }
}

impl CommandTarget<PreviousSection> for ReaderView {
    fn execute(&mut self, _: &PreviousSection, _: &mut Window, cx: &mut Context<Self>) {
        self.chrome.hide(cx);
        self.with_renderer(cx, |renderer, cx| renderer.previous_section(cx));
    }
}

impl CommandTarget<NextSection> for ReaderView {
    fn execute(&mut self, _: &NextSection, _: &mut Window, cx: &mut Context<Self>) {
        self.chrome.hide(cx);
        self.with_renderer(cx, |renderer, cx| renderer.next_section(cx));
    }
}

impl CommandTarget<ToggleToc> for ReaderView {
    fn execute(&mut self, _: &ToggleToc, _: &mut Window, cx: &mut Context<Self>) {
        self.chrome.show(cx);
        let visible = !self.sidebar.shows(SidebarPanel::Contents);
        state_panel::set_contents_visible(&mut self.sidebar, visible, cx);
        crate::invalidation::notify(cx, Component::ReaderShell, "toc_visibility_changed");
    }
}

impl CommandTarget<FocusReaderSearch> for ReaderView {
    fn execute(&mut self, _: &FocusReaderSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.chrome.show(cx);
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
            self.search_input.read(cx).focus_handle(cx).focus(window, cx);
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

impl CommandTarget<IncreaseScale> for ReaderView {
    fn execute(&mut self, _: &IncreaseScale, _: &mut Window, cx: &mut Context<Self>) {
        self.adjust_scale(0.1, cx);
    }
}

impl CommandTarget<DecreaseScale> for ReaderView {
    fn execute(&mut self, _: &DecreaseScale, _: &mut Window, cx: &mut Context<Self>) {
        self.adjust_scale(-0.1, cx);
    }
}

impl CommandTarget<IncreaseColumnWidth> for ReaderView {
    fn execute(&mut self, _: &IncreaseColumnWidth, _: &mut Window, cx: &mut Context<Self>) {
        self.adjust_column_width(40.0, cx);
    }
}

impl CommandTarget<DecreaseColumnWidth> for ReaderView {
    fn execute(&mut self, _: &DecreaseColumnWidth, _: &mut Window, cx: &mut Context<Self>) {
        self.adjust_column_width(-40.0, cx);
    }
}

impl CommandTarget<ToggleFullscreen> for ReaderView {
    fn execute(&mut self, _: &ToggleFullscreen, window: &mut Window, _: &mut Context<Self>) {
        window.toggle_fullscreen();
    }
}
