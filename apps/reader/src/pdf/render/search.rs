//! PDF search visibility and renderer navigation.

use crate::shell::search;
use crate::shell::toolbar::SidebarTarget;
use gpui::prelude::*;
use gpui::{AnyElement, Context, Window};
use ui_components as components;

use super::PdfReaderView;
use crate::shell::search::SearchBarTarget;

impl SearchBarTarget for PdfReaderView {
    fn search_position(&self) -> (usize, usize) {
        self.search_match.unwrap_or((0, 0))
    }

    fn search_navigate(&mut self, direction: i8, cx: &mut Context<Self>) {
        self.pdf.update(cx, |pdf, cx| pdf.navigate_search(direction, cx));
    }
}

impl PdfReaderView {
    pub(super) fn render_search(&self, theme: components::BrowserTheme, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let mut rows = Vec::new();
        let mut page = None;
        for (index, hit) in self.pdf.read(cx).search_hits().iter().enumerate() {
            if page != Some(hit.page_index()) {
                page = Some(hit.page_index());
                rows.push(components::reader_annotation_group_heading(format!("Page {}", hit.page_index() + 1), theme).into_any_element());
            }
            let selected = self.search_match.is_some_and(|(current, _)| current == index + 1);
            rows.push(
                search::result_row(("pdf-search-hit", index), hit.excerpt().to_owned(), selected, theme)
                    .on_click(cx.listener(move |reader, _, window, cx| {
                        reader.pdf.update(cx, |pdf, cx| pdf.go_to_search_hit(index, cx));
                        if components::uses_mobile_reader_controls(window) {
                            reader.sidebar.close(cx);
                            reader.focus_document(window, cx);
                        } else {
                            reader.focus_sidebar_body(window, cx);
                        }
                    }))
                    .into_any_element(),
            );
        }
        search::panel(search::search_bar(&self.search_input, theme, window, cx), rows, theme)
    }
}
