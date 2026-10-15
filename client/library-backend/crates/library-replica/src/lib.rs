//! Client-replica interpretation of opaque sync payloads.
//!
//! The sync service depends only on `sync-common` and never links this crate.

pub use book_model::BookDetails;
use book_model::{Annotation, BookRecord, BookTocDocument};
use pdf_view_common::PdfReaderMetadata;
use std::sync::OnceLock;
pub mod filenames;
mod folder_management;
mod folder_names;
pub use folder_names::project_folder_name;
mod enrichment;
mod library_state;
pub use enrichment::EnrichmentResult;

pub use folder_management::{file_name_candidates, folder_name_candidates, numbered_file_name, numbered_folder_name, unique_file_name, unique_folder_name};
pub use library_state::{portable_name_key, BookFacts, BookLifecycleState, DirectoryLifecycleState, ReadingPositionState};

use serde::{Deserialize, Serialize};

use std::collections::{hash_map::Entry, HashMap};
use sync_common::ContentHash;
pub use sync_common::VersionKey;
use sync_common::{ConflictRank, DirId, FileName, OpaqueStateEntry, Publication, ReplicaId, ReplicaSeq, StateAddress, StatePayload, UnixMillis, Versioned};

mod directory_projection;
pub(crate) mod serde_enum;
pub use directory_projection::{project_directories, DirectoryIntent, DirectoryProjection};

/// The complete, immutable facts admitted when a file is added to a library.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BookAddition {
    pub facts: BookFacts,
    pub record: BookRecord,
    pub navigation: Option<BookTocDocument>,
    pub pdf: Option<PdfReaderMetadata>,
    pub recording: Option<metadata_contract::audible::LookupRequest>,
    /// Absent only in a device-local draft. Every publication has a definitive answer.
    pub enrichment: Option<EnrichmentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum LibraryStateEntry {
    DirectoryName { dir_id: DirId, name: FileName },
    DirectoryParent { dir_id: DirId, parent_id: DirId },
    DirectoryLifecycle { dir_id: DirId, value: DirectoryLifecycleState },
    BookLifecycle { content_hash: ContentHash, value: BookLifecycleState },
    Placement { dir_id: DirId, content_hash: ContentHash, present: bool, origin_folder_id: Option<String> },
    ReadingPosition { content_hash: ContentHash, value: ReadingPositionState },
    Annotation { value: Annotation },
    BookAdd { content_hash: ContentHash, value: BookAddition },
}

/// A publication uses the same envelope whether its state is decoded or opaque.
pub type StatePublication = Publication<LibraryStateEntry>;

/// Coalesce state by its original winning version, independently of the
/// publication's retry ID, publishing replica, or queue sequence.
pub fn coalesce_mutations_for_push(publications: Vec<StatePublication>) -> Vec<StatePublication> {
    let mut entries = HashMap::<StateAddress, StatePublication>::new();
    for publication in publications {
        let address = publication.state.value.address();
        match entries.entry(address) {
            Entry::Occupied(mut current) => {
                if current.get().state.version < publication.state.version {
                    current.insert(publication);
                }
            }
            Entry::Vacant(entry) => { entry.insert(publication); }
        }
    }
    let mut reduced = entries.into_values().collect::<Vec<_>>();
    reduced.sort_by_key(|publication| publication.publisher_seq);
    reduced
}

pub fn prepare_push(mut publications: Vec<StatePublication>) -> Result<sync_common::PushBatcher, Box<dyn std::error::Error>> {
    publications.sort_by_key(|publication| !matches!(publication.state.value, LibraryStateEntry::BookLifecycle { .. }));
    Ok(sync_common::PushBatcher::try_from_publications(publications.into_iter().map(|publication| sync_common::transport::representation(&publication)))?)
}

impl LibraryStateEntry {
    /// Book bytes that must be uploaded before this state can be shared.
    /// Removals must remain possible even when a local import never uploaded.
    pub fn upload_dependency(&self) -> Option<sync_common::ContentHash> {
        match self {
            Self::DirectoryName { .. } | Self::DirectoryParent { .. } | Self::DirectoryLifecycle { .. } | Self::BookLifecycle { value: BookLifecycleState::Purged, .. } | Self::Placement { present: false, .. } => None,
            Self::Annotation { value, .. } => (!value.deleted).then_some(value.content_hash),
            Self::BookLifecycle { content_hash, .. } | Self::Placement { content_hash, .. } | Self::ReadingPosition { content_hash, .. } | Self::BookAdd { content_hash, .. } => Some(*content_hash),
        }
    }

