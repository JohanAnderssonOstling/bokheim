//! PDF implementations of shared navigation commands.

use gpui::{Context, Window};

use crate::invalidation::Component;
use crate::shell::input::CommandTarget;
use crate::shell::search::SearchBarTarget;
use crate::shell::state_panel::{self, SidebarPanel};
use crate::shell::toolbar::ToolbarTarget;
use crate::{CloseSearch, DecreaseScale, FocusReaderSearch, IncreaseScale, NextLine, NextPage, PreviousLine, PreviousPage, ReturnToLibrary, ToggleSearch, ToggleToc};

use super::{PdfReaderView, ZOOM_STEP};

impl CommandTarget<PreviousPage> for PdfReaderView {
    fn execute(&mut self, _: &PreviousPage, _: &mut Window, cx: &mut Context<Self>) {
        self.chrome.hide(cx);
        if self.pdf.read(cx).can_previous_page() {
            self.pdf.update(cx, |pdf, cx| pdf.previous_page(cx));
        }
    }
}

impl CommandTarget<NextPage> for PdfReaderView {
    fn execute(&mut self, _: &NextPage, _: &mut Window, cx: &mut Context<Self>) {
        self.chrome.hide(cx);
        if self.pdf.read(cx).can_next_page() {
            self.pdf.update(cx, |pdf, cx| pdf.next_page(cx));
        }
    }
}

impl CommandTarget<PreviousLine> for PdfReaderView {
    fn execute(&mut self, _: &PreviousLine, _: &mut Window, cx: &mut Context<Self>) {
        self.chrome.hide(cx);
        self.pdf.update(cx, |pdf, cx| pdf.previous_item(cx));
    }
}

impl CommandTarget<NextLine> for PdfReaderView {
    fn execute(&mut self, _: &NextLine, _: &mut Window, cx: &mut Context<Self>) {
        self.chrome.hide(cx);
        self.pdf.update(cx, |pdf, cx| pdf.next_item(cx));
    }
}

impl CommandTarget<ToggleToc> for PdfReaderView {
    fn execute(&mut self, _: &ToggleToc, _: &mut Window, cx: &mut Context<Self>) {
        self.chrome.show(cx);
        let visible = !self.sidebar.shows(SidebarPanel::Contents);
        state_panel::set_contents_visible(&mut self.sidebar, visible, cx);
        crate::invalidation::notify(cx, Component::PdfShell, "toc_visibility_changed");
    }
}

impl CommandTarget<FocusReaderSearch> for PdfReaderView {
    fn execute(&mut self, _: &FocusReaderSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.chrome.show(cx);
        self.open_search(window, cx);
    }
}

impl CommandTarget<ToggleSearch> for PdfReaderView {
    fn execute(&mut self, _: &ToggleSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.chrome.show(cx);
        if self.search_match.is_some() {
            self.search_close(window, cx);
        } else {
            self.open_search(window, cx);
        }
    }
}

impl CommandTarget<CloseSearch> for PdfReaderView {
    fn execute(&mut self, _: &CloseSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.search_close(window, cx);
    }
}

impl CommandTarget<IncreaseScale> for PdfReaderView {
    fn execute(&mut self, _: &IncreaseScale, _: &mut Window, cx: &mut Context<Self>) {
        self.adjust_zoom(ZOOM_STEP, cx);
    }
}

impl CommandTarget<DecreaseScale> for PdfReaderView {
    fn execute(&mut self, _: &DecreaseScale, _: &mut Window, cx: &mut Context<Self>) {
        self.adjust_zoom(-ZOOM_STEP, cx);
    }
}

/// Leaving the book is one behaviour with two entry points: this and the
/// toolbar's library button both go through `close_reader`.
impl CommandTarget<ReturnToLibrary> for PdfReaderView {
    fn execute(&mut self, _: &ReturnToLibrary, window: &mut Window, cx: &mut Context<Self>) {
        self.close_reader(window, cx);
    }
}
