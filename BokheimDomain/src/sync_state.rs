use std::collections::HashMap;
use sync_common::{ClientId, DeclaredBlobReference, DirId, FileName, MutationId, ReplicaSeq, UnixMillis, WireMutation};
use sync_common::wire;
use serde::{Deserialize, Serialize};
use crate::{AnnotationState, BookmarkState, BookLifecycleState, BookLinkState, BookMetadataState, ReadingPositionState};
use bokheim_identifiers::ContentHash;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StateKey {
    DirectoryName(DirId),
    DirectoryParent(DirId),
    DirectoryLifecycle(DirId),
    BookLifecycle(ContentHash),
    Placement { content_hash: ContentHash, dir_id: DirId },
    ReadingPosition(ContentHash),
    Annotation(String),
    Bookmark(String),
    BookLink(String),
    Metadata(ContentHash),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MutationBody {
    DirectoryName { dir_id: DirId, name: FileName },
    DirectoryParent { dir_id: DirId, parent_id: DirId },
    DirectoryLifecycle { dir_id: DirId, deleted: bool },
    BookLifecycle { content_hash: ContentHash, value: BookLifecycleState },
    Placement { dir_id: DirId, content_hash: ContentHash, present: bool },
    ReadingPosition { content_hash: ContentHash, value: ReadingPositionState },
    Annotation { annotation_id: String, value: AnnotationState },
    Bookmark { bookmark_id: String, value: BookmarkState },
    BookLink { link_id: String, value: BookLinkState },
    Metadata { content_hash: ContentHash, value: BookMetadataState },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StateMutation {
    /// Stable retry/idempotency key used for conditional outbox acknowledgement.
    pub mutation_id: MutationId,
    pub body: MutationBody,
    pub changed_at: UnixMillis,
    pub replica_seq: ReplicaSeq,
}

/// Reduces pending mutations produced by one replica without changing their
/// observable server-side meaning.
///
/// Ordinary state domains are unconditional registers and safely collapse to
/// their greatest local version.
pub fn reduce_same_replica_mutations<'a>(mutations: impl IntoIterator<Item = &'a StateMutation>) -> Vec<&'a StateMutation> {
    let mut registers = HashMap::<StateKey, &'a StateMutation>::new();
    let mut reduced = Vec::new();
    for mutation in mutations {
        let key = mutation.body.state_key();
        match registers.get(&key) {
            Some(current) if !same_replica_candidate_wins(current, mutation) => {}
            _ => {
                registers.insert(key, mutation);
            }
        }
    }
    reduced.extend(registers.into_values());
    reduced.sort_by_key(|mutation| (mutation.replica_seq, mutation.mutation_id));
    reduced
}

fn same_replica_candidate_wins(current: &StateMutation, candidate: &StateMutation) -> bool {
    candidate.body.state_key() == current.body.state_key()
        && (candidate.changed_at, candidate.body.conflict_rank(), candidate.replica_seq, candidate.mutation_id) > (current.changed_at, current.body.conflict_rank(), current.replica_seq, current.mutation_id)
}

/// Total conflict-resolution key shared by clients and the server.
///
/// `changed_at` supplies the user-facing last-writer-wins order.
/// `conflict_rank` makes destructive state win an exact timestamp tie, while
/// replica identity, sequence, and mutation identity make all remaining ties
/// deterministic regardless of delivery order.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct VersionKey {
    pub changed_at: UnixMillis,
    pub conflict_rank: u8,
    pub replica_id: ClientId,
    pub replica_seq: u64,
    pub mutation_id: MutationId,
}

impl VersionKey {
    pub fn new(changed_at: UnixMillis, conflict_rank: u8, replica_id: ClientId, replica_seq: u64, mutation_id: MutationId) -> Self {
        Self { changed_at, conflict_rank, replica_id, replica_seq, mutation_id }
    }
}

