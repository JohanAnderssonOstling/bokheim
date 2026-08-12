//! Client-replica interpretation of opaque sync payloads.
//!
//! The sync service depends only on `sync-common` and never links this crate.

use catalog_contract::{CatalogPublicationIdentity, CatalogSourceId};
use library_model::{AnnotationState, BookLifecycleState, BookLinkState, BookMetadataState, BookmarkState, DirectoryLifecycleState, ReadingPositionState};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use sync_common::wire;
use sync_common::ContentHash;
use sync_common::{DeclaredBlobReference, DirId, FileName, MutationId, ReplicaId, ReplicaSeq, UnixMillis, WireMutation};

mod directory_projection;
pub use directory_projection::{project_directories, DirectoryIntent, DirectoryProjection};

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
    CatalogPublication { source_id: String, publication_key: String },
    Metadata(ContentHash),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MutationBody {
    DirectoryName { dir_id: DirId, name: FileName },
    DirectoryParent { dir_id: DirId, parent_id: DirId },
    DirectoryLifecycle { dir_id: DirId, value: DirectoryLifecycleState },
    BookLifecycle { content_hash: ContentHash, value: BookLifecycleState },
    Placement { dir_id: DirId, content_hash: ContentHash, present: bool, origin_folder_id: Option<String> },
    ReadingPosition { content_hash: ContentHash, value: ReadingPositionState },
    Annotation { annotation_id: String, value: AnnotationState },
    Bookmark { bookmark_id: String, value: BookmarkState },
    BookLink { link_id: String, value: BookLinkState },
    CatalogPublication { source_id: CatalogSourceId, publication: CatalogPublicationIdentity, content_hash: ContentHash },
    Metadata { content_hash: ContentHash, value: BookMetadataState },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct CatalogPublicationValue {
    publication: CatalogPublicationIdentity,
    content_hash: ContentHash,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct PlacementValue {
    present: bool,
    origin_folder_id: Option<String>,
}

/// Collision-free register subkey for an edition/work identity. The edition's
/// UTF-8 byte length makes the boundary unambiguous without constraining OPDS
/// identifiers or storing provider URLs.
pub fn catalog_publication_state_subkey(edition_id: &str, work_id: Option<&str>) -> String {
    format!("{}:{edition_id}{}", edition_id.len(), work_id.unwrap_or_default())
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

/// Reduces unsent local mutations to one canonical winner per state cell.
pub fn coalesce_mutations_for_push(mutations: Vec<StateMutation>) -> Vec<StateMutation> {
    reduce_same_replica_mutations(&mutations).into_iter().cloned().collect()
}

fn same_replica_candidate_wins(current: &StateMutation, candidate: &StateMutation) -> bool {
    candidate.body.state_key() == current.body.state_key()
        && (candidate.changed_at, candidate.body.conflict_rank(), candidate.replica_seq, candidate.mutation_id) > (current.changed_at, current.body.conflict_rank(), current.replica_seq, current.mutation_id)
}

/// Total conflict-resolution key used by library replicas.
///
/// `changed_at` supplies the user-facing last-writer-wins order.
/// `conflict_rank` makes destructive state win an exact timestamp tie, while
/// replica identity, sequence, and mutation identity make all remaining ties
/// deterministic regardless of delivery order. The service mirrors this order
/// in storage without linking this client-side type.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct VersionKey {
    pub changed_at: UnixMillis,
    pub conflict_rank: u8,
    pub replica_id: ReplicaId,
    pub replica_seq: u64,
    pub mutation_id: MutationId,
}

impl VersionKey {
    pub fn new(changed_at: UnixMillis, conflict_rank: u8, replica_id: ReplicaId, replica_seq: u64, mutation_id: MutationId) -> Self {
        Self { changed_at, conflict_rank, replica_id, replica_seq, mutation_id }
    }
}

impl MutationBody {
    /// Stable, open wire kind. Every producer obtains its kind here so a
    /// misspelled literal cannot create an unreachable register.
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::DirectoryName { .. } => sync_common::mutation_kind::DIRECTORY_NAME,
            Self::DirectoryParent { .. } => sync_common::mutation_kind::DIRECTORY_PARENT,
            Self::DirectoryLifecycle { .. } => sync_common::mutation_kind::DIRECTORY_LIFECYCLE,
            Self::BookLifecycle { .. } => sync_common::mutation_kind::BOOK_LIFECYCLE,
            Self::Placement { .. } => sync_common::mutation_kind::PLACEMENT,
            Self::ReadingPosition { .. } => sync_common::mutation_kind::READING_POSITION,
            Self::Annotation { .. } => sync_common::mutation_kind::ANNOTATION,
            Self::Bookmark { .. } => sync_common::mutation_kind::BOOKMARK,
            Self::BookLink { .. } => sync_common::mutation_kind::BOOK_LINK,
            Self::CatalogPublication { .. } => sync_common::mutation_kind::CATALOG_PUBLICATION,
            Self::Metadata { .. } => sync_common::mutation_kind::METADATA,
        }
    }

    /// Encodes the domain value into the generic wire envelope.
    pub fn to_wire(&self, mutation_id: MutationId, changed_at: UnixMillis, replica_seq: ReplicaSeq) -> Result<WireMutation, wire::WireError> {
        let (entity_key, entity_subkey, value, blob_reference) = match self {
            Self::DirectoryName { dir_id, name } => (dir_id.to_string(), String::new(), wire::encode(name)?, None),
            Self::DirectoryParent { dir_id, parent_id } => (dir_id.to_string(), String::new(), wire::encode(parent_id)?, None),
            Self::DirectoryLifecycle { dir_id, value } => (dir_id.to_string(), String::new(), wire::encode(value)?, None),
            Self::BookLifecycle { content_hash, value } => {
                let blob_reference = match value {
                    BookLifecycleState::Present { .. } | BookLifecycleState::Deleted { .. } => DeclaredBlobReference { present: true, content_hash: Some(content_hash.clone()) },
                    BookLifecycleState::Purged => DeclaredBlobReference { present: false, content_hash: None },
                };
                (content_hash.to_string(), String::new(), wire::encode(value)?, Some(blob_reference))
            }
            Self::Placement { dir_id, content_hash, present, origin_folder_id } => (content_hash.to_string(), dir_id.to_string(), wire::encode(&PlacementValue { present: *present, origin_folder_id: origin_folder_id.clone() })?, None),
            Self::ReadingPosition { content_hash, value } => (content_hash.to_string(), String::new(), wire::encode(value)?, None),
            Self::Annotation { annotation_id, value } => (annotation_id.clone(), String::new(), wire::encode(value)?, None),
            Self::Bookmark { bookmark_id, value } => (bookmark_id.clone(), String::new(), wire::encode(value)?, None),
            Self::BookLink { link_id, value } => (link_id.clone(), String::new(), wire::encode(value)?, None),
            Self::CatalogPublication { source_id, publication, content_hash } => {
                let value = CatalogPublicationValue { publication: publication.clone(), content_hash: content_hash.clone() };
                (source_id.as_str().to_owned(), catalog_publication_state_subkey(&publication.edition_id, publication.work_id.as_deref()), wire::encode(&value)?, None)
            }
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
            sync_common::mutation_kind::DIRECTORY_NAME => Self::DirectoryName { dir_id: parse_dir_id(key)?, name: wire::decode(value, wire::MAX_DECODED_REQUEST_BYTES)? },
            sync_common::mutation_kind::DIRECTORY_PARENT => Self::DirectoryParent { dir_id: parse_dir_id(key)?, parent_id: wire::decode(value, wire::MAX_DECODED_REQUEST_BYTES)? },
            sync_common::mutation_kind::DIRECTORY_LIFECYCLE => Self::DirectoryLifecycle { dir_id: parse_dir_id(key)?, value: wire::decode(value, wire::MAX_DECODED_REQUEST_BYTES)? },
            sync_common::mutation_kind::BOOK_LIFECYCLE => Self::BookLifecycle { content_hash: ContentHash::new(&key.as_str()), value: wire::decode(value, wire::MAX_DECODED_REQUEST_BYTES)? },
            sync_common::mutation_kind::PLACEMENT => {
                let value: PlacementValue = wire::decode(value, wire::MAX_DECODED_REQUEST_BYTES)?;
                Self::Placement { content_hash: ContentHash::new(key.as_str()), dir_id: parse_dir_id(&wire_mutation.entity_subkey)?, present: value.present, origin_folder_id: value.origin_folder_id }
            }
            sync_common::mutation_kind::READING_POSITION => Self::ReadingPosition { content_hash: ContentHash::new(&key.as_str()), value: wire::decode(value, wire::MAX_DECODED_REQUEST_BYTES)? },
            sync_common::mutation_kind::ANNOTATION => Self::Annotation { annotation_id: key.clone(), value: wire::decode(value, wire::MAX_DECODED_REQUEST_BYTES)? },
            sync_common::mutation_kind::BOOKMARK => Self::Bookmark { bookmark_id: key.clone(), value: wire::decode(value, wire::MAX_DECODED_REQUEST_BYTES)? },
            sync_common::mutation_kind::BOOK_LINK => Self::BookLink { link_id: key.clone(), value: wire::decode(value, wire::MAX_DECODED_REQUEST_BYTES)? },
            sync_common::mutation_kind::CATALOG_PUBLICATION => {
                let decoded: CatalogPublicationValue = wire::decode(value, wire::MAX_DECODED_REQUEST_BYTES)?;
                let source_id = CatalogSourceId::parse(key).map_err(wire::WireError::Decode)?;
                let expected_subkey = catalog_publication_state_subkey(&decoded.publication.edition_id, decoded.publication.work_id.as_deref());
                if wire_mutation.entity_subkey != expected_subkey {
                    return Err(wire::WireError::Decode("catalog publication identity does not match its register subkey".to_owned()));
                }
                Self::CatalogPublication { source_id, publication: decoded.publication, content_hash: decoded.content_hash }
            }
            sync_common::mutation_kind::METADATA => Self::Metadata { content_hash: ContentHash::new(&key.as_str()), value: wire::decode(value, wire::MAX_DECODED_REQUEST_BYTES)? },
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
            Self::DirectoryLifecycle { dir_id, value: _ } => StateKey::DirectoryLifecycle(dir_id.clone()),
            Self::BookLifecycle { content_hash, value: _ } => StateKey::BookLifecycle(content_hash.clone()),
            Self::Placement { dir_id, content_hash, .. } => StateKey::Placement { content_hash: content_hash.clone(), dir_id: dir_id.clone() },
            Self::ReadingPosition { content_hash, value: _ } => StateKey::ReadingPosition(content_hash.clone()),
            Self::Annotation { annotation_id, value: _ } => StateKey::Annotation(annotation_id.clone()),
            Self::Bookmark { bookmark_id, value: _ } => StateKey::Bookmark(bookmark_id.clone()),
            Self::BookLink { link_id, value: _ } => StateKey::BookLink(link_id.clone()),
            Self::CatalogPublication { source_id, publication, content_hash: _ } => {
                StateKey::CatalogPublication { source_id: source_id.as_str().to_owned(), publication_key: catalog_publication_state_subkey(&publication.edition_id, publication.work_id.as_deref()) }
            }
            Self::Metadata { content_hash, value: _ } => StateKey::Metadata(content_hash.clone()),
        }
    }

    /// Rank used inside [`VersionKey`]. Destructive updates in unconditional
    /// registers win an exact timestamp tie.
    pub fn conflict_rank(&self) -> u8 {
        match self {
            Self::Placement { present: false, .. } => 1,
            Self::DirectoryLifecycle { value: DirectoryLifecycleState::Deleted, .. }
            | Self::BookLifecycle { value: BookLifecycleState::Deleted { .. }, .. }
            | Self::Annotation { value: AnnotationState { deleted: true, .. }, .. }
            | Self::Bookmark { value: BookmarkState { deleted: true, .. }, .. }
            | Self::BookLink { value: BookLinkState { deleted: true, .. }, .. } => 1,
            Self::DirectoryLifecycle { value: DirectoryLifecycleState::Purged, .. } | Self::BookLifecycle { value: BookLifecycleState::Purged, .. } => 2,
            _ => 0,
        }
    }
}

