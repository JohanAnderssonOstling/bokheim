//! GPUI library-browsing window.
//!
//! Dependencies run top-down. A parent constructs each child with the data and
//! services it needs and pushes updates into it; a child never holds a handle
//! to its parent, and instead emits a typed event that its immediate parent
//! subscribes to. State read by two siblings lives in a store injected into
//! both rather than being reached through a shared parent.
//!
//! Ownership splits at the library boundary:
//!
//! - [`root::BrowserRoot`] owns universal routing and the Store, Sync and
//!   Settings pages.
//! - [`library::LibraryHost`] owns library selection and replaces its
//!   [`library::LibraryRoot`] whenever the active library changes.

mod catalog;
mod description;
mod library;
mod model;
mod navigation;
mod root;
mod route;
mod services;
mod stores;
mod universal;
mod virtual_grid;

use gpui::{App, KeyBinding, actions};

pub use model::{BrowserSession, BrowserWindowGeometry, FolderCrumb, SubjectCrumb};
pub use root::BrowserRoot;
pub use route::{LibraryRoute, Route, UniversalRoute};
#[cfg(feature = "kobo")]
pub use services::SaveKoboAutoRotate;
pub use services::{AppServices, DesktopUpdate, DesktopUpdater, OpenReader, SaveBrowserState};

actions!(bokheim_browser, [FocusSearch, ShowHome, ShowAuthors, ShowFolders, ShowSubjects, ShowStore, ShowSync, ShowSettings, DismissSettings, FocusPreviousListItem, FocusNextListItem, ActivateAuthor]);

pub fn bind_keys(cx: &mut App) {
    // ctrl-1..6 follow `NAVIGATION_FEATURES` so the numbers match the order the
    // rail draws. Account and Settings sit outside that contract and take the
    // two numbers after it.
    cx.bind_keys([
        KeyBinding::new("ctrl-f", FocusSearch, Some("Browser")),
        KeyBinding::new("ctrl-1", ShowHome, Some("Browser")),
        KeyBinding::new("ctrl-2", ShowFolders, Some("Browser")),
        KeyBinding::new("ctrl-3", ShowSubjects, Some("Browser")),
        KeyBinding::new("ctrl-4", ShowAuthors, Some("Browser")),
        KeyBinding::new("ctrl-5", ShowStore, Some("Browser")),
        KeyBinding::new("ctrl-6", ShowSync, Some("Browser")),
        KeyBinding::new("ctrl-7", ShowSettings, Some("Browser")),
        KeyBinding::new("escape", DismissSettings, Some("Browser")),
        KeyBinding::new("up", FocusPreviousListItem, Some("BrowserArrowList")),
        KeyBinding::new("down", FocusNextListItem, Some("BrowserArrowList")),
        KeyBinding::new("enter", ActivateAuthor, Some("BrowserAuthorList")),
        KeyBinding::new("space", ActivateAuthor, Some("BrowserAuthorList")),
    ]);
}
