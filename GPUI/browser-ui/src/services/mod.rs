//! Explicit service boundaries for browser UI code.

mod app;

pub use app::AppServices;
#[cfg(feature = "kobo")]
pub use app::SaveKoboAutoRotate;
pub use app::{OpenReader, SaveBrowserState};
