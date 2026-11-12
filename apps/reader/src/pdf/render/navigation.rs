//! PDF page and outline navigation.

use crate::shell::toolbar::SidebarTarget;
use book_model::BookTocEntry;
use gpui::prelude::*;
use gpui::{Context, Entity, ScrollStrategy, Window};
use gpui_component::tree::TreeItem;
use std::rc::Rc;
use std::time::Duration;
use ui_components as components;

use super::PdfReaderView;
use crate::pdf::toc::pdf_toc_tree_items;
use crate::shell::state_panel::{self, TocActivation};

impl PdfReaderView {
    pub(super) fn load_database_toc(&mut self, cx: &mut Context<Self>) {
        let library = self.library.clone();
        let book_id = self.locator.book_id();
        // Owned by the reader so closing it cancels requests and delayed retries.
        self._toc_load = Some(cx.spawn(async move |this, cx| {
            let mut delay = Duration::from_secs(1);
            loop {
                match library.book_detail(book_id).await {
                    Ok(detail) => {
                        let _ = this.update(cx, |this, cx| {
                            this.toc.read_status = detail.chapter_read_status;
                            this.set_toc(detail.toc, cx);
                            let current_page = this.current_page;
                            this.update_active_toc(current_page, cx);
                            cx.notify();
                        });
                        break;
                    }
                    Err(error) => log::warn!("could not load PDF table of contents from library database; retrying in {delay:?}: {error}"),
                }
                cx.background_executor().timer(delay).await;
                delay = (delay * 2).min(Duration::from_secs(30));
            }
        }));
    }

    /// Reclaim document focus inside the active window without raising it.
    pub(super) fn reclaim_hover_focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        if !window.is_window_active() || self.dictionary.is_some() || self.sidebar_focus.contains_focused(window, cx) || self.rail_focus.contains_focused(window, cx) || self.marks.selected().is_some() {
            return;
        }
        self.focus_document(window, cx);
    }

    pub(super) fn set_toc(&mut self, outline: Vec<BookTocEntry>, cx: &mut Context<Self>) {
        self.toc_filter.update(cx, |filter, cx| filter.set_items(pdf_toc_tree_items(&outline), cx));
        self.toc.replace_entries(outline);
        self.search_list.rebuild_contents(&self.toc);
        self.refresh_bookmark_rows(cx);
        self.refresh_annotation_overlays(cx);
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
        let toc = state_panel::reader_toc_tree(
            &self.toc_tree,
            Some(self.toc_filter.read(cx).index()),
            &entity,
            |this, id| state_panel::TocRowStatus { active: this.toc.active_id() == Some(id), read: this.toc.is_read(id) },
            Some(reset_read),
            self.settings_navigation.keyboard_visible(),
            theme,
            cx,
            |id, activation, window, this: &mut Self, cx| {
                this.navigate_to_toc(id, cx);
                if components::uses_mobile_reader_controls(window) {
                    this.hide_sidebar(window, cx);
                } else if activation == TocActivation::Pointer {
                    this.focus_sidebar_body(window, cx);
                }
            },
        );
        components::reader_toc(theme).child(self.toc_filter.clone()).child(toc).into_any_element()
    }
}
