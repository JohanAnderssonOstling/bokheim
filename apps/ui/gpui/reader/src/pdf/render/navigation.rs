//! PDF page and outline navigation.

use gpui::prelude::*;
use gpui::{Context, Entity, ScrollStrategy, Window};
use gpui_component::tree::TreeItem;
use pdf_reader_core::PdfOutlineEntry;
use ui_components as components;

use super::PdfReaderView;
use crate::pdf::toc::{nest_pdf_outline, pdf_toc_tree_items};

impl PdfReaderView {
    pub(super) fn set_toc(&mut self, outline: &[PdfOutlineEntry], cx: &mut Context<Self>) {
        let nodes = nest_pdf_outline(outline);
        let mut targets = Vec::new();
        self.toc_tree.update(cx, |tree, cx| tree.set_items(pdf_toc_tree_items(&nodes, &mut targets), cx));
        self.toc.replace_targets(targets);
    }

    pub(super) fn update_active_toc(&mut self, page_index: usize, cx: &mut Context<Self>) {
        let next = self.toc.entry_at(page_index);
        if !self.toc.set_active(next.clone()) {
            return;
        }
        self.toc_tree.update(cx, |tree, cx| {
            if let Some(id) = next.as_ref() {
                let selected = TreeItem::new(id.clone(), id.clone());
                tree.set_selected_item(Some(&selected), cx);
                tree.reveal_item(&selected.id, ScrollStrategy::Center, cx);
            } else {
                tree.set_selected_item(None, cx);
            }
        });
    }

    fn navigate_to_toc(&self, id: &str, cx: &mut Context<Self>) {
        if let Some(page_index) = self.toc.page_of(id) {
            self.pdf.update(cx, |pdf, cx| pdf.set_page(page_index, cx));
        }
    }

    pub(super) fn focus_document(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.pdf.read(cx).focus_handle().focus(window, cx);
    }

    pub(super) fn active_outline_title(&self) -> Option<&str> {
        self.toc.active_title()
    }

    pub(super) fn render_toc(&self, entity: Entity<Self>, theme: components::BrowserTheme, cx: &mut Context<Self>) -> gpui::AnyElement {
        let toc = crate::shell::state_panel::reader_toc_tree(&self.toc_tree, &entity, theme, |id, window, this: &mut Self, cx| {
            this.navigate_to_toc(id, cx);
            this.focus_document(window, cx);
        });
        components::reader_toc(theme).child(self.render_sidebar_tabs(false, true, cx)).child(toc).into_any_element()
    }
}
