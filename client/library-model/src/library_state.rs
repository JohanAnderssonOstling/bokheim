use crate::{BookMetadataState, Contributor, PublicationMetadata, ReadingPosition, UnixMillis};
use serde::{Deserialize, Serialize};

use crate::BookFormat;
use unicode_normalization::UnicodeNormalization;

/// Portable sibling-name identity used for folders and materialized files.
/// Display casing and accents are retained separately; comparison uses
/// compatibility normalization plus locale-independent lowercase mapping.
pub fn portable_name_key(value: &str) -> String {
    value.nfkc().flat_map(char::to_lowercase).collect()
}

/// The metadata sync register. Descriptions belong to a different register,
/// so this type cannot represent a description update or accidental clear.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncBookMetadata {
    pub title: String,
    pub contributors: Vec<Contributor>,
    pub publication: PublicationMetadata,
}

impl SyncBookMetadata {
    pub fn subtitle(&self) -> Option<&str> { self.publication.subtitle() }
}

impl From<BookMetadataState> for SyncBookMetadata {
    fn from(value: BookMetadataState) -> Self {
        Self { title: value.title, contributors: value.contributors, publication: value.publication }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BookLifecycleState {
    Present {
        metadata: SyncBookMetadata,
        added_at: UnixMillis,
        format: BookFormat,
    },
    /// Hidden from the active library, but retained in Trash with its assets
    /// and reading state intact.
    Deleted {
        /// Set only when deleting a folder removed the final active placement.
        /// This lets any replica restore exactly the books orphaned by that
        /// folder, without reviving books trashed independently beforehand.
        origin_folder_id: Option<String>,
    },
    /// Permanently removed. Replicas retain only the lifecycle tombstone so an
    /// older offline `Present` value cannot resurrect the book.
    Purged,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DirectoryLifecycleState {
    Present,
    Deleted,
    Purged,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReadingPositionState {
    pub location: ReadingPosition,
    pub progress: f32,
}
