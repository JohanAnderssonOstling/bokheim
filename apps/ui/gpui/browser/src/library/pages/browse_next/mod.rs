//! Adapter-driven browse implementation.

mod book_cover;
mod browse_page;
mod folder_picker;
mod library_adapter;
mod store_adapter;
mod store_cover;

pub(crate) use book_cover::{BookCoverState, book_cover, decode_book_cover_jpeg};
pub(crate) use browse_page::{BrowseAdapter, BrowseCrumb, BrowsePage, BrowseQuery, BrowseRequest};
pub(crate) use library_adapter::{folder, subject};
pub(crate) use store_adapter::store;
pub(crate) use store_cover::StoreBookCover;
