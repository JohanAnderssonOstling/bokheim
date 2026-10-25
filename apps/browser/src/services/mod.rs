//! Explicit service boundaries for browser UI code.

mod app;
mod book_command;
pub(crate) use book_command::BookCommand;
mod update_prompt;
pub(crate) mod updates;
#[cfg(not(target_arch = "wasm32"))]
pub use updates::bind_worker as bind_update_worker;
pub use updates::{PublishUpdate, UpdateAction, UpdateActionHandler, UpdateControls, UpdateEvent, UpdateView};
mod format;
pub(crate) use format::format_storage_bytes;
pub(crate) mod library_removal;
pub use library_removal::{LibraryRemovalHook, LibraryRemovalPreparation, LibraryRemovalService, library_is_being_removed};
use ::app::BackendError;
pub(crate) type UiFuture<T> = std::pin::Pin<Box<dyn Future<Output = Result<T, BackendError>> + 'static>>;
mod book_transfer_error;
mod catalog_progress;
pub(crate) use catalog_progress::track_catalog_import;
mod import_presentation;
pub(crate) use book_transfer_error::book_transfer_error_message;
pub(crate) mod import_progress;
mod notifications;
pub(crate) use import_presentation::import_failure_message;
pub(crate) use import_progress::ImportProgressToast;

pub(crate) use notifications::{notify_error, notify_info, notify_undoable, notify_warning};

#[cfg(feature = "kobo")]
pub use app::SaveKoboAutoRotate;
pub use app::{AppServices, AppStartup};
