use std::rc::Rc;

use gpui::{Context, IntoElement, Render, Window, div};

use super::services::LibraryContext;

pub(crate) struct BookDetailPage;

impl BookDetailPage {
    pub(crate) fn new(_ui: LibraryContext, _content_hash: epub_svelte_lib::ContentHash, _on_back: Rc<dyn Fn(&mut gpui::App)>, _cx: &mut Context<Self>) -> Self {
        Self
    }
}

impl Render for BookDetailPage {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}
