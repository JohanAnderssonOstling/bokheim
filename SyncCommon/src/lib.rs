use serde::{Deserialize, Serialize};
use std::fmt;
use std::num::NonZeroU64;

pub mod api;
pub use bokheim_wire as wire;
pub mod mutation_ids;
pub use mutation_ids::{MutationId, MutationIdError};

pub use bokheim_identifiers::{
    AgentId, AgentIdError, AuthorId, AuthorIdError, BookId, ClientId, ContentHash,
    ContentHashParseError, ContributorId, DirId, DomainTextError, FileName, LibraryId,
    PublisherId, TimestampError, UnixMillis,
};
pub use bokheim_algorithms::{
    audiobook_position_millis, audiobook_reading_position, pdf_page_from_reading_position,
    pdf_page_reading_position, valid_epub_cfi, valid_reading_position, Contributor,
    LanguageTag, MarcRelatorCode, PublicationCollection, PublicationDate,
    PublicationIdentifier, PublicationIdentifierScheme, PublicationIdentifierType,
    PublicationMetadata, PublicationMetadataError, PublicationProperty, PublicationSubject,
    PublicationTitle, PublisherCredit, PublisherMatchKey, PublisherName, ReadingPosition,
};

/// Maximum encoded size of a synchronized JPEG cover.
pub const MAX_THUMBNAIL_BYTES: u64 = 4 * 1024 * 1024;
/// Maximum encoded size of a synchronized book asset.
pub const MAX_BOOK_BYTES: u64 = 1_000_000_000;
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

            pub const fn get(self) -> u64 { self.0.get() }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result { self.get().fmt(formatter) }
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
    pub replica_id: ClientId,
    pub revision: LibraryRevision,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PushMutationsRequest {
    pub library_id: LibraryId,
    pub replica_id: ClientId,
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
    pub next_cursor: Option<LibraryRevision>,
    pub has_more: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PullStateQuery {
    pub library_id: LibraryId,
    pub replica_id: ClientId,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "deserialize_library_cursor")]
    pub cursor: Option<LibraryRevision>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SyncExchangeRequest {
    pub library_id: LibraryId,
    pub replica_id: ClientId,
    #[serde(default)]
    pub mutations: Vec<WireMutation>,
    #[serde(default, deserialize_with = "deserialize_library_cursor")]
    pub cursor: Option<LibraryRevision>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SyncExchangeResponse {
    pub push: PushMutationsResponse,
    pub pull: PullStateResponse,
}

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
    fn wire_mutation_preserves_an_absent_blob_reference_in_binary_encoding() {
        let mutation = WireMutation {
            mutation_id: MutationId::new(),
            kind: "directory_name".to_owned(),
            entity_key: "shelf".to_owned(),
            entity_subkey: String::new(),
            value: Vec::new(),
            conflict_rank: 0,
            blob_reference: None,
            changed_at: UnixMillis::new(1).unwrap(),
            replica_seq: ReplicaSeq::new(1).unwrap(),
        };
        let encoded = wire::encode(&mutation).unwrap();
        assert_eq!(wire::decode::<WireMutation>(&encoded, wire::MAX_DECODED_REQUEST_BYTES).unwrap(), mutation);
    }
}
