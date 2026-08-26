//! Library-scoped pages and services.
//!
//! [`BrowserRoot`](crate::BrowserRoot) owns the selected library and current
//! flat page, while folder and subject pages own their own nested navigation.

mod pages;
mod services;
mod widgets;

pub(crate) use self::pages::authors::AuthorsPage;
pub(crate) use self::pages::browse::{folder as folder_browse_page, store as store_browse_page, subject as subject_browse_page};
pub(crate) use self::pages::home::HomePage;
pub(crate) use self::pages::trash::TrashPage;
pub(crate) use self::services::LibraryContext;
pub(crate) use self::widgets::{LoadedCover, loaded_cover_from_jpeg};
