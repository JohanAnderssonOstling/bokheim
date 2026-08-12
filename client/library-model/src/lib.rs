//! Portable library values and validation rules.
//!
//! This crate deliberately has no database, filesystem, synchronization,
//! networking, UI, or runtime dependencies.

mod annotations;
mod author_identity;
mod book_format;
mod contributor_roles;
mod library_state;
mod marc;
mod publication;
mod reader_state;
mod reading;
mod subjects;

pub use annotations::{AnnotationAnchor, AnnotationState, AnnotationStyle, PdfAnnotationAnchor, PdfAnnotationRect};
pub use author_identity::{AgentAuthority, AgentIdentityError, AuthorAuthority, AuthorIdentityError, ExternalAgentId, ExternalAuthorId, Isni, LibraryOfCongressAuthorityId, OpenLibraryAuthorId, Orcid, ViafId, WikidataItemId};
pub use book_format::BookFormat;
pub use catalog_model::{canonical_doi, canonical_isbn, canonical_uuid, normalize_search_text, normalized_substring_match, AuthorName, AuthorNameError, AuthorNameMatchKey};
pub use contributor_roles::{
    classify as classify_contributor_role, marc_relator_code_from_metadata_role, ContentRole, ContributorRole, PerformanceRole, ProductionRole, ProvenanceRole, SupplementaryWriterRole, SupportRole, VisualRole,
    AUTHOR as AUTHOR_MARC_RELATOR_CODE, CONTRIBUTOR as CONTRIBUTOR_MARC_RELATOR_CODE,
};
pub use library_state::{portable_name_key, BookLifecycleState, BookMetadataState, DirectoryLifecycleState, ReadingPositionState};
pub use marc::MarcRelatorCode;
pub use publication::{
    agent_id_from_migration_seed, AgentId, AuthorId, Contributor, ContributorId, LanguageTag, PublicationCollection, PublicationDate, PublicationIdentifier, PublicationIdentifierScheme, PublicationIdentifierType, PublicationMetadata,
    PublicationMetadataError, PublicationProperty, PublicationSubject, PublicationTitle, PublisherCredit, PublisherId, PublisherMatchKey, PublisherName,
};
pub use reader_state::{BookLinkState, BookmarkState};
pub use reading::{
    audiobook_position_millis, audiobook_reading_position, pdf_page_from_reading_position, pdf_page_reading_position, valid_epub_cfi, valid_reading_position, EpubCfi, InvalidEpubCfi, InvalidReadProgress, InvalidReadingPosition,
    ReadProgress, ReadingEpubCfi, ReadingPosition,
};
pub use subjects::{bisac_subject_system, match_bisac_subjects, SubjectAssignment, SubjectNode, SubjectSystem, BISAC_MATCHER_VERSION, BISAC_SYSTEM_ID, BISAC_VERSION};

pub use content_address::ContentHash;

/// A non-negative Unix timestamp measured in milliseconds.
pub type UnixMillis = u64;
