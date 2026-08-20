//! Store-specific loading around the neutral book-cover presentation.

use std::time::Duration;

use epub_svelte_lib::catalog::CatalogSourceId;
use gpui::prelude::*;
use gpui::{Context, Entity, IntoElement, Render, SharedString, Task, Window};

use super::book_cover::{BookCoverState, book_cover, decode_book_cover_jpeg};
use crate::services::AppServices;

const MAX_RETRIES: u8 = 3;

pub(crate) struct StoreBookCover {
    services: AppServices,
    source_id: CatalogSourceId,
    image_url: String,
    missing_label: SharedString,
    state: BookCoverState,
    task: Option<Task<()>>,
    retries: u8,
}

impl StoreBookCover {
    pub(crate) fn create<P: 'static>(services: AppServices, source_id: CatalogSourceId, image_url: String, missing_label: impl Into<SharedString>, cx: &mut Context<P>) -> Entity<Self> {
        cx.new(|_| Self { services, source_id, image_url, missing_label: missing_label.into(), state: BookCoverState::Loading, task: None, retries: 0 })
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let source_id = self.source_id.clone();
        let image_url = self.image_url.clone();
        let operation = self.services.backend.spawn(move |backend| async move { backend.catalog_cover(&source_id, &image_url).await.map_err(|error| error.to_string()) });
        self.task = Some(cx.spawn(async move |cover, cx| {
            let result = operation.await.unwrap_or_else(|error| Err(error.to_string()));
            let _ = cover.update(cx, |cover, cx| {
                cover.task.take();
                match result {
                    Ok(bytes) => {
                        cover.state = decode_book_cover_jpeg(bytes).map(BookCoverState::Loaded).unwrap_or_else(|| BookCoverState::Missing(cover.missing_label.clone()));
                    }
                    Err(error) if cover.retries < MAX_RETRIES => {
                        log::warn!("failed to load catalogue cover: {error}");
                        let delay = Duration::from_secs(2_u64.pow(u32::from(cover.retries)).min(30));
                        cover.retries += 1;
                        let timer = cx.background_executor().timer(delay);
                        cover.task = Some(cx.spawn(async move |cover, cx| {
                            timer.await;
                            let _ = cover.update(cx, |cover, cx| {
                                cover.task.take();
                                cx.notify();
                            });
                        }));
                    }
                    Err(error) => {
                        log::warn!("failed to load catalogue cover after retries: {error}");
                        cover.state = BookCoverState::Missing(cover.missing_label.clone());
                    }
                }
                cx.notify();
            });
        }));
    }
}

impl Render for StoreBookCover {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if matches!(self.state, BookCoverState::Loading) && self.task.is_none() {
            self.load(cx);
        }
        book_cover(self.state.clone(), ui_components::browser_theme(cx))
    }
}
