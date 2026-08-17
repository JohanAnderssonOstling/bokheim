//! Typed drag payloads shared by folder cells and book cards.

use epub_svelte_lib::{ContentHash, DirId};
use gpui::prelude::*;
use gpui::{Context, IntoElement, Render, Window, div, px};
use gpui_component::{Icon, IconName, Sizable};
use ui_components as components;

#[derive(Clone)]
pub(crate) enum LibraryDrag {
    Book { content_hash: ContentHash, source_id: DirId, title: String },
    Folder { folder_id: DirId, source_parent_id: DirId, source_path: String, name: String },
}

impl LibraryDrag {
    pub(crate) fn label(&self) -> &str {
        match self {
            Self::Book { title, .. } => title,
            Self::Folder { name, .. } => name,
        }
    }

    fn icon(&self) -> IconName {
        match self {
            Self::Book { .. } => IconName::BookOpen,
            Self::Folder { .. } => IconName::Folder,
        }
    }
}

pub(crate) struct LibraryDragPreview(LibraryDrag);

impl LibraryDragPreview {
    pub(crate) fn new(item: LibraryDrag) -> Self {
        Self(item)
    }
}

impl Render for LibraryDragPreview {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        div()
            .max_w(px(280.0))
            .px(px(components::SPACE_SM))
            .py(px(components::SPACE_XS))
            .flex()
            .items_center()
            .gap(px(components::SPACE_XS))
            .rounded(px(components::RADIUS_SM))
            .border_1()
            .border_color(theme.border)
            .bg(theme.surface)
            .shadow(components::overlay_shadow(theme))
            .child(Icon::new(self.0.icon()).small().text_color(theme.accent))
            .child(div().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(self.0.label().to_owned()))
    }
}
