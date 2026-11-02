//! Pure PDF reader preference and progress projections.

use ui_text::text;

use app_preferences::ReaderPreferencePatch;
use pdf_view_gpui::{MAX_VISIBLE_PAGE_COUNT, MIN_VISIBLE_PAGE_COUNT, PdfZoomMode};

use crate::settings::{PdfZoomModePreference, ReaderPreferences};

pub(super) fn stored_zoom_mode(preferences: &ReaderPreferences) -> PdfZoomMode {
    match preferences.synchronized.pdf_zoom_mode() {
        PdfZoomModePreference::FitWidth => PdfZoomMode::FitWidth { columns: usize::from(preferences.synchronized.pdf_page_count()).clamp(MIN_VISIBLE_PAGE_COUNT, MAX_VISIBLE_PAGE_COUNT) },
        PdfZoomModePreference::Fixed => PdfZoomMode::Fixed { zoom: preferences.synchronized.pdf_zoom() },
        PdfZoomModePreference::FitHeight => PdfZoomMode::FitHeight,
    }
}

pub(super) fn store_zoom_mode(zoom_mode: PdfZoomMode, preferences: &mut ReaderPreferences) {
    match zoom_mode {
        PdfZoomMode::FitWidth { columns } => {
            preferences.apply_preference(ReaderPreferencePatch::PdfZoomMode(PdfZoomModePreference::FitWidth));
            preferences.apply_preference(ReaderPreferencePatch::PdfPageCount(columns as u8));
        }
        PdfZoomMode::Fixed { zoom } => {
            preferences.apply_preference(ReaderPreferencePatch::PdfZoomMode(PdfZoomModePreference::Fixed));
            preferences.apply_preference(ReaderPreferencePatch::PdfZoom(zoom));
        }
        PdfZoomMode::FitHeight => preferences.apply_preference(ReaderPreferencePatch::PdfZoomMode(PdfZoomModePreference::FitHeight)),
    }
}

pub(super) fn page_indicator(visible_page_range: Option<(usize, usize)>, page_count: usize) -> String {
    let Some((first, last)) = visible_page_range else {
        return format!("0/{page_count}");
    };
    if first == last { format!("{}/{page_count}", first + 1) } else { format!(text!(reader::page_count), first + 1, last + 1, page_count = page_count) }
}

pub(super) fn progress(visible_page_range: Option<(usize, usize)>, page_count: usize) -> (f32, String) {
    let percent = visible_page_range.map_or(0.0, |(_, last)| crate::shell::persistence::StoredPosition::pdf_page(last, 0.0, page_count).percent());
    ((percent / 100.0).clamp(0.0, 1.0), format!("{percent:.0}%"))
}