    /// The wire kind is the variant name provided by this type's Serde schema.
    pub fn kind(&self) -> &'static str {
        use crate::serde_enum::NameSerializer;
        self.serialize(NameSerializer).expect("state entries must serialize as enum variants")
    }

    /// Identity is projected from the entry, never kept in a parallel enum.
    pub fn address(&self) -> StateAddress {
        let (entity_key, entity_subkey) = match self {
            Self::DirectoryName { dir_id, .. } | Self::DirectoryParent { dir_id, .. } | Self::DirectoryLifecycle { dir_id, .. } => (dir_id.to_string().into(), String::new()),
            Self::Placement { content_hash, dir_id, .. } => ((*content_hash).into(), dir_id.to_string()),
            Self::Annotation { value } => (value.id.clone().into(), String::new()),
            Self::BookLifecycle { content_hash, .. } | Self::ReadingPosition { content_hash, .. } | Self::BookAdd { content_hash, .. } => ((*content_hash).into(), String::new()),
        };
        StateAddress { kind: self.kind().to_owned(), entity_key, entity_subkey }
    }

    /// The book this value belongs to. The service purges and accounts by it.
    pub fn book(&self) -> Option<sync_common::ContentHash> {
        match self {
            Self::DirectoryName { .. } | Self::DirectoryParent { .. } | Self::DirectoryLifecycle { .. } => None,
            Self::Annotation { value, .. } => Some(value.content_hash),
            Self::BookLifecycle { content_hash, .. } | Self::Placement { content_hash, .. } | Self::ReadingPosition { content_hash, .. } | Self::BookAdd { content_hash, .. } => Some(*content_hash),
        }
    }

    /// Construct a new authored state and its initial publication.
    pub fn publication(self, replica_id: ReplicaId, changed_at: UnixMillis, sequence: ReplicaSeq) -> StatePublication {
        let version = VersionKey::new(changed_at, self.conflict_rank(), replica_id, sequence);
        Publication { publisher_seq: sequence, state: Versioned { value: self, version } }
    }

    pub fn decode(state: &Versioned<OpaqueStateEntry>) -> Result<Option<Self>, sync_common::transport::WireError> {
        Ok(sync_common::state_encoding::decode_known::<Self>(state)?.map(|state| state.value))
    }

    pub fn validate(&self) -> Result<(), sync_common::transport::WireError> {
        use sync_common::transport::WireError;
        let invalid = |message: &str| WireError::Decode(message.to_owned());
        match self {
            Self::ReadingPosition { value, .. } => {
                book_model::ReadProgress::new(value.progress).map_err(|error| invalid(&error.to_string()))?;
                if let book_model::ReadingPosition::PdfAtPoint { full_page_position, .. } = value.location {
                    if !full_page_position.is_finite() || full_page_position < 0.0 {
                        return Err(invalid("invalid PDF full-page position"));
                    }
                }
            }
            Self::Annotation { value, .. } => {
                value.validate().map_err(|error| invalid(&error.to_string()))?;
            }
            Self::BookAdd { content_hash, value } => {
                let Some(enrichment) = &value.enrichment else {
                    return Err(invalid("book addition is awaiting enrichment"));
                };
                if let EnrichmentResult::Found { toc: Some(entries), .. } = enrichment {
                    if sync_common::transport::encode(entries)?.len() > 512 * 1024 {
                        return Err(invalid("enriched navigation exceeds the mutation envelope"));
                    }
                }
                if value.facts.added_at > i64::MAX as u64 {
                    return Err(invalid("book addition timestamp exceeds storage range"));
                }
                if value.pdf.as_ref().is_some_and(|pdf| !pdf.valid_for(&content_hash.to_hex())) {
                    return Err(invalid("invalid PDF reader metadata"));
                }
                if let Some(navigation) = &value.navigation {
                    validate_navigation(navigation)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Rank used inside [`VersionKey`]. Destructive updates in unconditional
    /// registers win an exact timestamp tie.
    pub fn conflict_rank(&self) -> ConflictRank {
        match self {
            Self::Placement { present: false, .. } => ConflictRank::Delete,
            Self::DirectoryLifecycle { value: DirectoryLifecycleState::Deleted, .. } | Self::BookLifecycle { value: BookLifecycleState::Deleted { .. }, .. } | Self::Annotation { value: Annotation { deleted: true, .. } } => {
                ConflictRank::Delete
            }
            Self::DirectoryLifecycle { value: DirectoryLifecycleState::Purged, .. } | Self::BookLifecycle { value: BookLifecycleState::Purged, .. } => ConflictRank::Purge,
            _ => ConflictRank::Update,
        }
    }
}

impl StatePayload for LibraryStateEntry {
    fn recognizes_kind(kind: &str) -> bool {
        static VARIANTS: OnceLock<&'static [&'static str]> = OnceLock::new();
        VARIANTS
            .get_or_init(|| match Self::deserialize(serde::de::value::StrDeserializer::<serde_enum::Names>::new("")) {
                Err(serde_enum::Names(names)) if !names.is_empty() => names,
                _ => panic!("state entries must deserialize as named enum variants"),
            })
            .contains(&kind)
    }

    fn serialize_entry<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.validate().map_err(serde::ser::Error::custom)?;
        let entry = OpaqueStateEntry { address: self.address(), book: self.book(), bytes: sync_common::transport::encode(self).map_err(serde::ser::Error::custom)? };
        entry.serialize(serializer)
    }

    fn deserialize_entry<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let entry = OpaqueStateEntry::deserialize(deserializer)?;
        let decode = || -> Result<Self, sync_common::transport::WireError> {
            use sync_common::transport::WireError;
            if !Self::recognizes_kind(&entry.address.kind) {
                return Err(WireError::Decode("unknown state kind".into()));
            }
            let value: Self = sync_common::transport::decode_state(&entry.bytes, sync_common::transport::MAX_DECODED_REQUEST_BYTES)?;
            value.validate()?;
            if value.address() != entry.address || value.book() != entry.book {
                return Err(WireError::Decode("state envelope does not match its value".to_owned()));
            }
            Ok(value)
        };
        decode().map_err(serde::de::Error::custom)
    }

    fn validate_version(&self, version: &VersionKey) -> Result<(), sync_common::transport::WireError> {
        if version.conflict_rank != self.conflict_rank() {
            return Err(sync_common::transport::WireError::Decode("state version rank does not match its value".into()));
        }
        Ok(())
    }
}

