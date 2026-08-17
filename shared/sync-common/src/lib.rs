//! Stable synchronization protocol shared by clients and the sync service.
//!
//! Library payload bytes are intentionally opaque here. Their client-side
//! schema and conversion live in `library-replica`.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::num::NonZeroU64;

pub mod api;
pub use protobuf_wire as transport;
pub use wire;
pub mod mutation_ids;
pub use mutation_ids::{MutationId, MutationIdError};
mod protocol;
pub use protocol::{validate_pull_batch, validate_push_response, PullBatchError, PushResponseError, ValidatedPushResponse};

pub use content_address::ContentHash;

/// A non-negative Unix timestamp measured in milliseconds.
pub type UnixMillis = u64;

/// Mutation kinds whose envelope-level meaning is visible to the sync service.
/// Payload schemas remain client-owned in `library-replica`.
pub mod mutation_kind {
    pub const ANNOTATION: &str = "annotation";
    pub const BOOK_LIFECYCLE: &str = "book_lifecycle";
    pub const BOOK_LINK: &str = "book_link";
    pub const BOOKMARK: &str = "bookmark";
    pub const CATALOG_PUBLICATION: &str = "catalog_publication";
    pub const DIRECTORY_LIFECYCLE: &str = "directory_lifecycle";
    pub const DIRECTORY_NAME: &str = "directory_name";
    pub const DIRECTORY_PARENT: &str = "directory_parent";
    pub const METADATA: &str = "metadata";
    pub const PLACEMENT: &str = "placement";
    pub const READING_POSITION: &str = "reading_position";
}

/// The replica that produced a synchronized mutation.
pub type ReplicaId = uuid::Uuid;
/// A synchronized library.
pub type LibraryId = uuid::Uuid;
/// A synchronized directory.
pub type DirId = uuid::Uuid;
/// A file name is user-visible text, not an identity.
pub type FileName = String;

/// The canonical root directory. No generated directory can collide with nil.
pub const ROOT_DIR_ID: DirId = uuid::Uuid::nil();

/// Maximum encoded size of a synchronized JPEG cover.
pub const MAX_THUMBNAIL_BYTES: u64 = 4 * 1024 * 1024;
pub const MAX_PULL_CHANGES: usize = 1_000;
pub const MAX_PUSH_MUTATIONS: usize = 1_000;
pub const MAX_PUSH_MUTATION_BYTES: usize = 4 * 1024 * 1024;
/// Admission bound shared by the generic service and domain clients.
pub const MAX_LWW_FUTURE_SKEW_MS: u64 = 5 * 60 * 1_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SequenceError;

impl fmt::Display for SequenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("sequence numbers must be between one and the signed 64-bit database limit")
    }
}

impl std::error::Error for SequenceError {}

macro_rules! sequence_type {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(NonZeroU64);

        impl $name {
            pub fn new(value: u64) -> Result<Self, SequenceError> {
                if value > i64::MAX as u64 {
                    return Err(SequenceError);
                }
                NonZeroU64::new(value).map(Self).ok_or(SequenceError)
            }

            pub const fn get(self) -> u64 {
                self.0.get()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.get().fmt(formatter)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                Self::new(u64::deserialize(deserializer)?).map_err(serde::de::Error::custom)
            }
        }
    };
}

sequence_type!(ReplicaSeq);
sequence_type!(LibraryRevision);

/// A checkpoint over independently paged synchronization channels.
/// A component advances only after that channel has been inspected.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncCursor {
    #[serde(default, deserialize_with = "deserialize_library_cursor")]
    pub state_revision: Option<LibraryRevision>,
    #[serde(default, deserialize_with = "deserialize_library_cursor")]
    pub reading_revision: Option<LibraryRevision>,
}

/// Opaque mutation envelope transported and persisted by the sync service.
/// The service does not decode `value`; clients own each kind's domain schema.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WireMutation {
    pub mutation_id: MutationId,
    pub kind: String,
    pub entity_key: String,
    #[serde(default)]
    pub entity_subkey: String,
    pub value: Vec<u8>,
    pub conflict_rank: u8,
    #[serde(default)]
    pub blob_reference: Option<DeclaredBlobReference>,
    pub changed_at: UnixMillis,
    pub replica_seq: ReplicaSeq,
}

