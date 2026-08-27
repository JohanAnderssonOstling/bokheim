//! GPUI library-browsing window.
//!
//! [`root::BrowserRoot`] owns application routing, library selection, and the
//! current flat library or universal page. Folder/subject navigation and book
//! detail belong to their individual library pages.

mod application;
mod library;
mod model;
mod navigation;
mod root;
mod services;
mod stores;
mod universal;

pub(crate) const BOOK_IMPORT_EXTENSIONS: [&str; 6] = ["epub", "pdf", "mobi", "azw", "azw3", "m4b"];

use gpui::{App, KeyBinding, actions};

fn directory_import(selected: gpui::SelectedDirectory) -> bokheim_backend::DirectoryImport {
    bokheim_backend::DirectoryImport {
        name: selected.name,
        directories: selected.directories,
        files: selected.files.into_iter().map(|file| bokheim_backend::DirectoryImportFile { path: file.path.clone(), open: Box::new(move || Box::pin(async move { file.open().await.map_err(|error| error.to_string()) })) }).collect(),
    }
}

pub use application::{ApplicationRoot, OpenRequest, OpenedBook, ReturnToLibrary};
pub use navigation::Route;
pub use root::BrowserRoot;
#[cfg(feature = "kobo")]
pub use services::SaveKoboAutoRotate;
pub use services::{AppServices, DesktopUpdate, DesktopUpdater, OpenReader};

actions!(bokheim_browser, [DismissSettings, ShowSettings, DecreaseUiFontSize, IncreaseUiFontSize, FocusPreviousListItem, FocusNextListItem, ActivateAuthor]);

pub fn bind_keys(cx: &mut App) {
    // Number shortcuts follow the order drawn by the navigation rail.
    cx.bind_keys([
        KeyBinding::new("escape", DismissSettings, Some("Browser")),
        #[cfg(feature = "mobile")]
        KeyBinding::new("back", DismissSettings, Some("Browser")),
        KeyBinding::new("ctrl--", DecreaseUiFontSize, Some("Browser")),
        KeyBinding::new("ctrl-=", IncreaseUiFontSize, Some("Browser")),
        KeyBinding::new("ctrl-+", IncreaseUiFontSize, Some("Browser")),
        KeyBinding::new("up", FocusPreviousListItem, Some("BrowserArrowList")),
        KeyBinding::new("down", FocusNextListItem, Some("BrowserArrowList")),
        KeyBinding::new("enter", ActivateAuthor, Some("BrowserAuthorList")),
        KeyBinding::new("space", ActivateAuthor, Some("BrowserAuthorList")),
    ]);
}
