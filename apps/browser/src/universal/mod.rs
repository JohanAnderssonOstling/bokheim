//! Pages whose lifetime is independent of the selected library.

#[cfg(not(target_arch = "wasm32"))]
mod annotation_export;
pub(crate) mod settings;
pub(crate) mod sync;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use annotation_export::open_annotation_export;
mod library_downloads;
pub(crate) use library_downloads::library_book_menu;

pub(crate) use settings::SETTINGS_KEY_CONTEXT;
