//! Shared HTTP wire contracts used by both the client and server transports.
//!
//! These types deliberately contain no HTTP-framework or application logic.
//! Each boundary still authenticates, validates status codes, and maps errors
//! independently.

pub use bokheim_account_contract as auth;

pub mod assets {
    use crate::ContentHash;
    use serde::{Deserialize, Serialize};

    pub const MAX_BLOB_MANIFEST_ENTRIES: usize = 4096;

    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
    pub struct BlobManifestEntry {
        pub content_hash: ContentHash,
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
        pub content_hash: ContentHash,
        pub reason: BlobManifestRejectionReason,
    }

    /// Every requested hash appears exactly once across these four collections.
    /// `claimed` means a globally stored, validated blob was charged to this
    /// account atomically; `upload` means the bytes still require the ordinary
    /// quota-checked upload admission path.
    #[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
    pub struct BlobManifestResponse {
        pub owned: Vec<ContentHash>,
        pub claimed: Vec<ContentHash>,
        pub upload: Vec<ContentHash>,
        pub rejected: Vec<RejectedBlobManifestEntry>,
    }
}

pub mod libraries {
    use crate::LibraryId;
    use serde::{Deserialize, Serialize};

    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
    pub struct LibrarySummary {
        pub library_id: LibraryId,
        pub library_name: String,
    }

    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
    pub struct UpsertLibraryRequest {
        pub library_id: LibraryId,
        pub library_name: String,
    }
}

pub mod notifications {
    use crate::LibraryId;
    use serde::{Deserialize, Serialize};

    /// Best-effort wakeups carried by the authenticated WebSocket. Durable
    /// state and cursors remain in the ordinary synchronization protocol.
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
    #[serde(rename_all = "snake_case")]
    pub enum NotificationMessage {
        Hello { heartbeat_seconds: u64, timeout_seconds: u64 },
        LibraryChanged { library_id: LibraryId },
        CatalogueChanged,
        Reconcile,
    }
}

#[cfg(test)]
mod tests {
    use super::{assets, auth, libraries, notifications};
    use crate::{ContentHash, LibraryId};

    fn round_trip<T>(value: &T) -> T
    where
        T: serde::Serialize + serde::de::DeserializeOwned,
    {
        let encoded = crate::wire::encode(value).unwrap();
        crate::wire::decode(&encoded, crate::wire::MAX_DECODED_REQUEST_BYTES).unwrap()
    }

    fn hash(character: char) -> ContentHash {
        ContentHash::new(&character.to_string().repeat(64))
    }

    #[test]
    fn authentication_contracts_have_distinct_access_and_refresh_credentials() {
        let login = auth::LoginRequest { email: "reader@example.com".into(), password: "secret".into() };
        let login = round_trip(&login);
        assert_eq!((login.email.as_str(), login.password.as_str()), ("reader@example.com", "secret"));

        let registration = auth::PublicRegistrationRequest { email: "reader@example.com".into(), password: "secret".into() };
        let registration = round_trip(&registration);
        assert_eq!((registration.email.as_str(), registration.password.as_str()), ("reader@example.com", "secret"));

        let response = round_trip(&auth::AuthResponse { token: "token".into(), expires_at: 1_900_000_000, refresh_token: "refresh".into(), refresh_expires_at: 1_910_000_000, user_id: "user-1".into(), email: "reader@example.com".into() });
        assert_eq!(response.token, "token");
        assert_eq!(response.expires_at, 1_900_000_000);
        assert_eq!(response.refresh_token, "refresh");
        assert_eq!(response.refresh_expires_at, 1_910_000_000);
        assert_eq!(response.user_id, "user-1");
        assert_eq!(response.email, "reader@example.com");
    }

    #[test]
    fn blob_manifest_contract_classifies_each_hash_without_ambiguous_booleans() {
        let owned = hash('a');
        let upload = hash('b');
        let rejected = hash('c');
        let request = assets::BlobManifestRequest { blobs: vec![assets::BlobManifestEntry { content_hash: upload.clone(), size_bytes: 42 }] };
        assert_eq!(round_trip(&request), request);

        let response = assets::BlobManifestResponse {
            owned: vec![owned.clone()],
            claimed: Vec::new(),
            upload: vec![upload.clone()],
            rejected: vec![assets::RejectedBlobManifestEntry { content_hash: rejected.clone(), reason: assets::BlobManifestRejectionReason::QuotaExceeded }],
        };
        assert_eq!(round_trip(&response), response);
    }

    #[test]
    fn library_catalogue_contract_preserves_stable_ids_and_names() {
        let request = libraries::UpsertLibraryRequest { library_id: LibraryId::parse("library-1").unwrap(), library_name: "History".to_string() };
        let summary = libraries::LibrarySummary { library_id: request.library_id.clone(), library_name: request.library_name.clone() };
        assert_eq!(round_trip(&request), request);
        assert_eq!(round_trip(&summary), summary);
    }

    #[test]
    fn notification_contract_is_granular_without_carrying_state() {
        let message = notifications::NotificationMessage::LibraryChanged { library_id: LibraryId::parse("library-1").unwrap() };
        assert_eq!(round_trip(&message), message);
        let expected: Vec<u8> = crate::wire::header_bytes().into_iter().chain([0x02]).collect();
        assert_eq!(crate::wire::encode(&notifications::NotificationMessage::CatalogueChanged).unwrap(), expected);
    }
}
