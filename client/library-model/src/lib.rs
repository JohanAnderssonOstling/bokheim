//! Portable library values and validation rules.
//!
//! This crate deliberately has no filesystem, synchronization, networking, UI,
//! or runtime dependencies. Native builds use SQLite only to load the embedded
//! unified taxonomy; portable values remain independent of database handles.

mod annotations;
mod author_identity;
mod book_format;
mod contributor_roles;
mod folder_management;
mod library_state;
mod marc;
mod publication;
mod reader_state;
mod reading;
mod subjects;
#[cfg(not(target_arch = "wasm32"))]
mod taxonomy_sqlite;

pub use annotations::{AnnotationAnchor, AnnotationState, AnnotationStyle, PdfAnnotationAnchor, PdfAnnotationRect};
pub use author_identity::{AgentAuthority, AgentIdentityError, AuthorAuthority, AuthorIdentityError, ExternalAgentId, ExternalAuthorId, Isni, LibraryOfCongressAuthorityId, OpenLibraryAuthorId, Orcid, ViafId, WikidataItemId};
pub use book_format::BookFormat;
pub use catalog_model::{canonical_doi, canonical_isbn, canonical_uuid, normalize_search_text, normalized_substring_match, AuthorName, AuthorNameError, AuthorNameMatchKey};
pub use contributor_roles::{
    classify as classify_contributor_role, marc_relator_code_from_metadata_role, ContentRole, ContributorRole, PerformanceRole, ProductionRole, ProvenanceRole, SupplementaryWriterRole, SupportRole, VisualRole,
    AUTHOR as AUTHOR_MARC_RELATOR_CODE, CONTRIBUTOR as CONTRIBUTOR_MARC_RELATOR_CODE,
};
pub use folder_management::{numbered_file_name, numbered_folder_name, unique_file_name, unique_folder_name, FolderDestinationPolicy};
pub use library_state::{portable_name_key, BookLifecycleState, BookMetadataState, DirectoryLifecycleState, ReadingPositionState};
pub use marc::MarcRelatorCode;
pub use publication::{
    agent_id_from_migration_seed, unique_publication_year, AgentId, AuthorId, Contributor, ContributorId, LanguageTag, PublicationCollection, PublicationDate, PublicationIdentifier, PublicationIdentifierScheme, PublicationIdentifierType,
    PublicationMetadata, PublicationMetadataError, PublicationProperty, PublicationSubject, PublicationTitle, PublicationTocEntry, PublisherCredit, PublisherId, PublisherMatchKey, PublisherName,
};
pub use reader_state::{BookLinkState, BookmarkState};
pub use reading::{
    audiobook_position_millis, audiobook_reading_position, pdf_page_from_reading_position, pdf_page_position_from_reading_position, pdf_page_position_reading_position, pdf_page_reading_position, valid_epub_cfi, valid_reading_position,
    EpubCfi, InvalidEpubCfi, InvalidReadProgress, InvalidReadingPosition, ReadProgress, ReadingEpubCfi, ReadingPosition,
};
pub use subjects::{
    bisac_code_for_path, bisac_subject_system, canonical_lcc_notation, ddc_subject_path, install_unified_taxonomy_concepts, install_unified_taxonomy_overlay, lcc_subject_path, loaded_unified_taxonomy_release_id, match_bisac_subjects,
    match_bisac_subjects_with_minimum_adjacent, project_subject_codes, project_subjects, project_unified_subjects, unified_concept_definitions, unified_concept_id, unified_ddc_subject_paths, unified_has_exact_source_code,
    unified_lcc_subject_paths, unified_subject_paths, unified_subject_similarity_keys, unified_subject_sort_order, unified_taxonomy_release_id, unified_taxonomy_routes, ProjectedSubjectAssignment, ProjectedSubjectCode, SubjectAssignment,
    SubjectNode, SubjectSystem, UnifiedConceptDefinition, UnifiedConceptRoute, UnifiedTaxonomy, BISAC_MATCHER_VERSION, BISAC_SYSTEM_ID, BISAC_VERSION, BUNDLED_UNIFIED_TAXONOMY_RELEASE_ID, DDC_MATCHER_VERSION, DDC_SYSTEM_ID, DDC_VERSION,
    DEFAULT_UNIFIED_TAXONOMY_SQLITE, LCC_MATCHER_VERSION, LCC_SYSTEM_ID, LCC_VERSION, ROOT_SUBJECT_PATH, UNIFIED_MATCHER_VERSION, UNIFIED_SYSTEM_ID, UNIFIED_VERSION,
};
#[cfg(not(target_arch = "wasm32"))]
pub use taxonomy_sqlite::{
    has_unified_taxonomy_overlay, is_unified_taxonomy_client_seed, merge_unified_taxonomy_sqlite, merge_unified_taxonomy_sqlite_with_resolutions, read_resolved_unified_taxonomy_codes, read_unified_taxonomy_sqlite,
    unified_taxonomy_server_release_id, UnifiedCodeResolution,
};

pub use content_address::ContentHash;

/// A non-negative Unix timestamp measured in milliseconds.
pub type UnixMillis = u64;
