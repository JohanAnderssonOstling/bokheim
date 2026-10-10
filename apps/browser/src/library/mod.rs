//! Library-scoped pages and services.
//!
//! Each library session owns its current page, while folder and subject pages
//! own their nested navigation.

mod pages;
mod services;
mod widgets;

pub(crate) use self::widgets::CursorMove;
pub(crate) use self::pages::browse::BrowseCrumb;
pub(crate) use self::pages::browse::{BrowseKind, BrowseSplitPlane};
pub(crate) use self::pages::trash::TrashPage;
pub(crate) use self::services::{BookDetailClosed, LibraryContext, LibraryUpdateBridge, OpenBook};
