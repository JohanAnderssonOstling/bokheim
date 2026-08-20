//! GPUI library-browsing window.
//!
//! [`root::BrowserRoot`] owns application routing, library selection, and the
//! current flat library or universal page. Folder/subject navigation and book
//! detail belong to their individual library pages.

mod catalog;
mod description;
mod library;
mod model;
mod navigation;
mod root;
mod services;
mod stores;
mod universal;
mod virtual_grid;

use gpui::{App, KeyBinding, actions};

pub use model::{BrowserSession, FolderCrumb, SubjectCrumb};
pub use root::BrowserRoot;
#[cfg(feature = "kobo")]
pub use services::SaveKoboAutoRotate;
pub use services::{AppServices, DesktopUpdate, DesktopUpdater, OpenReader};

/// The currently displayed browser page.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Route {
    Home,
    Authors,
    Folders,
    Subjects,
    Trash,
    Store,
    Account,
    Settings,
}

impl Default for Route {
    fn default() -> Self {
        Self::Folders
    }
}

actions!(bokheim_browser, [DismissSettings, FocusPreviousListItem, FocusNextListItem, ActivateAuthor]);

pub fn bind_keys(cx: &mut App) {
    // Number shortcuts follow the order drawn by the navigation rail.
    cx.bind_keys([
        KeyBinding::new("escape", DismissSettings, Some("Browser")),
        KeyBinding::new("up", FocusPreviousListItem, Some("BrowserArrowList")),
        KeyBinding::new("down", FocusNextListItem, Some("BrowserArrowList")),
        KeyBinding::new("enter", ActivateAuthor, Some("BrowserAuthorList")),
        KeyBinding::new("space", ActivateAuthor, Some("BrowserAuthorList")),
    ]);
}
