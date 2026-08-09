//! Portable Bokheim domain values and validation rules.
//!
//! This crate deliberately has no database, filesystem, networking, UI, or
//! runtime dependencies so native and server applications can share it.

mod author_identity;
mod annotations;
mod book_format;
mod contributor_roles;
mod library_state;
mod library_values;
mod paths;
mod reading;
mod reader_state;
mod sync_state;

pub use author_identity::{AgentAuthority, AgentIdentityError, AuthorAuthority, AuthorIdentityError, ExternalAgentId, ExternalAuthorId, Isni, LibraryOfCongressAuthorityId, OpenLibraryAuthorId, Orcid, ViafId, WikidataItemId};
pub use annotations::{AnnotationAnchor, AnnotationState, AnnotationStyle, PdfAnnotationAnchor, PdfAnnotationRect};
pub use book_format::BookFormat;
pub use contributor_roles::{classify as classify_contributor_role, marc_relator_code_from_metadata_role, ContentRole, ContributorRole, PerformanceRole, ProductionRole, ProvenanceRole, SupplementaryWriterRole, SupportRole, VisualRole, AUTHOR as AUTHOR_MARC_RELATOR_CODE, CONTRIBUTOR as CONTRIBUTOR_MARC_RELATOR_CODE};
pub use library_state::{BookLifecycleState, BookMetadataState, ReadingPositionState};
pub use library_values::{AuthorName, AuthorNameMatchKey, BookTitle, FolderPath, LibraryValueError, NonNegativeTimestamp};
pub use paths::{BookPlacement, RelativeBookPath, RelativeBookPathError, RelativeDirPath, RelativeDirPathError};
pub use reading::{EpubCfi, InvalidEpubCfi, InvalidReadProgress, ReadProgress};
pub use reader_state::{BookmarkState, BookLinkState};
pub use sync_state::{MutationBody, StateKey, StateMutation, VersionKey, reduce_same_replica_mutations};

/// Stable identity for a book within one configured library.
#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct BookLocator {
    library_id: bokheim_identifiers::LibraryId,
    content_hash: bokheim_identifiers::ContentHash,
}

impl BookLocator {
    pub fn new(library_id: bokheim_identifiers::LibraryId, content_hash: bokheim_identifiers::ContentHash) -> Self {
        Self { library_id, content_hash }
    }

    pub fn parse(library_id: &str, content_hash: bokheim_identifiers::ContentHash) -> Result<Self, bokheim_identifiers::DomainTextError> {
        Ok(Self::new(bokheim_identifiers::LibraryId::parse(library_id)?, content_hash))
    }

    pub fn library_id(&self) -> &bokheim_identifiers::LibraryId {
        &self.library_id
    }
    pub fn content_hash(&self) -> bokheim_identifiers::ContentHash {
        self.content_hash.clone()
    }
}
