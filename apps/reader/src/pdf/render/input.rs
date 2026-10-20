//! PDF implementations of shared navigation commands.

use crate::shell::annotation_editor::AnnotationEditorTarget;

use crate::invalidation::notify;
use gpui::{Context, Window};

use crate::invalidation::Component;
use crate::shell::input::CommandTarget;
use crate::shell::state_panel::SidebarTab;
use crate::shell::toolbar::SidebarTarget;
use crate::{CloseSearch, DecreaseScale, FocusReaderSearch, IncreaseScale, NextLine, NextPage, NextSidebarTab, PreviousLine, PreviousPage, PreviousSidebarTab, ReturnToLibrary, ToggleSearch, ToggleSidebar, ToggleToc};

use super::{PdfReaderView, ZOOM_STEP};

impl CommandTarget<PreviousPage> for PdfReaderView {
    fn execute(&mut self, _: &PreviousPage, window: &mut Window, cx: &mut Context<Self>) {
        self.chrome.hide_for_motion(window, cx);
        if self.pdf.read(cx).can_previous_page() {
            self.pdf.update(cx, |pdf, cx| pdf.previous_page(cx));
        }
    }
}

impl CommandTarget<NextPage> for PdfReaderView {
    fn execute(&mut self, _: &NextPage, window: &mut Window, cx: &mut Context<Self>) {
        self.chrome.hide_for_motion(window, cx);
        if self.pdf.read(cx).can_next_page() {
            self.pdf.update(cx, |pdf, cx| pdf.next_page(cx));
        }
    }
}

impl CommandTarget<PreviousLine> for PdfReaderView {
    fn execute(&mut self, _: &PreviousLine, window: &mut Window, cx: &mut Context<Self>) {
        self.chrome.hide_for_motion(window, cx);
        self.pdf.update(cx, |pdf, cx| pdf.previous_item(cx));
    }
}

impl CommandTarget<NextLine> for PdfReaderView {
    fn execute(&mut self, _: &NextLine, window: &mut Window, cx: &mut Context<Self>) {
        self.chrome.hide_for_motion(window, cx);
        self.pdf.update(cx, |pdf, cx| pdf.next_item(cx));
    }
}

impl CommandTarget<NextSidebarTab> for PdfReaderView {
    fn execute(&mut self, _: &NextSidebarTab, window: &mut Window, cx: &mut Context<Self>) {
        let tab = self.chrome.cycled_sidebar_tab(self.sidebar.is_visible(), self.sidebar_tab(), self.contents_available(), 1, window);
        self.chrome.show(cx);
        self.keyboard_visibility().set(!ui_components::uses_mobile_reader_controls(window));
        self.select_sidebar_tab(tab, window, cx);
    }
}

impl CommandTarget<PreviousSidebarTab> for PdfReaderView {
    fn execute(&mut self, _: &PreviousSidebarTab, window: &mut Window, cx: &mut Context<Self>) {
        let tab = self.chrome.cycled_sidebar_tab(self.sidebar.is_visible(), self.sidebar_tab(), self.contents_available(), -1, window);
        self.chrome.show(cx);
        self.keyboard_visibility().set(!ui_components::uses_mobile_reader_controls(window));
        self.select_sidebar_tab(tab, window, cx);
    }
}

impl CommandTarget<ToggleSidebar> for PdfReaderView {
    fn execute(&mut self, _: &ToggleSidebar, window: &mut Window, cx: &mut Context<Self>) {
        let chrome_visible = self.chrome.is_visible();
        let visible = chrome_visible && (!crate::shell::chrome::sidebar_overlays_document(window) || self.sidebar.is_visible());
        if visible {
            self.sidebar.close(cx);
            self.chrome.hide(cx);
            self.focus_document(window, cx);
        } else {
            self.sidebar.reopen(cx);
            self.chrome.show(cx);
            self.focus_sidebar_body(window, cx);
        }
        notify(cx, Component::PdfShell, "sidebar_toggled");
    }
}

impl CommandTarget<ToggleToc> for PdfReaderView {
    fn execute(&mut self, _: &ToggleToc, window: &mut Window, cx: &mut Context<Self>) {
        self.select_sidebar_tab(SidebarTab::Contents, window, cx);
    }
}

impl CommandTarget<FocusReaderSearch> for PdfReaderView {
    fn execute(&mut self, _: &FocusReaderSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.select_sidebar_tab(SidebarTab::Search, window, cx);
    }
}

impl CommandTarget<ToggleSearch> for PdfReaderView {
    fn execute(&mut self, _: &ToggleSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.select_sidebar_tab(SidebarTab::Search, window, cx);
    }
}

impl CommandTarget<CloseSearch> for PdfReaderView {
    fn execute(&mut self, _: &CloseSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.hide_sidebar(window, cx);
    }
}

impl PdfReaderView {
    pub(crate) fn native_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.context_sheet.take().is_some() {
            cx.notify();
        } else if self.marks.selected().is_some() {
            self.dismiss_annotation_editor(window, cx);
        } else if self.sidebar.is_visible() && self.chrome.is_visible() {
            self.hide_sidebar(window, cx);
        } else {
            self.close_reader(window, cx);
        }
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
