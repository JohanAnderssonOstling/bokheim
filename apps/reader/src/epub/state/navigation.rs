//! EPUB table-of-contents, focus, and document navigation.

use book_content::BookToc;
use std::sync::Arc;
use book_model::BookTocEntry;
use gpui::{App, Context, FocusHandle, Window};
use gpui_component::tree::TreeItem;
use html_view_gpui::HtmlView;
use skrift_column_paginator::RendererCommand;

use crate::epub::{LoadState, ReaderView};
use crate::invalidation::Component;

impl ReaderView {
    pub(super) fn load_database_toc(&self, cx: &mut Context<Self>) {
        let library = self.library.clone();
        let book = self.book.clone();
        let book_id = self.locator.book_id();
        cx.spawn(async move |this, cx| {
            let (entries, read_status) = if book.is_transient() {
                (book.toc().to_vec(), Default::default())
            } else {
                match library.book_detail(book_id).await {
                    Ok(detail) => (if detail.toc.is_empty() { book.toc().to_vec() } else { detail.toc }, detail.chapter_read_status),
                    Err(error) => {
                        log::warn!("could not load EPUB table of contents from library database: {error}");
                        (book.toc().to_vec(), Default::default())
                    }
                }
            };
            let source = book.clone();
            let (entries, resolutions, positions) = cx
                .background_executor()
                .spawn(async move {
                    let resolutions = BookToc::resolve(&entries, |href| BookToc::resolve_publication_href(href, book.document_uris(), |path| book.provider.resolve("", path)));
                    let positions = crate::epub::toc::TocState::source_positions(&resolutions, book.document_uris(), book.provider.as_ref());
                    (entries, resolutions, positions)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if !matches!(&this.load_state, LoadState::Ready(_)) || !Arc::ptr_eq(&this.book, &source) {
                    return;
                }
                this.toc.read_status = read_status;
                this.set_toc(entries, resolutions, positions, cx);
            });
        })
        .detach();
    }

    pub(super) fn set_toc(&mut self, entries: Vec<BookTocEntry>, resolutions: Vec<crate::epub::toc::ResolvedTocEntry>, positions: crate::epub::toc::SourcePositions, cx: &mut Context<Self>) {
        self.toc.set(entries, resolutions);
        let tree_items = self.toc.tree_items();
        self.toc_filter.update(cx, |filter, cx| filter.set_items(tree_items, cx));
        self.toc.install_source_positions(positions, self.book.document_uris().len());
        self.refresh_bookmark_rows(cx);
        self.toc.update_read_status(self.progress.fraction >= 1.0);
        self.search.rebuild_rows(&self.toc);
        self.refresh_recurring_annotations(true, cx);
        self.sync_toc_position(cx);
        self.record_current_reading_position(cx);
        crate::invalidation::notify(cx, Component::ReaderShell, "toc_loaded");
    }

    pub(super) fn sync_toc_position(&self, cx: &mut Context<Self>) {
        let next = self.toc.active_id().map(str::to_owned);
        self.toc_tree.update(cx, |tree, cx| {
            let selected = next.map(|id| TreeItem::new(id.clone(), id));
            tree.set_selected_item(selected.as_ref(), cx);
            if let Some(selected) = selected {
                tree.reveal_item(&selected.id, gpui::ScrollStrategy::Center, cx);
            }
        });
    }

    /// The document's focus handle, once the book has loaded.
    pub(in crate::epub) fn document_focus(&self, cx: &App) -> Option<FocusHandle> {
        match &self.load_state {
            LoadState::Ready(renderer) => Some(renderer.read(cx).focus_handle()),
            LoadState::Loading | LoadState::Error(_) => None,
        }
    }

    /// Returns keyboard control to the document after a chrome interaction.
    pub(in crate::epub) fn focus_document(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(focus) = self.document_focus(cx) {
            focus.focus(window, cx);
        }
    }

    pub(in crate::epub) fn with_renderer(&self, cx: &mut Context<Self>, update: impl FnOnce(&mut HtmlView, &mut Context<HtmlView>)) {
        if let LoadState::Ready(renderer) = &self.load_state {
            renderer.update(cx, update);
        }
    }

    pub(in crate::epub) fn copy_preview_image(&self, cx: &mut Context<Self>) {
        if let Some((_, image)) = &self.image_preview {
            cx.write_to_clipboard(gpui::ClipboardItem::new_image(image));
        }
    }

    pub(crate) fn navigate_to(&mut self, href: String, cx: &mut Context<Self>) {
        self.page_turn = None;
        self.swipe_preview.clear();
        self.with_renderer(cx, |renderer, cx| renderer.apply(RendererCommand::NavigateToHref(href), cx));
        #[cfg(all(any(feature = "native-tts", target_arch = "wasm32"), not(feature = "kobo")))]
        self.tts_navigation_changed(cx);
    }

    pub(crate) fn restore_document_position(&mut self, cfi: String, window: &mut Window, cx: &mut Context<Self>) {
        self.dismiss_selection(cx);
        self.clear_navigation_overlays();
        self.close_annotation_popup(cx);
        self.with_renderer(cx, |renderer, cx| renderer.apply(RendererCommand::SetCfiPosition(Some(cfi)), cx));
        self.focus_document(window, cx);
    }

    pub(in crate::epub) fn navigate_from_footnote(&mut self, cx: &mut Context<Self>) {
        let Some((preview, _)) = self.footnote.take() else { return };
        self.navigate_to(preview.href, cx);
        crate::invalidation::notify(cx, Component::ReaderShell, "footnote_navigated");
    }

    pub(in crate::epub) fn spine_title(&self) -> String {
        self.toc.active_title().or_else(|| (self.title != self.book_title).then(|| self.title.clone())).unwrap_or_default()
    }
}
