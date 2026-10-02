//! Client-replica interpretation of opaque sync payloads.
//!
//! The sync service depends only on `sync-common` and never links this crate.

use std::sync::OnceLock;
use book_model::AnnotationState;
pub use book_model::BookDetails;
pub mod filenames;
mod folder_management;
mod folder_names;
pub use folder_names::project_folder_name;
mod library_state;

pub use folder_management::{file_name_candidates, folder_name_candidates, numbered_file_name, numbered_folder_name, unique_file_name, unique_folder_name};
pub use library_state::{portable_name_key, BookFacts, BookLifecycleState, DirectoryLifecycleState, ReadingPositionState};

use serde::{Deserialize, Serialize};

use std::collections::HashMap;
use sync_common::ContentHash;
pub use sync_common::VersionKey;
use sync_common::{ConflictRank, DirId, FileName, OpaqueStateEntry, Publication, ReplicaId, ReplicaSeq, StateAddress, StatePayload, UnixMillis, Versioned};

mod directory_projection;
mod legacy_mutation;
pub(crate) mod serde_enum;
pub use directory_projection::{project_directories, DirectoryIntent, DirectoryProjection};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum LibraryStateEntry {
    DirectoryName {
        dir_id: DirId,
        name: FileName,
    },
    DirectoryParent {
        dir_id: DirId,
        parent_id: DirId,
    },
    DirectoryLifecycle {
        dir_id: DirId,
        value: DirectoryLifecycleState,
    },
    BookLifecycle {
        content_hash: ContentHash,
        value: BookLifecycleState,
    },
    Placement {
        dir_id: DirId,
        content_hash: ContentHash,
        present: bool,
        origin_folder_id: Option<String>,
    },
    ReadingPosition {
        content_hash: ContentHash,
        value: ReadingPositionState,
    },
    Annotation {
        annotation_id: String,
        value: AnnotationState,
    },
    Metadata {
        content_hash: ContentHash,
        value: BookDetails,
    },
    Description {
        content_hash: ContentHash,
        value: String,
    },
    PdfReaderMetadata {
        content_hash: ContentHash,
        value: pdf_view_common::PdfReaderMetadata,
    },
    /// The book's navigation, keyed by the book's identity rather than
    /// by the bytes it was read from. Transported because it is the one
    /// navigation a replica cannot always rebuild: an audiobook's chapters may
    /// have come from enrichment rather than from the file. An audiobook's
    /// total duration travels with its entries: without it a replica can show
    /// the chapters but cannot play the book.
    BookToc {
        content_hash: ContentHash,
        value: book_model::BookTocDocument,
    },
    BookFacts {
        content_hash: ContentHash,
        value: BookFacts,
    },
}

/// A publication uses the same envelope whether its state is decoded or opaque.
pub type StatePublication = Publication<LibraryStateEntry>;

/// Coalesce state by its original winning version, independently of the
/// publication's retry ID, publishing replica, or queue sequence.
pub fn coalesce_mutations_for_push(publications: Vec<StatePublication>) -> Vec<StatePublication> {
    let mut entries = HashMap::<StateAddress, StatePublication>::new();
    for publication in publications {
        let address = publication.state.value.address();
        match entries.get(&address) {
            Some(current) if current.state.version >= publication.state.version => {}
            _ => {
                entries.insert(address, publication);
            }
        }
    }
    let mut reduced = entries.into_values().collect::<Vec<_>>();
    reduced.sort_by_key(|publication| publication.publisher_seq);
    reduced
}

pub fn prepare_push(mut publications: Vec<StatePublication>) -> Result<sync_common::PushBatcher, Box<dyn std::error::Error>> {
    publications.sort_by_key(|publication| !matches!(publication.state.value, LibraryStateEntry::BookLifecycle { .. }));
    let encoded = publications.iter().map(sync_common::transport::representation::<_, sync_common::OpaquePublication>).collect::<Result<Vec<_>, _>>()?;
    Ok(sync_common::PushBatcher::new(encoded)?)
}

