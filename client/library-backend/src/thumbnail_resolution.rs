/// Thumbnail requests distinguish card densities from the unmodified detail
/// image. Stored pixel widths stay owned by the thumbnail crate.
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
            Self::Browse => thumbnail::BROWSE_THUMBNAIL_WIDTH,
            Self::CardHighDensity | Self::HighDensity => thumbnail::THUMBNAIL_WIDTH,
        }
    }
}
