//! GPUI reader windows.

mod model;
mod pdf_reader;
mod state;
mod view;
mod windows;

use gpui::actions;

pub use model::{ReaderPreferences, ReaderWindowGeometry, ReaderWindowId, SaveReaderSettings};
pub use windows::{close_readers_for_library, configure, handle_book_removed, open_reader_window};

actions!(
    bokheim_reader,
    [
        PreviousPage,
        NextPage,
        ToggleToc,
        FocusReaderSearch,
        ToggleSearch,
        CloseSearch,
        NavigateHistoryBack,
        NavigateHistoryForward,
        IncreaseFont,
        DecreaseFont,
        IncreaseScale,
        DecreaseScale,
        IncreaseColumnWidth,
        DecreaseColumnWidth,
        ToggleLineNumbers
    ]
);

pub fn bind_keys(cx: &mut gpui::App) {
    view::bind_keys(cx);
    pdf_reader::bind_pdf_keys(cx);
}
