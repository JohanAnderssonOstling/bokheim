//! PDF reader action dispatch and chrome/sidebar transitions.

use crate::invalidation::notify;
use std::{cell::Cell, rc::Rc};

use gpui::{Context, Window};

use crate::invalidation::Component;
use crate::shell::annotation_editor::AnnotationEditorTarget;
use crate::shell::toolbar::{SidebarTarget, document_sidebar_methods};
use crate::shell::{chrome::ChromeState, input::ReaderSidebarCommands, state_panel::SidebarState};

use super::PdfReaderView;

impl ReaderSidebarCommands for PdfReaderView {
    fn sidebar_controls(&mut self) -> (&mut SidebarState, &mut ChromeState) {
        (&mut self.sidebar, &mut self.chrome)
    }
}

impl PdfReaderView {
    pub(super) fn open_search(&mut self, cx: &mut Context<Self>) {
        if self.search_match.is_none() {
            self.search_match = Some((0, 0));
            notify(cx, Component::PdfShell, "search_opened");
        }
    }
}

impl SidebarTarget for PdfReaderView {
    document_sidebar_methods!();
    fn history_availability(&self) -> (bool, bool) {
        self.history.availability()
    }

    fn navigate_history(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let current = self.pdf.read(cx).source_position();
        if let Some(target) = self.history.navigate(current, forward) {
            self.context_sheet = None;
            self.dismiss_annotation_editor(window, cx);
            self.pdf.update(cx, |pdf, cx| pdf.restore_position(target, cx));
            self.focus_document(window, cx);
        }
        cx.notify();
    }

    #[cfg(feature = "kobo")]
    fn device_controls(&self) -> Option<gpui::AnyView> {
        self.device_controls.clone()
    }

    fn close_reader(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        #[cfg(all(any(feature = "native-tts", target_arch = "wasm32"), not(feature = "kobo")))]
        self.tts.close();
        (self.close_reader)(window, cx);
    }

    fn hide_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.hide_sidebar_controls(window, cx);
    }

    fn sidebar_focus(&self) -> gpui::FocusHandle {
        self.sidebar_focus.clone()
    }

    fn rail_focus(&self) -> gpui::FocusHandle {
        self.rail_focus.clone()
    }

    fn keyboard_visibility(&self) -> Rc<Cell<bool>> {
        self.settings_navigation.keyboard_visibility()
    }

    fn contents_available(&self) -> bool {
        self.toc.has_entries()
    }

    fn settings_key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.handle_settings_key(key, window, cx)
    }

    #[cfg(all(any(feature = "native-tts", target_arch = "wasm32"), not(feature = "kobo")))]
    fn tts_mark(&self) -> Option<ui_components::ReaderRailMark> {
        self.tts.available().then(|| self.tts.button_mark())
    }

    #[cfg(all(any(feature = "native-tts", target_arch = "wasm32"), not(feature = "kobo")))]
    fn press_tts(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        crate::shell::tts::TtsReader::press_tts(self, cx);
    }
}
