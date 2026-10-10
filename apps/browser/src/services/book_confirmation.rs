//! Confirmation wording belongs to the book command being performed.

use std::future::Future;

use gpui::{App, PromptLevel, Window};

use super::notify_error;

#[derive(Clone, Copy)]
pub(crate) enum BookConfirmation {
    RemoveFromFolder,
    TrashBook,
    TrashBooks(usize),
}

impl BookConfirmation {
    pub(crate) fn prompt(self, detail: &str, window: &mut Window, cx: &mut App) -> impl Future<Output = bool> + use<> {
        let (title, accept) = match self {
            Self::RemoveFromFolder => ("Remove this book from the folder?", "Remove"),
            Self::TrashBook | Self::TrashBooks(1) => ("Move this book to Trash?", "Move to Trash"),
            Self::TrashBooks(_) => ("Move these books to Trash?", "Move to Trash"),
        };
        let answer = window.prompt(PromptLevel::Warning, title, Some(detail), &[accept, "Cancel"], cx);
        async move { answer.await.ok() == Some(0) }
    }

    pub(crate) fn report_failure(self, error: String, window: &mut Window, cx: &mut App) {
        let (id, prefix) = match self {
            Self::RemoveFromFolder => ("book-remove-error", "Could not remove book from folder"),
            Self::TrashBook => ("book-trash-error", "Could not move book to Trash"),
            Self::TrashBooks(_) => ("book-trash-error", "Could not move selected books to Trash"),
        };
        notify_error(id, format!("{prefix}: {error}"), window, cx);
    }
}
