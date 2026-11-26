//! Explicit service boundaries for browser UI code.

mod app;
mod book_command;
pub(crate) mod browser;
pub(crate) use book_command::{BookCommand, open_book_windows};
pub(crate) mod updates;
#[cfg(not(target_arch = "wasm32"))]
pub use updates::bind_worker as bind_update_worker;
pub use updates::{PublishUpdate, UpdateAction, UpdateActionHandler, UpdateControls, UpdateEvent, UpdateView};
mod format;
pub(crate) use format::{download_byte_progress, format_storage_bytes};
pub(crate) mod library_removal;
pub use library_removal::{LibraryRemovalHook, LibraryRemovalPreparation, LibraryRemovalService, library_is_being_removed};
mod book_transfer_error;
mod catalog_import;
pub(crate) use catalog_import::import_catalog_book_with_progress;
mod import_presentation;
pub(crate) use book_transfer_error::book_transfer_error_message;
pub(crate) mod notifications;
pub(crate) use import_presentation::import_failure_message;

pub(crate) use notifications::{notify_error, notify_info, notify_undoable, notify_warning};

#[cfg(feature = "kobo")]
pub use app::SaveKoboAutoRotate;
pub(crate) use app::preference_client;
pub use app::{AppServices, AppStartup};

/// Shared limits for editable folder and library names. Backend validation
/// still handles filesystem-specific restrictions.
pub(crate) fn name_error(name: &str, empty_message: &'static str) -> Option<&'static str> {
    if name.is_empty() {
        Some(empty_message)
    } else if name.len() > 255 {
        Some(ui_text::text!(browse::name_too_long))
    } else if name.chars().any(char::is_control) {
        Some(ui_text::text!(browse::name_control_characters))
    } else {
        None
    }
}

/// Popup dismissal restores focus in a deferred callback; wait through it
/// before a new modal claims focus.
pub(crate) fn after_menu_dismiss(window: &mut gpui::Window, cx: &mut gpui::App, action: impl FnOnce(&mut gpui::Window, &mut gpui::App) + 'static) {
    window.defer(cx, move |window, cx| window.defer(cx, action));
}
