use catalog_model::normalize_search_text;
use catalog_model::{CatalogBookFormat, CatalogDescription, CatalogLinkTarget, CatalogPage, CatalogPublication, CatalogPublicationAction, CatalogPublicationIdentity, CatalogSourceId};
use publication_model::PublicationSubject;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::time::Instant;

pub(crate) type CatalogSubjectAssignment = subject_projection::ProjectedSubjectAssignment;

pub(crate) fn prepare_catalog_subject_assignments(subjects: &[PublicationSubject]) -> Vec<CatalogSubjectAssignment> {
    if subjects.is_empty() {
        return Vec::new();
    }
    subject_projection::project_subjects(subjects).into_iter().chain(subject_projection::project_unified_subjects(subjects)).collect()
}

// Shared local bibliographic inspection now checks front/back matter.
const CATALOG_EPUB_ENRICHMENT_VERSION: i64 = 14;
const CATALOG_METADATA_BACKFILL_VERSION: i64 = 4;
const CATALOG_DESCRIPTION_BACKFILL_VERSION: i64 = 4;
const CATALOG_METADATA_INDEX_VERSION: i64 = 1;
const CATALOG_BROWSE_SERVING_INDEX_VERSION: i64 = 2;
const CATALOG_SEARCH_INDEX_VERSION: i64 = 1;
const CATALOG_SUBJECT_BROWSE_INDEX_VERSION: i64 = 1;
const MAX_CRAWL_ERROR_BYTES: usize = 2_048;

pub use process_control::CatalogProcessSettings;
pub use types::*;

pub(crate) use storage::CatalogStoreError;
use storage::{decode_cached, optional_stored_u64, sql_decode_error, storage_i64, stored_u64};

#[derive(Clone)]
pub struct CatalogStore {
    pool: Pool<SqliteConnectionManager>,
    serving: std::sync::Arc<serving_projection::ServingStore>,
}

#[derive(Clone, Debug)]
pub(crate) struct CatalogPageWrite {
    pub page: CatalogPage,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub provider_paginated: bool,
}

/// A server-owned crawl write prepared before entering the serialized SQLite
/// writer queue.
pub(crate) struct PreparedCatalogPageWrite {
    write: CatalogPageWrite,
    payload: Vec<u8>,
    publications: Vec<PreparedCatalogPublication>,
}

pub(crate) struct PreparedCatalogPublication {
    publication: CatalogPublication,
    payload: Vec<u8>,
    payload_hash: String,
    normalized_title: String,
    provider_popularity: u64,
    normalized_authors: Vec<String>,
    languages: Vec<String>,
    formats: Vec<&'static str>,
    subjects: Vec<PublicationSubject>,
    subject_assignments: Vec<CatalogSubjectAssignment>,
    book_format: Option<&'static str>,
    importable: bool,
    has_upstream_cover: bool,
}

impl CatalogStore {
    pub fn open(path: impl Into<std::path::PathBuf>) -> Result<Self, CatalogStoreError> {
        let path = path.into();
        schema::prepare_database_file(&path)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| CatalogStoreError::new(error.to_string()))?;
        }
        {
            let connection = Connection::open(&path)?;
            connection.busy_timeout(std::time::Duration::from_secs(10))?;
            connection.pragma_update(None, "journal_mode", "WAL")?;
        }
        let manager = SqliteConnectionManager::file(&path).with_init(schema::configure_connection);
        let pool = Pool::builder().max_size(8).build(manager).map_err(|error| CatalogStoreError::new(error.to_string()))?;
        let serving = std::sync::Arc::new(serving_projection::ServingStore::open(&path)?);
        let store = Self { pool, serving };
        schema::initialize_server_schema(&store)?;
        serving_projection::initialize(&store)?;
        Ok(store)
    }

    pub(crate) fn prepare_page_writes(pages: Vec<CatalogPageWrite>) -> Result<Vec<PreparedCatalogPageWrite>, CatalogStoreError> {
        pages.into_iter().map(PreparedCatalogPageWrite::new).collect()
    }

    fn connection(&self) -> Result<r2d2::PooledConnection<SqliteConnectionManager>, CatalogStoreError> {
        self.pool.get().map_err(|error| CatalogStoreError::new(error.to_string()))
    }
}

mod authority;
mod cover_queue;
mod crawl_generation;
mod extraction_queue;
mod frontier;
mod identity_review;
mod indexes;
mod ingestion;
mod inventory;
mod metadata_backfill;
mod ocr_backfill;
mod page_search;
mod process_control;
mod publication_enrichment;
mod publication_storage;
mod schema;
mod serving_queries;
mod serving_changes;
mod serving_projection;
mod sources;
mod storage;
mod subject_browse;
mod subjects;
mod types;
mod work_supplement_backfill;

#[cfg(test)]
mod tests;

pub(crate) use authority::{advance_catalog_transport_revision, allocate_sequence, next_sequence_in, sequence_in, sequenced_publication_in, unix_timestamp};
pub(crate) use cover_queue::queue_publication_cover;
pub(crate) use extraction_queue::{merge_extracted_author_names, metadata_index_from_metadata, queue_publication_extraction};
pub(crate) use frontier::put_page_row;
pub(crate) use identity_review::{identity_review_inventory_in, record_identity_review, record_identity_review_failure};
pub(crate) use ingestion::{insert_staged_publication, upsert_active_publication};
pub(crate) use publication_enrichment::merge_catalog_subject_evidence;
pub(crate) use publication_storage::{book_format_storage, catalog_book_format_from_storage, publication_entry_id, publication_extraction, publication_inventory, publication_key};
pub(crate) use schema::bounded_crawl_error;
pub(crate) use sources::source_state_in;
pub(crate) use sources::{active_generation_in, active_generation_optional, ensure_source_in, refresh_active_counts};
pub(crate) use subjects::{
    insert_prepared_subject_assignments, insert_subject_browse_memberships, rebuild_subject_assignments, replace_feed_publication_subjects_prepared, replace_publication_subjects_prepared, replace_publication_toc,
    ENRICHED_SUBJECT_POSITION_BASE,
};