impl MutationBody {
    /// Stable, open wire kind. Every producer obtains its kind here so a
    /// misspelled literal cannot create an unreachable register.
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::DirectoryName { .. } => "directory_name",
            Self::DirectoryParent { .. } => "directory_parent",
            Self::DirectoryLifecycle { .. } => "directory_lifecycle",
            Self::BookLifecycle { .. } => "book_lifecycle",
            Self::Placement { .. } => "placement",
            Self::ReadingPosition { .. } => "reading_position",
            Self::Annotation { .. } => "annotation",
            Self::Bookmark { .. } => "bookmark",
            Self::BookLink { .. } => "book_link",
            Self::Metadata { .. } => "metadata",
        }
    }

    /// Encodes the domain value into the generic wire envelope.
    pub fn to_wire(&self, mutation_id: MutationId, changed_at: UnixMillis, replica_seq: ReplicaSeq) -> Result<WireMutation, wire::WireError> {
        let (entity_key, entity_subkey, value, blob_reference) = match self {
            Self::DirectoryName { dir_id, name } => (dir_id.as_str().to_owned(), String::new(), wire::encode(name)?, None),
            Self::DirectoryParent { dir_id, parent_id } => (dir_id.as_str().to_owned(), String::new(), wire::encode(parent_id)?, None),
            Self::DirectoryLifecycle { dir_id, deleted } => (dir_id.as_str().to_owned(), String::new(), wire::encode(deleted)?, None),
            Self::BookLifecycle { content_hash, value } => {
                let blob_reference = match value {
                    BookLifecycleState::Present { .. } => DeclaredBlobReference { present: true, content_hash: Some(content_hash.clone()) },
                    BookLifecycleState::Deleted => DeclaredBlobReference { present: false, content_hash: None },
                };
                (content_hash.to_string(), String::new(), wire::encode(value)?, Some(blob_reference))
            }
            Self::Placement { dir_id, content_hash, present } => (content_hash.to_string(), dir_id.as_str().to_owned(), wire::encode(present)?, None),
            Self::ReadingPosition { content_hash, value } => (content_hash.to_string(), String::new(), wire::encode(value)?, None),
            Self::Annotation { annotation_id, value } => (annotation_id.clone(), String::new(), wire::encode(value)?, None),
            Self::Bookmark { bookmark_id, value } => (bookmark_id.clone(), String::new(), wire::encode(value)?, None),
            Self::BookLink { link_id, value } => (link_id.clone(), String::new(), wire::encode(value)?, None),
            Self::Metadata { content_hash, value } => (content_hash.to_string(), String::new(), wire::encode(value)?, None),
        };
        Ok(WireMutation { mutation_id, kind: self.kind().to_owned(), entity_key, entity_subkey, value, conflict_rank: self.conflict_rank(), blob_reference, changed_at, replica_seq })
    }

    /// Decodes a recognized wire value. Unknown kinds are intentionally
    /// forward-compatible and return `None` without interpreting their bytes.
    pub fn from_wire(wire_mutation: &WireMutation) -> Result<Option<Self>, wire::WireError> {
        let key = &wire_mutation.entity_key;
        let value = &wire_mutation.value;
        let body = match wire_mutation.kind.as_str() {
            "directory_name" => Self::DirectoryName { dir_id: DirId::parse(key).map_err(|error| wire::WireError::Decode(error.to_string()))?, name: wire::decode(value, wire::MAX_DECODED_REQUEST_BYTES)? },
            "directory_parent" => Self::DirectoryParent { dir_id: DirId::parse(key).map_err(|error| wire::WireError::Decode(error.to_string()))?, parent_id: wire::decode(value, wire::MAX_DECODED_REQUEST_BYTES)? },
            "directory_lifecycle" => Self::DirectoryLifecycle { dir_id: DirId::parse(key).map_err(|error| wire::WireError::Decode(error.to_string()))?, deleted: wire::decode(value, wire::MAX_DECODED_REQUEST_BYTES)? },
            "book_lifecycle" => Self::BookLifecycle { content_hash: ContentHash::new(&key.as_str()), value: wire::decode(value, wire::MAX_DECODED_REQUEST_BYTES)? },
            "placement" => Self::Placement { content_hash: ContentHash::new(&key.as_str()), dir_id: DirId::parse(&wire_mutation.entity_subkey).map_err(|error| wire::WireError::Decode(error.to_string()))?, present: wire::decode(value, wire::MAX_DECODED_REQUEST_BYTES)? },
            "reading_position" => Self::ReadingPosition { content_hash: ContentHash::new(&key.as_str()), value: wire::decode(value, wire::MAX_DECODED_REQUEST_BYTES)? },
            "annotation" => Self::Annotation { annotation_id: key.clone(), value: wire::decode(value, wire::MAX_DECODED_REQUEST_BYTES)? },
            "bookmark" => Self::Bookmark { bookmark_id: key.clone(), value: wire::decode(value, wire::MAX_DECODED_REQUEST_BYTES)? },
            "book_link" => Self::BookLink { link_id: key.clone(), value: wire::decode(value, wire::MAX_DECODED_REQUEST_BYTES)? },
            "metadata" => Self::Metadata { content_hash: ContentHash::new(&key.as_str()), value: wire::decode(value, wire::MAX_DECODED_REQUEST_BYTES)? },
            _ => return Ok(None),
        };
        Ok(Some(body))
    }
    /// Every field of every variant is bound here (payload fields discarded
    /// as `_` by name) rather than skipped with `..`, so adding a new field
    /// to any `MutationBody` variant fails to compile here until someone
    /// decides whether it belongs in the cell's identity or not.
    pub fn state_key(&self) -> StateKey {
        match self {
            Self::DirectoryName { dir_id, name: _ } => StateKey::DirectoryName(dir_id.clone()),
        Self::DirectoryParent { dir_id, parent_id: _ } => StateKey::DirectoryParent(dir_id.clone()),
        Self::DirectoryLifecycle { dir_id, deleted: _ } => StateKey::DirectoryLifecycle(dir_id.clone()),
        Self::BookLifecycle { content_hash, value: _ } => StateKey::BookLifecycle(content_hash.clone()),
        Self::Placement { dir_id, content_hash, present: _ } => StateKey::Placement { content_hash: content_hash.clone(), dir_id: dir_id.clone() },
        Self::ReadingPosition { content_hash, value: _ } => StateKey::ReadingPosition(content_hash.clone()),
            Self::Annotation { annotation_id, value: _ } => StateKey::Annotation(annotation_id.clone()),
            Self::Bookmark { bookmark_id, value: _ } => StateKey::Bookmark(bookmark_id.clone()),
            Self::BookLink { link_id, value: _ } => StateKey::BookLink(link_id.clone()),
            Self::Metadata { content_hash, value: _ } => StateKey::Metadata(content_hash.clone()),
        }
    }

    /// Rank used inside [`VersionKey`]. Destructive updates in unconditional
    /// registers win an exact timestamp tie.
    pub fn conflict_rank(&self) -> u8 {
        match self {
            Self::Placement { present: false, .. } => 1,
            Self::DirectoryLifecycle { deleted: true, .. }
            | Self::BookLifecycle { value: BookLifecycleState::Deleted, .. }
            | Self::Annotation { value: AnnotationState { deleted: true, .. }, .. }
            | Self::Bookmark { value: BookmarkState { deleted: true, .. }, .. }
            | Self::BookLink { value: BookLinkState { deleted: true, .. }, .. } => 1,
            _ => 0,
        }
    }

}

impl StateMutation {
    pub fn to_wire(&self) -> Result<WireMutation, wire::WireError> {
        self.body.to_wire(self.mutation_id, self.changed_at, self.replica_seq)
    }

    pub fn from_wire(wire_mutation: &WireMutation) -> Result<Option<Self>, wire::WireError> {
        Ok(MutationBody::from_wire(wire_mutation)?.map(|body| Self {
            mutation_id: wire_mutation.mutation_id,
            body,
            changed_at: wire_mutation.changed_at,
            replica_seq: wire_mutation.replica_seq,
        }))
    }

}
