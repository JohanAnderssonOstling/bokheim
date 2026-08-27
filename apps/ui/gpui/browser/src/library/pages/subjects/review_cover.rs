use gpui::prelude::*;
use gpui::{Context, Entity, IntoElement, ObjectFit, Render, Task, Window, img};
use gpui_component::ElementExt;
use ui_components as components;

use crate::library::services::LibraryContext;
use crate::library::widgets::{LoadedCover, loaded_cover_from_jpeg};

#[derive(Clone)]
enum ReviewCoverState {
    Pending,
    Loaded(LoadedCover),
    Missing,
}

pub(super) struct ReviewCoverView {
    content_hash: sync_common::ContentHash,
    ui: LibraryContext,
    state: ReviewCoverState,
    task: Option<Task<()>>,
}

impl ReviewCoverView {
    pub(super) fn create<P: 'static>(content_hash: sync_common::ContentHash, ui: LibraryContext, cx: &mut Context<P>) -> Entity<Self> {
        cx.new(|_| Self { content_hash, ui, state: ReviewCoverState::Pending, task: None })
    }

    fn prepare(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.state, ReviewCoverState::Pending) || self.task.is_some() {
            return;
        }
        let content_hash = self.content_hash;
        let load = crate::services::backend::library_blocking(self.ui.backend(), move |library| library.thumbnail(content_hash));
        let decoder = cx.background_executor().clone();
        let svg_renderer = cx.svg_renderer();
        self.task = Some(cx.spawn(async move |cover, cx| {
            let result = match load.await {
                Ok(Some(bytes)) => decoder.spawn(async move { loaded_cover_from_jpeg(bytes, svg_renderer) }).await,
                Ok(None) => None,
                Err(error) => {
                    log::error!("failed to load ISBN review thumbnail: {error}");
                    None
                }
            };
            let _ = cover.update(cx, |cover, cx| {
                cover.task.take();
                cover.state = result.map(ReviewCoverState::Loaded).unwrap_or(ReviewCoverState::Missing);
                cx.notify();
            });
        }));
    }
}

impl Render for ReviewCoverView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let frame = components::book_cover_frame().overflow_hidden().bg(theme.surface_alt);
        match &self.state {
            ReviewCoverState::Loaded(image) => frame.child(img(image.image.clone()).size_full().object_fit(ObjectFit::Contain)).into_any_element(),
            ReviewCoverState::Pending if self.task.is_none() => {
                let cover = cx.entity().downgrade();
                frame
                    .on_prepaint(move |_, window, cx| {
                        window.defer(cx, move |_, cx| {
                            let _ = cover.update(cx, |cover, cx| cover.prepare(cx));
                        });
                    })
                    .into_any_element()
            }
            ReviewCoverState::Pending => frame.into_any_element(),
            ReviewCoverState::Missing => components::missing_book_cover("No cover", theme).into_any_element(),
        }
    }
}
