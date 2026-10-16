//! Failure wording belongs to the book command being performed.
//!
//! Book commands run without asking first: moving to Trash and removing from a
//! folder can both be undone from Trash, so a prompt only slowed down the
//! common case.

use ui_components::user_facing_error;

use gpui::{App, Window};

use super::notify_error;

#[derive(Clone, Copy)]
pub(crate) enum BookCommand {
    RemoveFromFolder,
    TrashBook,
    TrashBooks,
}

impl BookCommand {
    pub(crate) fn report_failure(self, error: String, window: &mut Window, cx: &mut App) {
        let (id, prefix) = match self {
            Self::RemoveFromFolder => ("book-remove-error", "Could not remove book from folder"),
            Self::TrashBook => ("book-trash-error", "Could not move book to Trash"),
            Self::TrashBooks => ("book-trash-error", "Could not move selected books to Trash"),
        };
        notify_error(id, format!("{prefix}. {}", user_facing_error(&error, "Please try again.")), window, cx);
    }
}
