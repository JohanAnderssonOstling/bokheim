//! Browser book capabilities and their library-owned leases.

pub(crate) use crate::{book::BookSource, LibraryId};
pub(crate) use std::cell::RefCell;
pub(crate) use std::collections::HashMap;

thread_local! {
    pub(crate) static SOURCES: RefCell<HashMap<String, (LibraryId, client_platform_web::range_reader::ReaderLease)>> = RefCell::new(HashMap::new());
}

pub(crate) enum BookLease {
    Local(String),
    Audiobook(crate::library::browser_audiobook::PlaybackLease),
    None,
}

/// Actor-local ownership of browser book resources. The actor owns the
/// leases, while the platform module owns the JavaScript-backed readers.
pub(crate) struct BookResources {
    pub(crate) leases: HashMap<u64, BookLease>,
    pub(crate) next: u64,
}
impl Default for BookResources {
    fn default() -> Self {
        Self { leases: HashMap::new(), next: 1 }
    }
}
impl BookResources {
    pub(crate) fn retain(&mut self, lease: BookLease) -> u64 {
        let id = self.next;
        self.next = self.next.wrapping_add(1);
        self.leases.insert(id, lease);
        id
    }
    pub(crate) fn release(&mut self, id: u64) {
        self.leases.remove(&id);
    }
}

impl Drop for BookLease {
    fn drop(&mut self) {
        if let Self::Local(id) = self {
            SOURCES.with(|sources| sources.borrow_mut().remove(id));
        }
    }
}
