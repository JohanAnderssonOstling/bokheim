use gpui::{Context, IntoElement, Render, Window, div};

use crate::library::services::LibraryContext;

pub(crate) struct AuthorsPage;

impl AuthorsPage {
    pub(crate) fn new(_ui: LibraryContext, _window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self
    }
}

impl Render for AuthorsPage {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}