impl LibraryStateEntry {
    /// Book bytes that must be uploaded before this state can be shared.
    /// Removals must remain possible even when a local import never uploaded.
    pub fn upload_dependency(&self) -> Option<sync_common::ContentHash> {
        match self {
            Self::DirectoryName { .. } | Self::DirectoryParent { .. } | Self::DirectoryLifecycle { .. } | Self::BookLifecycle { value: BookLifecycleState::Purged, .. } | Self::Placement { present: false, .. } => None,
            Self::Annotation { value, .. } => (!value.deleted).then_some(value.content_hash),
            Self::BookFacts { content_hash, .. }
            | Self::BookLifecycle { content_hash, .. }
            | Self::Placement { content_hash, .. }
            | Self::ReadingPosition { content_hash, .. }
            | Self::Metadata { content_hash, .. }
            | Self::Description { content_hash, .. }
            | Self::PdfReaderMetadata { content_hash, .. }
            | Self::BookToc { content_hash, .. } => Some(*content_hash),
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
            Self::DirectoryName { dir_id, .. } | Self::DirectoryParent { dir_id, .. } | Self::DirectoryLifecycle { dir_id, .. } => (dir_id.to_string(), String::new()),
            Self::Placement { content_hash, dir_id, .. } => (content_hash.to_string(), dir_id.to_string()),
            Self::Annotation { annotation_id, .. } => (annotation_id.clone(), String::new()),
            Self::PdfReaderMetadata { content_hash, value } => (content_hash.to_string(), value.checksum.clone()),
            Self::BookFacts { content_hash, .. }
            | Self::BookLifecycle { content_hash, .. }
            | Self::ReadingPosition { content_hash, .. }
            | Self::Metadata { content_hash, .. }
            | Self::Description { content_hash, .. }
            | Self::BookToc { content_hash, .. } => (content_hash.to_string(), String::new()),
        };
        StateAddress { kind: self.kind().to_owned(), entity_key, entity_subkey }
    }

