//! Thumbnail payloads and cache naming shared by clients and server ingestion.
use serde::{Deserialize, Serialize};

/// Covers are displayed at `LARGE_BOOK_COVER_WIDTH_PX` *logical* pixels, so the
/// stored asset has to carry the device pixel ratio too: 200 logical at DPR 3
/// is 600 real pixels. Anything smaller is upscaled on HiDPI displays and on
/// phones.
///
/// The 600px asset remains the synchronized source of truth. The 300px browse
/// bucket is cached on the server and device, and can be rebuilt from it.
pub const BROWSE_THUMBNAIL_WIDTH: u32 = 300;
pub const THUMBNAIL_WIDTH: u32 = 600;

/// Increment when the crop policy changes, to retire previously cached cards.
pub const CARD_THUMBNAIL_VERSION: u32 = 5;

/// Cache suffix shared by generated, stored and downloaded card thumbnails.
pub fn card_thumbnail_suffix(width: u32) -> String {
    format!("card-v{CARD_THUMBNAIL_VERSION}-{width}")
}

/// An unmodified high-density master plus adjusted 300px and 600px cards.
/// Every derivative is produced independently from the decoded original.
#[derive(Serialize, Deserialize)]
pub struct ThumbnailVersions {
    #[serde(with = "serde_bytes")]
    pub browse: Vec<u8>,
    #[serde(with = "serde_bytes")]
    pub card_high_density: Vec<u8>,
    #[serde(with = "serde_bytes")]
    pub high_density: Vec<u8>,
}