fn parse_dir_id(value: &str) -> Result<DirId, wire::WireError> {
    DirId::parse_str(value).map_err(|error| wire::WireError::Decode(error.to_string()))
}

impl StateMutation {
    pub fn to_wire(&self) -> Result<WireMutation, wire::WireError> {
        self.body.to_wire(self.mutation_id, self.changed_at, self.replica_seq)
    }

    pub fn from_wire(wire_mutation: &WireMutation) -> Result<Option<Self>, wire::WireError> {
        Ok(MutationBody::from_wire(wire_mutation)?.map(|body| Self { mutation_id: wire_mutation.mutation_id, body, changed_at: wire_mutation.changed_at, replica_seq: wire_mutation.replica_seq }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use library_model::{AnnotationAnchor, AnnotationStyle, BookFormat, ReadingPosition};

    fn content_hash(character: char) -> ContentHash {
        ContentHash::new(&character.to_string().repeat(64))
    }

    fn dir_id(value: &str) -> DirId {
        DirId::parse_str(value).unwrap()
    }

    fn metadata() -> BookMetadataState {
        BookMetadataState { title: "Book".to_owned(), contributors: Vec::new(), description: String::new(), publication: Default::default() }
    }

    #[test]
    fn every_library_payload_round_trips_through_the_opaque_protocol_envelope() {
        let first_dir = dir_id("11111111-1111-4111-8111-111111111111");
        let second_dir = dir_id("22222222-2222-4222-8222-222222222222");
        let first_hash = content_hash('a');
        let second_hash = content_hash('b');
        let bodies = vec![
            MutationBody::DirectoryName { dir_id: first_dir, name: "Shelf".to_owned() },
            MutationBody::DirectoryParent { dir_id: first_dir, parent_id: second_dir },
            MutationBody::DirectoryLifecycle { dir_id: first_dir, value: DirectoryLifecycleState::Present },
            MutationBody::BookLifecycle { content_hash: first_hash, value: BookLifecycleState::Present { metadata: metadata(), added_at: 1, format: BookFormat::Epub } },
            MutationBody::Placement { dir_id: first_dir, content_hash: first_hash, present: true, origin_folder_id: None },
            MutationBody::ReadingPosition { content_hash: first_hash, value: ReadingPositionState { location: ReadingPosition::parse("epubcfi(/6/2)").unwrap(), progress: 0.5 } },
            MutationBody::Annotation {
                annotation_id: "annotation".to_owned(),
                value: AnnotationState {
                    content_hash: first_hash,
                    anchor: AnnotationAnchor::epub_cfi("epubcfi(/6/2!/4/1:0)"),
                    exact_text: "Text".to_owned(),
                    style: AnnotationStyle::Highlight,
                    color: "#ffee00".to_owned(),
                    note: String::new(),
                    created_at: 1,
                    modified_at: 2,
                    deleted: false,
                },
            },
            MutationBody::Bookmark { bookmark_id: "bookmark".to_owned(), value: BookmarkState { content_hash: first_hash, cfi: "epubcfi(/6/2)".to_owned(), label: "Start".to_owned(), created_at: 1, modified_at: 2, deleted: false } },
            MutationBody::BookLink {
                link_id: "link".to_owned(),
                value: BookLinkState { source_content_hash: first_hash, target_content_hash: second_hash, relation: "references".to_owned(), note: String::new(), created_at: 1, modified_at: 2, deleted: false },
            },
            MutationBody::CatalogPublication {
                source_id: CatalogSourceId::parse("standard-ebooks").unwrap(),
                publication: CatalogPublicationIdentity { edition_id: "urn:uuid:edition".to_owned(), work_id: Some("urn:uuid:work".to_owned()) },
                content_hash: first_hash,
            },
            MutationBody::Metadata { content_hash: first_hash, value: metadata() },
        ];

        for (index, body) in bodies.into_iter().enumerate() {
            let mutation = StateMutation { mutation_id: MutationId::new(), body, changed_at: 100 + index as u64, replica_seq: ReplicaSeq::new(index as u64 + 1).unwrap() };
            let wire = mutation.to_wire().unwrap();
            assert_eq!(wire.kind, mutation.body.kind());
            assert_eq!(StateMutation::from_wire(&wire).unwrap(), Some(mutation));
        }
    }

    #[test]
    fn unknown_service_payloads_remain_forward_compatible() {
        let wire = WireMutation {
            mutation_id: MutationId::new(),
            kind: "future_kind".to_owned(),
            entity_key: String::new(),
            entity_subkey: String::new(),
            value: Vec::new(),
            conflict_rank: 0,
            blob_reference: None,
            changed_at: 1,
            replica_seq: ReplicaSeq::new(1).unwrap(),
        };
        assert_eq!(StateMutation::from_wire(&wire).unwrap(), None);
    }

    #[test]
    fn placement_updates_coalesce_to_the_latest_register_value() {
        let dir_id = dir_id("11111111-1111-4111-8111-111111111111");
        let claim = StateMutation { mutation_id: MutationId::new(), body: MutationBody::Placement { dir_id, content_hash: content_hash('a'), present: true, origin_folder_id: None }, changed_at: 1, replica_seq: ReplicaSeq::new(1).unwrap() };
        let release =
            StateMutation { mutation_id: MutationId::new(), body: MutationBody::Placement { dir_id, content_hash: content_hash('a'), present: false, origin_folder_id: None }, changed_at: 2, replica_seq: ReplicaSeq::new(2).unwrap() };
        let reduced = coalesce_mutations_for_push(vec![claim, release]);
        assert!(matches!(reduced.as_slice(), [StateMutation { body: MutationBody::Placement { present: false, .. }, .. }]));
    }
}