/// The server-visible lifecycle fact used solely for book-blob accounting.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeclaredBlobReference {
    pub present: bool,
    pub content_hash: Option<ContentHash>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServerMutation {
    pub mutation: WireMutation,
    pub replica_id: ReplicaId,
    pub revision: LibraryRevision,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PushMutationsRequest {
    pub library_id: LibraryId,
    pub replica_id: ReplicaId,
    pub mutations: Vec<WireMutation>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PushMutationsResponse {
    #[serde(default)]
    pub accepted: Vec<MutationId>,
    #[serde(default)]
    pub rejected: Vec<MutationRejection>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MutationRejection {
    pub mutation_id: MutationId,
    pub reason: MutationRejectionReason,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MutationRejectionReason {
    TimestampTooFarFuture,
    InvalidPayload,
    MissingDependency,
}

impl MutationRejectionReason {
    pub fn is_transient(self) -> bool {
        matches!(self, Self::TimestampTooFarFuture | Self::MissingDependency)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PullStateResponse {
    pub mutations: Vec<ServerMutation>,
    pub next_cursor: SyncCursor,
    pub has_more: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PullStateQuery {
    pub library_id: LibraryId,
    pub replica_id: ReplicaId,
    #[serde(default)]
    pub cursor: SyncCursor,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SyncExchangeRequest {
    pub library_id: LibraryId,
    pub replica_id: ReplicaId,
    #[serde(default)]
    pub mutations: Vec<WireMutation>,
    #[serde(default)]
    pub cursor: SyncCursor,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SyncExchangeResponse {
    pub push: PushMutationsResponse,
    pub pull: PullStateResponse,
}

/// The stable identity of one independently merged synchronization register.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct StateCell {
    pub kind: String,
    pub entity_key: String,
    #[serde(default)]
    pub entity_subkey: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StateInventoryRequest {
    pub library_id: LibraryId,
    pub cells: Vec<StateCell>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StateInventoryResponse {
    pub missing: Vec<StateCell>,
}

/// Inventory is deliberately paged independently of mutation exchange so an
/// open library never needs to build or transmit one unbounded request.
pub const MAX_STATE_INVENTORY_CELLS: usize = 1_000;

fn deserialize_library_cursor<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<LibraryRevision>, D::Error> {
    match Option::<u64>::deserialize(deserializer)? {
        None | Some(0) => Ok(None),
        Some(value) => LibraryRevision::new(value).map(Some).map_err(serde::de::Error::custom),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_identifiers_use_their_canonical_strings_in_json_and_bincode_storage() {
        let content_hash = ContentHash::new(&"a".repeat(64));
        let mutation_id = MutationId::parse("12345678-1234-5678-9234-567812345678").unwrap();

        assert_eq!(serde_json::to_value(&content_hash).unwrap(), serde_json::json!(content_hash.as_str()));
        assert_eq!(serde_json::to_value(mutation_id).unwrap(), serde_json::json!(mutation_id.to_string()));

        assert_eq!(wire::encode(&content_hash).unwrap(), wire::encode(&content_hash.as_str()).unwrap());
        assert_eq!(wire::encode(&mutation_id).unwrap(), wire::encode(&mutation_id.to_string()).unwrap());
    }

    #[test]
    fn transport_and_storage_codecs_are_not_interchangeable() {
        let value = ContentHash::new(&"a".repeat(64));
        let stored = wire::encode(&value).unwrap();
        let transported = transport::encode(&value).unwrap();

        assert!(stored.starts_with(b"BKHM"));
        assert!(!transported.starts_with(b"BKHM"));
        assert!(wire::decode::<ContentHash>(&transported, 1024).is_err());
        assert!(transport::decode::<ContentHash>(&stored, 1024).is_err());
    }

    #[test]
    fn wire_mutation_preserves_an_absent_blob_reference_in_binary_encoding() {
        let mutation = WireMutation {
            mutation_id: MutationId::new(),
            kind: "directory_name".to_owned(),
            entity_key: "shelf".to_owned(),
            entity_subkey: String::new(),
            value: Vec::new(),
            conflict_rank: 0,
            blob_reference: None,
            changed_at: 1,
            replica_seq: ReplicaSeq::new(1).unwrap(),
        };
        let encoded = wire::encode(&mutation).unwrap();
        assert_eq!(wire::decode::<WireMutation>(&encoded, wire::MAX_DECODED_REQUEST_BYTES).unwrap(), mutation);
    }
}
