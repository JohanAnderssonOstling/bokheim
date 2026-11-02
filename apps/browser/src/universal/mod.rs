//! Pages whose lifetime is independent of the selected library.

pub(crate) mod settings;
pub(crate) mod sync;
#[cfg(not(target_arch = "wasm32"))]
mod annotation_export;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use annotation_export::open_annotation_export;
#[cfg(all(not(feature = "kobo"), any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod reader_import;
#[cfg(all(not(feature = "kobo"), any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub(crate) use reader_import::open_reader_import;

pub(crate) use settings::SETTINGS_KEY_CONTEXT;

#[cfg(not(target_arch = "wasm32"))]
mod background_job;
