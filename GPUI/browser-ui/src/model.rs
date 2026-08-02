use std::sync::Arc;

use epub_svelte_lib::BookId;
use gpui::{Image, SharedString};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserSection {
    Home,
    Authors,
    #[default]
    Folders,
    Search,
    Sync,
    Settings,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BrowserSession {
    pub active_library_id: Option<String>,
    pub section: BrowserSection,
    #[serde(default)]
    pub folder_path: Vec<FolderCrumb>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FolderCrumb {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub book_count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BrowserWindowGeometry {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone)]
pub(crate) struct BookCard {
    pub(crate) id: BookId,
    pub(crate) title: SharedString,
    pub(crate) author: SharedString,
    pub(crate) cover: Option<Arc<Image>>,
    pub(crate) progress: Option<SharedString>,
}

#[derive(Clone)]
pub(crate) struct HomeSection {
    pub(crate) title: SharedString,
    pub(crate) books: Arc<Vec<BookCard>>,
}

#[derive(Clone)]
pub(crate) struct AuthorGroup {
    pub(crate) folder_path: SharedString,
    pub(crate) books: Arc<Vec<BookCard>>,
}
