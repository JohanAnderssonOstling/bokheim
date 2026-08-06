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

    #[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
    pub struct BlobsExistsRequest {
        pub hashes: Vec<ContentHash>,
    }

    #[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
    pub struct ThumbnailsExistsRequest {
        pub content_hashes: Vec<ContentHash>,
    }

    #[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
    pub struct ExistsResponse {
        pub present: Vec<ContentHash>,
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
        LibraryChanged { library_id: LibraryId, reading: bool, other: bool, assets: bool },
        CatalogueChanged,
        PreferencesChanged,
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
        ContentHash::parse(&character.to_string().repeat(64)).unwrap()
    }

    #[test]
    fn authentication_contracts_have_distinct_access_and_refresh_credentials() {
        let login = auth::LoginRequest { username: "reader".into(), password: "secret".into() };
        let login = round_trip(&login);
        assert_eq!((login.username.as_str(), login.password.as_str()), ("reader", "secret"));

        let signup = auth::SignupRequest { username: "reader".into(), password: "secret".into(), invite: "invite-code".into() };
        let signup = round_trip(&signup);
        assert_eq!((signup.username.as_str(), signup.password.as_str(), signup.invite.as_str()), ("reader", "secret", "invite-code"));

        let registration = auth::PublicRegistrationRequest { email: "reader@example.com".into(), username: "reader".into(), password: "secret".into() };
        let registration = round_trip(&registration);
        assert_eq!((registration.email.as_str(), registration.username.as_str(), registration.password.as_str()), ("reader@example.com", "reader", "secret"));

        let response = round_trip(&auth::AuthResponse { token: "token".into(), expires_at: 1_900_000_000, refresh_token: "refresh".into(), refresh_expires_at: 1_910_000_000, user_id: "user-1".into(), username: "reader".into() });
        assert_eq!(response.token, "token");
        assert_eq!(response.expires_at, 1_900_000_000);
        assert_eq!(response.refresh_token, "refresh");
        assert_eq!(response.refresh_expires_at, 1_910_000_000);
        assert_eq!(response.user_id, "user-1");
        assert_eq!(response.username, "reader");
    }

    #[test]
    fn asset_contracts_keep_book_and_thumbnail_request_keys_distinct() {
        let book_hash = hash('a');
        let thumbnail_hash = hash('b');
        assert_eq!(round_trip(&assets::BlobsExistsRequest { hashes: vec![book_hash.clone()] }).hashes, vec![book_hash.clone()]);
        assert_eq!(round_trip(&assets::ThumbnailsExistsRequest { content_hashes: vec![thumbnail_hash.clone()] }).content_hashes, vec![thumbnail_hash.clone()]);
        let response = round_trip(&assets::ExistsResponse { present: vec![book_hash.clone(), thumbnail_hash.clone()] });
        assert_eq!(response.present, vec![book_hash, thumbnail_hash]);
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
        let message = notifications::NotificationMessage::LibraryChanged { library_id: LibraryId::parse("library-1").unwrap(), reading: true, other: false, assets: false };
        assert_eq!(round_trip(&message), message);
        assert_eq!(crate::wire::encode(&notifications::NotificationMessage::CatalogueChanged).unwrap(), b"BKHM\x05\x02");
    }
}
