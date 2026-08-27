//! GPUI reader surfaces embedded by the application shell.

/// Records a book-open startup phase against the `book_startup` log target.
///
/// Book launch is profiled phase by phase on slow e-ink hardware. Routing the
/// trace through `log` keeps it off stdout in normal runs while leaving one
/// named seam to redirect if the device workflow needs something else.
macro_rules! startup_phase {
    ($($arg:tt)*) => {
        log::debug!(target: "book_startup", $($arg)*)
    };
}

mod epub;
mod invalidation;
#[cfg(feature = "pdf")]
mod pdf;
mod publication;
mod settings;
mod shell;
mod windows;

use gpui::actions;

pub use app_preferences::PdfZoomMode;
pub use app_preferences::{ReaderFontFamily, ReaderTextAlignment};
pub use settings::{ReaderPreferences, SaveReaderSettings};
pub use windows::{ReaderView, ReturnToLibrary, configure};

actions!(
    bokheim_reader,
    [
        PreviousPage,
        NextPage,
        PreviousLine,
        NextLine,
        PreviousSection,
        NextSection,
        ToggleToc,
        FocusReaderSearch,
        ToggleSearch,
        CloseSearch,
        IncreaseFont,
        DecreaseFont,
        IncreaseScale,
        DecreaseScale,
        IncreaseColumnWidth,
        DecreaseColumnWidth,
        ToggleFullscreen
    ]
);

pub fn bind_keys(cx: &mut gpui::App) {
    shell::input::bind_reader_keys(cx);
    epub::render::bind_keys(cx);
    #[cfg(feature = "pdf")]
    pdf::bind_pdf_keys(cx);
}

/// Returns debug-only invalidation counts grouped by component and reason.
#[cfg(debug_assertions)]
pub fn notification_counts() -> Vec<((invalidation::Component, &'static str), u64)> {
    invalidation::snapshot()
}

#[cfg(debug_assertions)]
pub use invalidation::{Component, ReaderPerformanceSnapshot};

/// Returns reader invalidation and render counts accumulated since the previous
/// reset.
#[cfg(debug_assertions)]
pub fn performance_snapshot() -> ReaderPerformanceSnapshot {
    invalidation::performance_snapshot()
}

#[cfg(debug_assertions)]
pub fn reset_performance_measurements() {
    invalidation::reset_performance_measurements();
}
