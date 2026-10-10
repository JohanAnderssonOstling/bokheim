//! GPUI state scoped to the selected library.

mod context;
mod undo;
mod update_bridge;

pub(crate) use context::{BookDetailClosed, LibraryContext, OpenBook};
pub(crate) use undo::{UndoEntry, UndoStack, UndoStep};
pub(crate) use update_bridge::{LibraryContentsChanged, LibraryCoversChanged, LibraryDownloadChanged, LibraryScanChanged, LibraryUpdateBridge};