fn validate_navigation(value: &BookTocDocument) -> Result<(), sync_common::transport::WireError> {
    use sync_common::transport::WireError;
    if value.duration_ms.is_some_and(|duration| duration > i64::MAX as u64) {
        return Err(WireError::Decode("audiobook duration exceeds storage range".into()));
    }
    if let Some(tracks) = &value.tracks {
        let mut end = 0;
        let mut names = std::collections::HashSet::new();
        if tracks.is_empty()
            || tracks.len() > 1024
            || tracks.iter().any(|track| {
                let invalid = !track.valid() || track.start_ms != end || !names.insert(&track.name);
                end = track.end_ms;
                invalid
            })
            || value.duration_ms != Some(end)
        {
            return Err(WireError::Decode("invalid audiobook track index".into()));
        }
    }
    if sync_common::transport::encode(value)?.len() > 512 * 1024 {
        return Err(WireError::Decode("navigation document exceeds the mutation envelope".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coalescing_keeps_winning_versions_and_first_equal_candidate() {
        let make = |name: &str, time, sequence| LibraryStateEntry::DirectoryName { dir_id: DirId::from_u128(1), name: name.into() }.publication(ReplicaId::from_u128(1), time, ReplicaSeq::new(sequence).unwrap());
        let newest = make("winning", 3, 1);
        let stale = make("stale retry", 2, 2);
        let mut equal = make("equal candidate", 3, 1);
        equal.publisher_seq = ReplicaSeq::new(99).unwrap();
        let other = LibraryStateEntry::DirectoryName { dir_id: DirId::from_u128(2), name: "other".into() }.publication(ReplicaId::from_u128(1), 1, ReplicaSeq::new(3).unwrap());
        assert_eq!(coalesce_mutations_for_push(vec![newest.clone(), stale, equal, other.clone()]), vec![newest, other]);
    }

    #[test]
    fn streamed_preparation_preserves_lifecycle_and_publication_order() {
        let publish = |state: LibraryStateEntry, sequence| state.publication(ReplicaId::from_u128(1), 1, ReplicaSeq::new(sequence).unwrap());
        let directory = |id| LibraryStateEntry::DirectoryName { dir_id: DirId::from_u128(id), name: "directory".into() };
        let lifecycle = |hash| LibraryStateEntry::BookLifecycle { content_hash: ContentHash::from_bytes([hash; 32]), value: BookLifecycleState::Present };
        let input = vec![publish(directory(1), 1), publish(lifecycle(1), 2), publish(directory(2), 3), publish(lifecycle(2), 4)];
        let expected: Vec<sync_common::OpaquePublication> = [1, 3, 0, 2].into_iter().map(|index| sync_common::transport::representation(&input[index]).unwrap()).collect();
        let mut batcher = prepare_push(input).unwrap();
        assert_eq!(batcher.next_batch(), expected);
        assert!(batcher.is_empty());
    }

    #[test]
    #[ignore = "release benchmark of typed outbox preparation; run in a separate process"]
    fn outbox_preparation_performance() {
        use std::time::Instant;
        let publications = (1..=1_000).map(|sequence| {
            LibraryStateEntry::BookAdd {
                content_hash: ContentHash::from_bytes([sequence as u8; 32]),
                value: BookAddition {
                    facts: BookFacts { added_at: 1, format: book_model::BookFormat::Epub },
                    record: BookRecord { metadata: Default::default(), description: "x".repeat(64 * 1024) },
                    navigation: None, pdf: None, recording: None, enrichment: Some(EnrichmentResult::NoMatch),
                },
            }.publication(ReplicaId::from_u128(1), 1, ReplicaSeq::new(sequence).unwrap())
        }).collect();
        let started = Instant::now();
        let mut batcher = prepare_push(publications).unwrap();
        let prepared_ms = started.elapsed().as_secs_f64() * 1000.0;
        let mut drained = 0;
        while !batcher.is_empty() { drained += batcher.next_batch().len(); }
        assert_eq!(drained, 1_000);
        println!("PREPARE cells={drained} payload_bytes={} elapsed_ms={prepared_ms:.3}", 1_000 * 64 * 1024);
        if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
            if let Some(line) = status.lines().find(|line| line.starts_with("VmHWM:")) { println!("PREPARE {line}"); }
        }
    }

    #[test]
    fn history_upgrade_preserves_relay_values_written_with_protocol_28() {
        let publication = LibraryStateEntry::DirectoryName { dir_id: DirId::from_u128(1), name: "Retained".into() }.publication(ReplicaId::from_u128(2), 3, ReplicaSeq::new(4).unwrap());
        let mut stored: Versioned<OpaqueStateEntry> = sync_common::transport::representation(&publication.state).unwrap();
        stored.value.bytes[1] = 28;
        let decoded = sync_common::state_encoding::decode_known::<LibraryStateEntry>(&stored).unwrap().unwrap();
        assert_eq!(decoded, publication.state);
    }

    #[test]
    fn only_completed_additions_can_be_encoded_for_publication() {
        assert!(!LibraryStateEntry::recognizes_kind("enrichment"));
        let mut addition = BookAddition {
            facts: BookFacts { added_at: 1, format: book_model::BookFormat::Epub },
            record: BookRecord { metadata: Default::default(), description: String::new() },
            navigation: None,
            pdf: None,
            recording: None,
            enrichment: None,
        };
        let publish = |value| LibraryStateEntry::BookAdd { content_hash: ContentHash::new(&"f".repeat(64)), value }.publication(ReplicaId::from_u128(1), 0, ReplicaSeq::new(1).unwrap());
        let draft = publish(addition.clone());
        assert!(sync_common::transport::representation::<_, sync_common::OpaquePublication>(&draft).is_err());
        addition.enrichment = Some(EnrichmentResult::NoMatch);
        let complete = publish(addition);
        assert!(sync_common::transport::representation::<_, sync_common::OpaquePublication>(&complete).is_ok());
        let encoded = sync_common::transport::encode(&complete).unwrap();
        let decoded: StatePublication = sync_common::transport::decode_byte_fields(&encoded, sync_common::transport::MAX_DECODED_REQUEST_BYTES).unwrap();
        assert_eq!(decoded, complete);
    }
}
