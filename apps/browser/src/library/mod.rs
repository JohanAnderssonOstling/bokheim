//! Library-scoped pages and services.
//!
//! Each library session owns its current page, while folder and subject pages
//! own their nested navigation.

mod pages;
mod services;
mod widgets;

pub(crate) use self::pages::browse::BrowseCrumb;
pub(crate) use self::pages::browse::{BrowseKind, BrowsePageHost};
pub(crate) use self::pages::trash::TrashPage;
pub(crate) use self::services::{LibraryContext, LibraryUpdateBridge, OpenBook, UndoEntry, UndoStack};
pub(crate) use self::widgets::CursorMove;
pub(crate) use self::widgets::evict_covers;

#[cfg(test)]
pub(crate) use self::services::UndoStep;
