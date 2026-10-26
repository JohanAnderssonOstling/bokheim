//! PDF page and outline navigation.

use crate::shell::toolbar::SidebarTarget;
use book_model::BookTocEntry;
use gpui::prelude::*;
use gpui::{Context, Entity, ScrollStrategy, Window};
use gpui_component::tree::TreeItem;
use std::rc::Rc;
use ui_components as components;

use super::PdfReaderView;
use crate::pdf::toc::pdf_toc_tree_items;
use crate::shell::state_panel::{self, TocActivation};

impl PdfReaderView {
    pub(super) fn load_database_toc(&self, cx: &mut Context<Self>) {
        let library = self.library.clone();
        let content_hash = self.locator.content_hash();
        cx.spawn(async move |this, cx| match library.book_detail(content_hash).await {
            Ok(detail) => {
                let _ = this.update(cx, |this, cx| {
                    this.toc.read_status = detail.chapter_read_status;
                    this.set_toc(detail.toc, cx);
                    let current_page = this.current_page;
                    this.update_active_toc(current_page, cx);
                });
            }
            Err(error) => log::warn!("could not load PDF table of contents from library database: {error}"),
        })
        .detach();
    }

    /// Reclaim document focus on hover without interrupting sidebar keyboard input.
    pub(super) fn reclaim_hover_focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        if self.sidebar_focus.contains_focused(window, cx) || self.rail_focus.contains_focused(window, cx) || self.marks.selected().is_some() {
            return;
        }
        window.activate_window();
        self.focus_document(window, cx);
    }

    pub(super) fn set_toc(&mut self, outline: Vec<BookTocEntry>, cx: &mut Context<Self>) {
        self.toc_tree.update(cx, |tree, cx| tree.set_items(pdf_toc_tree_items(&outline), cx));
        self.toc.replace_entries(outline);
    }

    pub(super) fn update_active_toc(&mut self, page_index: usize, cx: &mut Context<Self>) {
        self.toc.update_read_status(page_index, self.page_count > 0 && page_index + 1 == self.page_count);
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

    pub(super) fn render_toc_body(&self, entity: Entity<Self>, theme: components::BrowserTheme, cx: &gpui::App) -> gpui::AnyElement {
        let reset_entity = entity.clone();
        let reset_read: state_panel::TocReadReset = Rc::new(move |id, _, cx| {
            reset_entity.update(cx, |this, cx| {
                if let Some(target) = this.toc.reset_chapter_read_status(id) {
                    this.positions.reset_chapter_read_status(target);
                    cx.notify();
                }
            });
        });
        let toc = state_panel::reader_toc_tree(&self.toc_tree, &entity, self.toc.active_id(), self.toc.read_ids(), Some(reset_read), self.settings_navigation.keyboard_visible(), theme, cx, |id, activation, window, this: &mut Self, cx| {
            this.navigate_to_toc(id, cx);
            if activation == TocActivation::Pointer {
                this.focus_sidebar_body(window, cx);
            }
        });
        let reset = components::topbar_action_button("reset-chapter-read-status", "Reset read status", gpui_component::IconName::Undo2, theme).on_click(move |_, _, cx| {
            entity.update(cx, |this, cx| {
                this.positions.reset_read_status();
                this.toc.read_status.reset();
                cx.notify();
            });
        });
        components::reader_toc(theme).child(gpui::div().flex_none().p(gpui::px(components::SPACE_SM)).child(reset)).child(toc).into_any_element()
    }
}
