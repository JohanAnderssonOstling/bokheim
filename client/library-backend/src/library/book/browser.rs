//! Browser book capabilities and their library-owned leases.

pub(crate) use crate::LibraryId;
pub(crate) use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

pub(crate) static SOURCES: LazyLock<Mutex<HashMap<String, (LibraryId, client_platform_web::range_reader::ReaderLease)>>> = LazyLock::new(Default::default);

pub(crate) enum BookLease {
    Local(String),
    Audiobook(crate::library::browser_audiobook::PlaybackLease),
    None,
}

/// Leases stay with the library; browser readers stay in the platform adapter.
pub(crate) type BookResources = HashMap<uuid::Uuid, BookLease>;

impl Drop for BookLease {
    fn drop(&mut self) {
        if let Self::Local(id) = self {
            SOURCES.lock().expect("browser book sources").remove(id);
        }
    }
}
