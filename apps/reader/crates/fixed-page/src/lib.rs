//! Page-image retention, geometry, and gestures shared by fixed-page readers.

mod cache;
mod layout;
mod swipe;

pub use cache::PageCache;
pub use layout::{PageLayout, PageZoom};
pub use swipe::PageSwipe;

pub const DISPLAY_CACHE_BUDGET: usize = 64 * 1024 * 1024;

#[cfg(feature = "gpui")]
mod raster;
#[cfg(feature = "gpui")]
pub use raster::PageImage;
