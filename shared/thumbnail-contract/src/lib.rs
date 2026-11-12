//! Encoded thumbnail results shared by client workers and server ingestion.
use serde::{Deserialize, Serialize};

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
