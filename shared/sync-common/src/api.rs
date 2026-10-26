//! Shared HTTP wire contracts used by both the client and server transports.
//!
//! These types deliberately contain no HTTP-framework or application logic.
//! Each boundary still authenticates, validates status codes, and maps errors
//! independently.

pub mod assets {
    use crate::BookId;
    use serde::{Deserialize, Serialize};
    use std::collections::HashSet;
    use std::fmt;

    /// Ephemeral read capability, never part of saved playback state.
    #[derive(Clone, Serialize, Deserialize)]
    pub struct PlaybackGrant {
        pub path: String,
        pub expires_in_seconds: u32,
        pub content_hash: BookId,
        pub length: u64,
    }
    impl fmt::Debug for PlaybackGrant {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("PlaybackGrant").field("expires_in_seconds", &self.expires_in_seconds).field("content_hash", &self.content_hash).finish_non_exhaustive()
        }
    }

    pub const MAX_BLOB_MANIFEST_ENTRIES: usize = 4096;
    pub const MAX_THUMBNAIL_PRESENCE_ENTRIES: usize = MAX_BLOB_MANIFEST_ENTRIES;
    pub const MAX_THUMBNAIL_BATCH_ENTRIES: usize = 16;
    pub const MAX_THUMBNAIL_BATCH_BYTES: usize = 2 * MAX_THUMBNAIL_BATCH_ENTRIES * crate::MAX_THUMBNAIL_BYTES as usize;
    pub const MAX_THUMBNAIL_BATCH_DOWNLOAD_BYTES: usize = MAX_THUMBNAIL_BATCH_BYTES + 1024 * 1024;

    #[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
    pub struct ThumbnailPresenceRequest {
        pub content_hashes: Vec<BookId>,
    }
    #[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
    pub struct ThumbnailPresenceResponse {
        pub present: Vec<BookId>,
        #[serde(default)]
        pub revisions: Vec<ThumbnailRevision>,
    }

    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
    pub struct ThumbnailRevision {
        pub content_hash: BookId,
        pub revision: BookId,
    }

    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
    pub struct BlobManifestEntry {
        pub content_hash: BookId,
        pub size_bytes: u64,
    }

    #[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
    pub struct BlobManifestRequest {
        pub blobs: Vec<BlobManifestEntry>,
    }

    #[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
    #[serde(rename_all = "snake_case")]
    pub enum BlobManifestRejectionReason {
        InvalidSize,
        SizeMismatch,
        QuotaExceeded,
        UploadInProgress,
    }

    #[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
    pub struct RejectedBlobManifestEntry {
        pub content_hash: BookId,
        pub reason: BlobManifestRejectionReason,
    }

    /// Every requested hash appears exactly once across these three collections.
    /// `upload` means the bytes still require the ordinary quota-checked upload
    /// admission path, even when another account stores the same blob.
    #[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
    pub struct BlobManifestResponse {
        pub owned: Vec<BookId>,
        pub upload: Vec<BookId>,
        pub rejected: Vec<RejectedBlobManifestEntry>,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum BlobManifestValidationError {
        DuplicateRequestHash,
        UnknownOrRepeatedResponseHash,
        OmittedResponseHash,
    }

    impl fmt::Display for BlobManifestValidationError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str(match self {
                Self::DuplicateRequestHash => "blob manifest contains duplicate hashes",
                Self::UnknownOrRepeatedResponseHash => "blob manifest response contains an unknown or repeated hash",
                Self::OmittedResponseHash => "blob manifest response omitted a requested hash",
            })
        }
    }

    impl std::error::Error for BlobManifestValidationError {}

    /// Ensures every requested blob is classified exactly once by the server.
    pub fn validate_blob_manifest_response(request: &BlobManifestRequest, response: &BlobManifestResponse) -> Result<(), BlobManifestValidationError> {
        let requested: HashSet<_> = request.blobs.iter().map(|blob| blob.content_hash).collect();
        if requested.len() != request.blobs.len() {
            return Err(BlobManifestValidationError::DuplicateRequestHash);
        }
        let classified = response.owned.iter().chain(&response.upload).chain(response.rejected.iter().map(|entry| &entry.content_hash));
        let mut unique = HashSet::with_capacity(requested.len());
        for hash in classified {
            if !requested.contains(hash) || !unique.insert(*hash) {
                return Err(BlobManifestValidationError::UnknownOrRepeatedResponseHash);
            }
        }
        if unique != requested {
            return Err(BlobManifestValidationError::OmittedResponseHash);
        }
        Ok(())
    }

    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
    pub struct ThumbnailBatchEntry {
        pub content_hash: BookId,
        pub bytes: Vec<u8>,
        pub browse_bytes: Vec<u8>,
    }

    #[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
    pub struct ThumbnailBatchUploadRequest {
        pub thumbnails: Vec<ThumbnailBatchEntry>,
    }

    #[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
    pub struct ThumbnailBatchDownloadRequest {
        pub content_hashes: Vec<BookId>,
    }

    #[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
    pub struct ThumbnailBatchDownloadResponse {
        pub thumbnails: Vec<ThumbnailBatchDownloadEntry>,
        pub missing: Vec<BookId>,
    }

    pub type ThumbnailBatchDownloadEntry = ThumbnailBatchEntry;
}

pub mod libraries {
    use crate::LibraryId;
    use serde::{Deserialize, Serialize};

    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
    pub struct LibrarySummary {
        pub library_id: LibraryId,
        pub library_name: String,
        #[serde(default)]
        pub storage: crate::storage::LibraryStorage,
    }

    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
    pub struct LibraryNameRequest {
        pub library_id: LibraryId,
        pub library_name: String,
        #[serde(default)]
        pub storage: crate::storage::LibraryStorage,
    }

    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
    pub struct CloudStorageChange {
        pub change_id: uuid::Uuid,
        pub enabled: bool,
    }

    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
    pub struct LibraryCloudStorage {
        pub library_id: LibraryId,
        pub enabled: bool,
    }

    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
    pub struct DeleteLibraryResponse {
        pub library_id: LibraryId,
    }
}

pub mod opds_feeds {
    use serde::{Deserialize, Serialize};

    pub type OpdsFeedId = uuid::Uuid;

    /// One of the account's OPDS feeds. Credentials for it stay on devices.
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
    pub struct OpdsFeed {
        pub id: OpdsFeedId,
        pub name: String,
        pub url: String,
    }
}

pub mod notifications {
    use crate::LibraryId;
    use serde::{Deserialize, Serialize};

    /// Best-effort wakeups carried by the authenticated WebSocket. Durable
    /// state and cursors remain in the ordinary synchronization protocol.
    /// Empty binary frames are heartbeats; clients echo them without reconciling.
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
    #[serde(rename_all = "snake_case")]
    pub enum NotificationMessage {
        Hello { heartbeat_seconds: u64, timeout_seconds: u64 },
        LibraryChanged { library_id: LibraryId },
        LibrariesChanged,
        Reconcile,
    }
}
