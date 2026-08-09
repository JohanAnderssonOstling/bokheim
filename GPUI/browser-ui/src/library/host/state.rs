//! Passive catalogue selection shared with navigation and Sync.
//!
//! Backend selection and fallback policy live in [`LibraryHost`](super::LibraryHost).

use epub_svelte_lib::app::LibraryEntry;
use gpui::{Context, EventEmitter};

pub(crate) struct LibraryStateChanged;

pub(crate) struct LibraryState {
    entries: Vec<LibraryEntry>,
    library_id: Option<String>,
}

impl EventEmitter<LibraryStateChanged> for LibraryState {}

impl LibraryState {
    pub(super) fn new(entries: Vec<LibraryEntry>, library_id: Option<String>) -> Self {
        Self { entries, library_id }
    }

    pub(crate) fn entries(&self) -> &[LibraryEntry] {
        &self.entries
    }

    pub(crate) fn library_id(&self) -> Option<&str> {
        self.library_id.as_deref()
    }

    pub(crate) fn is_configured(&self) -> bool {
        !self.entries.is_empty()
    }

    pub(super) fn replace(&mut self, entries: Vec<LibraryEntry>, library_id: Option<String>, cx: &mut Context<Self>) {
        self.entries = entries;
        self.library_id = library_id;
        cx.notify();
        cx.emit(LibraryStateChanged);
    }

    pub(super) fn set_entries(&mut self, entries: Vec<LibraryEntry>, cx: &mut Context<Self>) {
        let unchanged = self.entries.len() == entries.len() && self.entries.iter().zip(&entries).all(|(left, right)| left.library_id() == right.library_id() && left.library_name() == right.library_name());
        if unchanged {
            return;
        }
        self.entries = entries;
        cx.notify();
        cx.emit(LibraryStateChanged);
    }
}
