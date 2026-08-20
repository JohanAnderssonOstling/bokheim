//! Explicit service boundaries for browser UI code.

mod app;

pub use app::OpenReader;
#[cfg(feature = "kobo")]
pub use app::SaveKoboAutoRotate;
pub use app::{AppServices, DesktopUpdate, DesktopUpdater};
