//! Explicit service boundaries for browser UI code.

mod app;

#[cfg(feature = "kobo")]
pub use app::SaveKoboAutoRotate;
pub use app::{AppServices, DesktopUpdate, DesktopUpdater};
pub use app::{OpenReader, SaveBrowserState};
