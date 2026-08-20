//! Passive configured-library selection shared with navigation and Sync.
//!
//! `BrowserRoot` owns selection and fallback policy; this entity is only the
//! observable snapshot shared with navigation and Sync.

use epub_svelte_lib::LibraryId;
use epub_svelte_lib::app::LibraryEntry;
use gpui::{Context, EventEmitter};

pub(crate) struct LibraryStateChanged;

pub(crate) struct LibraryState {
    entries: Vec<LibraryEntry>,
    library_id: Option<LibraryId>,
}

impl EventEmitter<LibraryStateChanged> for LibraryState {}

impl LibraryState {
    pub(crate) fn new(entries: Vec<LibraryEntry>, library_id: Option<LibraryId>) -> Self {
        Self { entries, library_id }
    }

    pub(crate) fn entries(&self) -> &[LibraryEntry] {
        &self.entries
    }

    pub(crate) fn library_id(&self) -> Option<&LibraryId> {
        self.library_id.as_ref()
    }

    pub(crate) fn is_configured(&self) -> bool {
        !self.entries.is_empty()
    }

    pub(crate) fn replace(&mut self, entries: Vec<LibraryEntry>, library_id: Option<LibraryId>, cx: &mut Context<Self>) {
        self.entries = entries;
        self.library_id = library_id;
        cx.notify();
        cx.emit(LibraryStateChanged);
    }

    pub(crate) fn set_entries(&mut self, entries: Vec<LibraryEntry>, cx: &mut Context<Self>) {
        let unchanged = self.entries.len() == entries.len() && self.entries.iter().zip(&entries).all(|(left, right)| left.library_id() == right.library_id() && left.library_name() == right.library_name());
        if unchanged {
            return;
        }
        self.entries = entries;
        cx.notify();
        cx.emit(LibraryStateChanged);
    }
}