    /// The book this value belongs to. The service purges and accounts by it.
    pub fn book(&self) -> Option<sync_common::ContentHash> {
        match self {
            Self::DirectoryName { .. } | Self::DirectoryParent { .. } | Self::DirectoryLifecycle { .. } => None,
            Self::Annotation { value, .. } => Some(value.content_hash),
            Self::BookFacts { content_hash, .. }
            | Self::BookLifecycle { content_hash, .. }
            | Self::Placement { content_hash, .. }
            | Self::ReadingPosition { content_hash, .. }
            | Self::Metadata { content_hash, .. }
            | Self::Description { content_hash, .. }
            | Self::PdfReaderMetadata { content_hash, .. }
            | Self::BookToc { content_hash, .. } => Some(*content_hash),
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
            Self::PdfReaderMetadata { value, .. } => {
                if !value.valid_for(&value.checksum) {
                    return Err(invalid("invalid PDF reader metadata"));
                }
            }
            Self::BookToc { value, .. } => {
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
                        return Err(invalid("invalid audiobook track index"));
                    }
                }
                if sync_common::transport::encode(value)?.len() > 512 * 1024 {
                    return Err(invalid("navigation document exceeds the mutation envelope"));
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
            Self::DirectoryLifecycle { value: DirectoryLifecycleState::Deleted, .. } | Self::BookLifecycle { value: BookLifecycleState::Deleted { .. }, .. } | Self::Annotation { value: AnnotationState { deleted: true, .. }, .. } => ConflictRank::Delete,
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
            // Malformed generic envelopes never fall back to the legacy reader.
            let value: Self = if entry.bytes.first() == Some(&0x08) { sync_common::transport::decode(&entry.bytes, sync_common::transport::MAX_DECODED_REQUEST_BYTES)? } else { legacy_mutation::decode(&entry)? };
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

#[cfg(test)]
mod tests {
    use super::*;
    use book_model::{AnnotationAnchor, AnnotationStyle, BookFormat, ReadingPosition};
    use book_model::{BookSubject, LocalizedText};

    fn content_hash(character: char) -> ContentHash {
        ContentHash::new(&character.to_string().repeat(64))
    }

    fn dir_id(value: &str) -> DirId {
        DirId::parse_str(value).unwrap()
    }

    fn metadata() -> BookDetails {
        let mut book = book_model::BookMetadata::default();
        book.subjects.push(
            BookSubject::with_details(None, "Historiska romaner", vec![LocalizedText::new("en", "Historical fiction").unwrap()], Some("Romaner, historiska".to_owned()), Vec::new(), "test:subject", Some("saogf".to_owned()), None).unwrap(),
        );
        BookDetails { title: "Book".to_owned(), subtitle: None, contributors: Vec::new(), book }
    }

    #[test]
    fn every_library_payload_round_trips_through_the_opaque_protocol_envelope() {
        let first_dir = dir_id("11111111-1111-4111-8111-111111111111");
        let second_dir = dir_id("22222222-2222-4222-8222-222222222222");
        let first_hash = content_hash('a');
        let bodies = vec![
            LibraryStateEntry::DirectoryName { dir_id: first_dir, name: "Shelf".to_owned() },
            LibraryStateEntry::DirectoryParent { dir_id: first_dir, parent_id: second_dir },
            LibraryStateEntry::DirectoryLifecycle { dir_id: first_dir, value: DirectoryLifecycleState::Present },
            LibraryStateEntry::BookLifecycle { content_hash: first_hash, value: BookLifecycleState::Present },
            LibraryStateEntry::BookFacts { content_hash: first_hash, value: BookFacts { added_at: 1, format: BookFormat::Epub } },
            LibraryStateEntry::Placement { dir_id: first_dir, content_hash: first_hash, present: true, origin_folder_id: None },
            LibraryStateEntry::ReadingPosition { content_hash: first_hash, value: ReadingPositionState { location: ReadingPosition::parse("epubcfi(/6/2)").unwrap(), progress: 0.5 } },
            LibraryStateEntry::Annotation {
                annotation_id: "annotation".to_owned(),
                value: AnnotationState {
                    content_hash: first_hash,
                    detail: book_model::AnnotationDetail {
                        anchor: AnnotationAnchor::epub_cfi("epubcfi(/6/2!/4/1:0)"),
                        exact_text: "Text".to_owned(),
                        style: AnnotationStyle::Highlight,
                        color: "#ffee00".to_owned(),
                        note: String::new(),
                        created_at: 1,
                        toc_ordinal: Some(5),
                        progress: Some(42.5),
                    },
                    modified_at: 2,
                    deleted: false,
                },
            },
            LibraryStateEntry::Metadata { content_hash: first_hash, value: metadata() },
            LibraryStateEntry::Description { content_hash: first_hash, value: "Independent description".into() },
            LibraryStateEntry::BookToc {
                content_hash: first_hash,
                value: book_model::BookTocDocument { entries: vec![book_model::BookTocEntry { title: "One".into(), target: book_model::audiobook_toc_target(0), children: Vec::new() }], duration_ms: Some(1000), tracks: None },
            },
        ];

        for (index, body) in bodies.into_iter().enumerate() {
            let mutation = body.publication(sync_common::ReplicaId::nil(), 100 + index as u64, ReplicaSeq::new(index as u64 + 1).unwrap());
            let wire = sync_common::transport::representation::<_, sync_common::OpaquePublication>(&mutation).unwrap();
            let bytes = sync_common::transport::encode(&mutation).unwrap();
            let opaque: sync_common::OpaquePublication = sync_common::transport::decode(&bytes, 8192).unwrap();
            assert_eq!(opaque, wire);
            assert_eq!(sync_common::transport::encode(&opaque).unwrap(), bytes);
            assert_eq!(sync_common::transport::decode::<StatePublication>(&bytes, 8192).unwrap(), mutation);
            let stored = sync_common::wire::encode(&mutation).unwrap();
            assert_eq!(sync_common::wire::decode::<StatePublication>(&stored, 8192).unwrap(), mutation);
            assert_eq!(wire.state.value.address.kind, mutation.state.value.kind());
            let dependency = (index >= 3).then_some(first_hash);
            assert_eq!(mutation.state.value.upload_dependency(), dependency);
            assert_eq!(wire.state.value.book, dependency);
            assert!(!wire.state.value.bytes.starts_with(b"BKHM"), "network mutation values must not use the bincode storage envelope");
            sync_common::transport::decode::<LibraryStateEntry>(&wire.state.value.bytes, sync_common::transport::MAX_DECODED_REQUEST_BYTES).unwrap();
            assert_eq!(sync_common::state_encoding::decode_publication::<LibraryStateEntry>(&wire).unwrap(), Some(mutation));
        }
    }

    #[test]
    fn recovery_push_sends_creation_first_and_defers_fields_across_pages() {
        let hash = content_hash('a');
        let other = content_hash('b');
        let mutation = |body: LibraryStateEntry, sequence| body.publication(sync_common::ReplicaId::nil(), 1, ReplicaSeq::new(sequence).unwrap());
        let mut queued = (0..sync_common::MAX_PUSH_MUTATIONS)
            .map(|i| mutation(LibraryStateEntry::Placement { content_hash: hash, dir_id: sync_common::DirId::from_u128(i as u128 + 1), present: true, origin_folder_id: None }, i as u64 + 1))
            .collect::<Vec<_>>();
        queued.push(mutation(LibraryStateEntry::BookLifecycle { content_hash: hash, value: BookLifecycleState::Present }, 1000));
        queued.push(mutation(LibraryStateEntry::Description { content_hash: other, value: "Other book".into() }, 1001));
        let mut batcher = prepare_push(queued).unwrap();
        let first = batcher.next_batch();
        assert_eq!(first[0].state.value.address.kind, "book_lifecycle");
        assert_eq!(first.len(), sync_common::MAX_PUSH_MUTATIONS);
        batcher.defer_book_fields(&[hash.to_string()]);
        let next = batcher.next_batch();
        assert_eq!(next.len(), 1);
        assert_eq!(next[0].state.value.book, Some(other));
        assert!(batcher.is_empty());
    }

    #[test]
    fn annotation_deletion_keeps_owner_without_upload_dependency() {
        let hash = content_hash('a');
        let body = LibraryStateEntry::Annotation {
            annotation_id: "note".into(),
            value: book_model::AnnotationState {
                content_hash: hash,
                detail: book_model::AnnotationDetail {
                    anchor: book_model::AnnotationAnchor::epub_cfi("epubcfi(/6/2)"),
                    exact_text: String::new(),
                    style: book_model::AnnotationStyle::Highlight,
                    color: "#fff".into(),
                    note: String::new(),
                    created_at: 1,
                    toc_ordinal: None,
                    progress: None,
                },
                modified_at: 2,
                deleted: true,
            },
        };
        assert_eq!(body.upload_dependency(), None);
        let mut wire = sync_common::transport::representation::<_, sync_common::OpaquePublication>(&body.clone().publication(sync_common::ReplicaId::nil(), 2, ReplicaSeq::new(1).unwrap())).unwrap();
        assert_eq!(wire.state.value.book, Some(hash));
        assert_eq!(LibraryStateEntry::decode(&wire.state).unwrap(), Some(body));
        wire.state.value.book = Some(content_hash('b'));
        assert!(LibraryStateEntry::decode(&wire.state).is_err());
    }

    #[test]
    fn removing_an_unuploaded_import_does_not_require_its_bytes() {
        let hash = content_hash('a');
        let removals =
            [LibraryStateEntry::BookLifecycle { content_hash: hash, value: BookLifecycleState::Purged }, LibraryStateEntry::Placement { dir_id: sync_common::ROOT_DIR_ID, content_hash: hash, present: false, origin_folder_id: None }];
        for body in removals {
            assert_eq!(body.upload_dependency(), None);
            let wire = sync_common::transport::representation::<_, sync_common::OpaquePublication>(&body.clone().publication(sync_common::ReplicaId::nil(), 1, ReplicaSeq::new(1).unwrap())).unwrap();
            assert_eq!(wire.state.value.book, Some(hash), "a removal still names its book");
        }
    }

    #[test]
    fn unknown_service_payloads_remain_forward_compatible() {
        let wire = sync_common::Publication {
            publisher_seq: ReplicaSeq::new(1).unwrap(),
            state: sync_common::Versioned {
                version: sync_common::VersionKey::new(1, ConflictRank::Update, sync_common::ReplicaId::nil(), ReplicaSeq::new(1).unwrap()),
                value: sync_common::OpaqueStateEntry { address: sync_common::StateAddress { kind: "future_kind".to_owned(), entity_key: String::new(), entity_subkey: String::new() }, bytes: Vec::new(), book: None },
            },
        };
        assert_eq!(sync_common::state_encoding::decode_publication::<LibraryStateEntry>(&wire).unwrap(), None);
    }

    #[test]
    fn placement_updates_coalesce_to_the_latest_register_value() {
        let dir_id = dir_id("11111111-1111-4111-8111-111111111111");
        let claim = (LibraryStateEntry::Placement { dir_id, content_hash: content_hash('a'), present: true, origin_folder_id: None }).publication(sync_common::ReplicaId::nil(), 1, ReplicaSeq::new(1).unwrap());
        let release = (LibraryStateEntry::Placement { dir_id, content_hash: content_hash('a'), present: false, origin_folder_id: None }).publication(sync_common::ReplicaId::nil(), 2, ReplicaSeq::new(2).unwrap());
        let reduced = coalesce_mutations_for_push(vec![claim, release]);
        assert!(matches!(reduced.as_slice(), [StatePublication { state: Versioned { value: LibraryStateEntry::Placement { present: false, .. }, .. }, .. }]));
    }

    #[test]
    fn forwarding_and_coalescing_use_the_original_state_version() {
        let dir_id = DirId::from_u128(7);
        let older = LibraryStateEntry::DirectoryName { dir_id, name: "Older".into() }.publication(ReplicaId::from_u128(1), 10, ReplicaSeq::new(90).unwrap());
        let mut winner = LibraryStateEntry::DirectoryName { dir_id, name: "Winner".into() }.publication(ReplicaId::from_u128(2), 10, ReplicaSeq::new(1).unwrap());
        let original = winner.state.clone();

        winner.publisher_seq = ReplicaSeq::new(500).unwrap();
        let encoded = sync_common::transport::representation::<_, sync_common::OpaquePublication>(&winner).unwrap();
        assert_eq!(sync_common::state_encoding::decode_publication::<LibraryStateEntry>(&encoded).unwrap().unwrap(), winner);
        let reduced = coalesce_mutations_for_push(vec![winner.clone(), older.clone(), winner.clone()]);
        assert_eq!(reduced, vec![winner]);
        assert_eq!(reduced[0].state, original);
        assert_eq!(coalesce_mutations_for_push(vec![older, reduced[0].clone()]), reduced);
    }

    #[test]
    fn direct_serialization_rejects_conflicting_addresses_declarations_and_versions() {
        let entry = LibraryStateEntry::Placement { content_hash: content_hash('a'), dir_id: DirId::from_u128(7), present: true, origin_folder_id: None };
        let publication = entry.publication(ReplicaId::from_u128(1), 10, ReplicaSeq::new(1).unwrap());
        let encoded = sync_common::transport::representation::<_, sync_common::OpaquePublication>(&publication).unwrap();
        for field in 0..5 {
            let mut corrupt = encoded.clone();
            match field {
                0 => corrupt.state.value.address.kind = "description".into(),
                1 => corrupt.state.value.address.entity_key = content_hash('b').to_string(),
                2 => corrupt.state.value.address.entity_subkey = DirId::from_u128(8).to_string(),
                3 => corrupt.state.value.book = None,
                4 => corrupt.state.version.conflict_rank = ConflictRank::Purge,
                _ => unreachable!(),
            }
            assert!(sync_common::state_encoding::decode_publication::<LibraryStateEntry>(&corrupt).is_err());
        }
    }

    #[test]
    fn metadata_uses_domain_deserialization_validation() {
        let entry = LibraryStateEntry::Metadata { content_hash: content_hash('a'), value: metadata() };
        let publication = entry.publication(ReplicaId::from_u128(1), 10, ReplicaSeq::new(1).unwrap());
        let mut encoded = sync_common::transport::representation::<_, sync_common::OpaquePublication>(&publication).unwrap();
        let mut malformed = publication.state.value;
        let LibraryStateEntry::Metadata { value, .. } = &mut malformed else { unreachable!() };
        value.book.subjects[0].name_localizations[0].value.clear();
        encoded.state.value.bytes = sync_common::transport::encode(&malformed).unwrap();
        assert!(sync_common::state_encoding::decode_publication::<LibraryStateEntry>(&encoded).is_err());
    }
}
