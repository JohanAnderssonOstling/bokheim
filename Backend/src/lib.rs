extern crate core;

pub mod auth;
pub mod db;
pub mod epub;
pub mod home;
pub mod library;
#[cfg(all(feature = "desktop", not(target_os = "android")))]
pub mod scanner;
#[cfg(not(all(feature = "desktop", not(target_os = "android"))))]
#[path = "scanner_stub.rs"]
pub mod scanner;
mod sync;
mod transfer;

pub type BookID = u64;

/// Initialize the logging system. Call this once at the start of your application.
///
/// Example:
/// ```no_run
/// epub_svelte_lib::init_logging();
/// ```
pub fn init_logging() {
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Warn)
        .filter_module("wgpu", log::LevelFilter::Warn)
        .filter_module("wgpu_core", log::LevelFilter::Warn)
        .filter_module("wgpu_hal", log::LevelFilter::Warn)
        .filter_module("zbus", log::LevelFilter::Warn)
        .filter_module("ashpd", log::LevelFilter::Warn)
        .init();
    log::info!("Logging initialized");
}
