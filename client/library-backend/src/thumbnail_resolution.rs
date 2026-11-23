use thumbnail_contract::{BROWSE_THUMBNAIL_WIDTH, THUMBNAIL_WIDTH};

/// Thumbnail requests distinguish card densities from the unmodified detail
/// image. Stored pixel widths stay owned by the thumbnail contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum ThumbnailResolution {
    /// Adjusted 300px card JPEG.
    Browse,
    /// Adjusted 600px card JPEG.
    CardHighDensity,
    /// Unmodified 600px detail/master JPEG.
    HighDensity,
}

impl ThumbnailResolution {
    pub const fn width(self) -> u32 {
        match self {
            Self::Browse => BROWSE_THUMBNAIL_WIDTH,
            Self::CardHighDensity | Self::HighDensity => THUMBNAIL_WIDTH,
        }
    }
}
