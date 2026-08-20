use gpui::prelude::*;
use gpui::{Context, Entity, IntoElement, ObjectFit, Render, Task, Window, img};
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
    content_hash: epub_svelte_lib::ContentHash,
    ui: LibraryContext,
    state: ReviewCoverState,
    task: Option<Task<()>>,
}

impl ReviewCoverView {
    pub(super) fn create<P: 'static>(content_hash: epub_svelte_lib::ContentHash, ui: LibraryContext, cx: &mut Context<P>) -> Entity<Self> {
        cx.new(|_| Self { content_hash, ui, state: ReviewCoverState::Pending, task: None })
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let load = self.ui.load_cover(self.content_hash.clone());
        self.task = Some(cx.spawn(async move |cover, cx| {
            let result = load.await;
            let _ = cover.update(cx, |cover, cx| {
                cover.task.take();
                cover.state = match result {
                    Ok(Some(bytes)) => loaded_cover_from_jpeg(bytes).map(ReviewCoverState::Loaded).unwrap_or(ReviewCoverState::Missing),
                    Ok(_) => ReviewCoverState::Missing,
                    Err(error) => {
                        log::error!("failed to load ISBN review thumbnail: {error}");
                        ReviewCoverState::Missing
                    }
                };
                cx.notify();
            });
        }));
    }
}

impl Render for ReviewCoverView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if matches!(self.state, ReviewCoverState::Pending) && self.task.is_none() {
            self.load(cx);
        }
        let theme = components::browser_theme(cx);
        let frame = components::book_cover_frame().overflow_hidden().bg(theme.surface_alt);
        match &self.state {
            ReviewCoverState::Loaded(image) => frame.child(img(image.image.clone()).size_full().object_fit(ObjectFit::Contain)).into_any_element(),
            ReviewCoverState::Pending => frame.into_any_element(),
            ReviewCoverState::Missing => components::missing_book_cover("No cover", theme).into_any_element(),
        }
    }
}
