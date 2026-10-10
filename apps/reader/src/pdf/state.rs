//! Pure PDF reader preference and progress projections.

use pdf_view_gpui::{MAX_VISIBLE_PAGE_COUNT, MIN_VISIBLE_PAGE_COUNT, PdfZoomMode};

use crate::settings::{PdfZoomModePreference, ReaderPreferences};

pub(super) fn stored_zoom_mode(preferences: &ReaderPreferences) -> PdfZoomMode {
    match preferences.pdf_zoom_mode {
        PdfZoomModePreference::FitWidth => PdfZoomMode::FitWidth { columns: usize::from(preferences.pdf_page_count).clamp(MIN_VISIBLE_PAGE_COUNT, MAX_VISIBLE_PAGE_COUNT) },
        PdfZoomModePreference::Fixed => PdfZoomMode::Fixed { zoom: preferences.pdf_zoom },
        PdfZoomModePreference::FitHeight => PdfZoomMode::FitHeight,
    }
}

pub(super) fn store_zoom_mode(zoom_mode: PdfZoomMode, preferences: &mut ReaderPreferences) {
    match zoom_mode {
        PdfZoomMode::FitWidth { columns } => {
            preferences.pdf_zoom_mode = PdfZoomModePreference::FitWidth;
            preferences.pdf_page_count = columns as u8;
        }
        PdfZoomMode::Fixed { zoom } => {
            preferences.pdf_zoom_mode = PdfZoomModePreference::Fixed;
            preferences.pdf_zoom = zoom;
        }
        PdfZoomMode::FitHeight => preferences.pdf_zoom_mode = PdfZoomModePreference::FitHeight,
    }
}

pub(super) fn page_indicator(visible_page_range: Option<(usize, usize)>, page_count: usize) -> String {
    let Some((first, last)) = visible_page_range else {
        return format!("0/{page_count}");
    };
    if first == last { format!("{}/{page_count}", first + 1) } else { format!("{}–{}/{page_count}", first + 1, last + 1) }
}

pub(super) fn progress(visible_page_range: Option<(usize, usize)>, page_count: usize) -> String {
    let Some((_, last)) = visible_page_range else {
        return "0%".to_owned();
    };
    format!("{:.0}%", crate::shell::persistence::StoredPosition::pdf_page(last, 0.0, page_count).percent())
}
