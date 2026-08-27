//! EPUB search visibility, debounce, and whole-publication streaming.

use std::time::Duration;

use gpui::{Context, Window};
use html_view_core::{RendererCommand, SearchScope};

use crate::epub::{ReaderView, SearchFilters};
use crate::invalidation::Component;
use crate::shell::search::SearchBarTarget;

const SEARCH_DEBOUNCE: Duration = Duration::from_millis(150);

impl ReaderView {
    pub(in crate::epub) fn open_search(&mut self, cx: &mut Context<Self>) {
        if self.search.query.is_some() {
            return;
        }
        self.search.query = Some(self.search_input.read(cx).value().to_string());
        self.with_renderer(cx, |renderer, cx| {
            renderer.apply(RendererCommand::ActivateSearch, cx);
        });
        crate::invalidation::notify(cx, Component::ReaderShell, "search_visibility_changed");
    }

    pub(super) fn queue_search(&mut self, query: String, cx: &mut Context<Self>) {
        let Some(generation) = self.search.set_query(query) else { return };
        let timer = cx.background_executor().timer(SEARCH_DEBOUNCE);
        self.search.debounce = Some(cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| {
                if this.search.is_current(generation) {
                    this.apply_pending_search(cx);
                }
            });
        }));
    }

    pub(super) fn apply_pending_search(&mut self, cx: &mut Context<Self>) -> bool {
        let Some((renderer_query, options)) = self.search.take_pending() else {
            return false;
        };
        if options.scope == SearchScope::CurrentDocument || renderer_query.is_empty() {
            self.with_renderer(cx, move |renderer, cx| renderer.apply(RendererCommand::SetSearch { query: renderer_query, options }, cx));
            return true;
        }
        let generation = self.search.generation;
        let provider = self.publication.provider.clone();
        let documents = self.publication.document_uris.clone();
        let cancellation = self.search.cancellation.clone();
        let executor = cx.background_executor().clone();
        self.with_renderer(cx, |renderer, cx| {
            renderer.apply(RendererCommand::SetSearchResults { query: renderer_query.clone(), options, results: Vec::new() }, cx);
        });
        self.search.index_task = Some(cx.spawn(async move |this, cx| {
            let query = renderer_query.clone();
            let (batch_tx, batch_rx) = async_channel::unbounded::<Vec<html_view_core::BookSearchMatch>>();
            let result = executor.spawn(async move {
                let results = html_view_core::search_publication_streaming(
                    provider.as_ref(),
                    &documents,
                    &query,
                    options,
                    || cancellation.load(std::sync::atomic::Ordering::Acquire) != generation,
                    |batch| {
                        let _ = batch_tx.try_send(batch.to_vec());
                    },
                );
                results.len()
            });
            while let Ok(batch) = batch_rx.recv().await {
                let _ = this.update(cx, |this, cx| {
                    if this.search.still_wants(generation, &renderer_query) {
                        this.with_renderer(cx, |renderer, cx| renderer.apply(RendererCommand::AppendSearchResults { query: renderer_query.clone(), options, results: batch }, cx));
                    }
                });
            }
            let _ = result.await;
        }));
        true
    }

    pub(in crate::epub) fn update_search_options(&mut self, update: impl FnOnce(&mut SearchFilters), cx: &mut Context<Self>) {
        self.search.update_filters(update);
        self.apply_pending_search(cx);
        crate::invalidation::notify(cx, Component::ReaderShell, "search_options_changed");
    }
}

impl SearchBarTarget for ReaderView {
    fn search_navigate(&mut self, direction: i8, cx: &mut Context<Self>) {
        self.with_renderer(cx, |renderer, cx| renderer.apply(RendererCommand::NavigateSearch(direction), cx));
    }

    fn search_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search.clear();
        self.search_input.update(cx, |input, cx| input.set_value("", window, cx));
        self.with_renderer(cx, |renderer, cx| {
            renderer.apply(RendererCommand::SetSearchQuery(String::new()), cx);
            renderer.apply(RendererCommand::DeactivateSearch, cx);
        });
        self.focus_document(window, cx);
        crate::invalidation::notify(cx, Component::ReaderShell, "search_closed");
    }
}
