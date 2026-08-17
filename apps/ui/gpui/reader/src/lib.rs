//! GPUI reader windows.

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

mod backends;
mod cbz_reader;
mod epub;
mod invalidation;
#[cfg(feature = "pdf")]
mod pdf;
#[cfg(feature = "pdf")]
mod pdf_reader;
mod publication;
mod settings;
mod shell;
mod windows;

use gpui::actions;

pub use backends::{NativeAudioBackend, NativeAudioState, NativeAudioStatus, SystemAudioBackend, SystemTtsProvider, TtsProvider, TtsState};
pub use epub::ReaderWindowId;
pub use epub_svelte_lib::app::preferences::PdfZoomMode;
pub use epub_svelte_lib::app::preferences::{ReaderFontFamily, ReaderTextAlignment};
pub use settings::{ReaderPreferences, ReaderWindowGeometry, SaveReaderSettings};
pub use windows::{close_readers_for_library, configure, configure_audio_backend, configure_tts_provider, handle_book_removed, open_reader_window};

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
        NavigateHistoryBack,
        NavigateHistoryForward,
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
    epub::render::bind_keys(cx);
    cbz_reader::bind_cbz_keys(cx);
    #[cfg(feature = "pdf")]
    pdf_reader::bind_pdf_keys(cx);
}

/// Returns debug-only invalidation counts grouped by component and reason.
#[cfg(debug_assertions)]
pub fn notification_counts() -> Vec<((invalidation::Component, &'static str), u64)> {
    invalidation::snapshot()
}

#[cfg(debug_assertions)]
pub use invalidation::{Component, ReaderPerformanceSnapshot, ReaderRepaintBudgetEvaluation, ReaderRepaintScenario};

/// Returns reader repaint and render timing measurements accumulated since the
/// previous snapshot or reset.
#[cfg(debug_assertions)]
pub fn performance_snapshot() -> ReaderPerformanceSnapshot {
    invalidation::performance_snapshot()
}

#[cfg(debug_assertions)]
pub fn reset_performance_measurements() {
    invalidation::reset_performance_measurements();
}
