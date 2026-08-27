use catalog_core::{prepare_catalog_subject_assignments, CatalogCache as SharedCatalogCache, CatalogCacheError, CatalogSourceState, CatalogSubjectAssignment};
use catalog_model::normalize_search_text;
use catalog_model::{CatalogBookFormat, CatalogPage, CatalogPublication, CatalogPublicationAction, CatalogPublicationIdentity, CatalogSourceId};
use publication_model::PublicationSubject;
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::ops::Deref;
use std::time::Instant;

const CATALOG_EPUB_ENRICHMENT_VERSION: i64 = 5;
const CATALOG_METADATA_BACKFILL_VERSION: i64 = 2;
const CATALOG_DESCRIPTION_BACKFILL_VERSION: i64 = 4;
const CATALOG_METADATA_INDEX_VERSION: i64 = 1;
const CATALOG_BROWSE_SERVING_INDEX_VERSION: i64 = 1;
const MAX_CRAWL_ERROR_BYTES: usize = 2_048;

pub use process_control::CatalogProcessSettings;
pub use types::*;

use catalog_core::server_extension::*;

#[derive(Clone)]
pub struct CatalogStore {
    inner: SharedCatalogCache,
}

/// Server-local name retained at call sites while the shared cache remains
/// available through `Deref` for ordinary cached catalogue reads.
pub type CatalogCache = CatalogStore;

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
    pub fn open(path: impl Into<std::path::PathBuf>) -> Result<Self, CatalogCacheError> {
        let store = Self { inner: SharedCatalogCache::open(path)? };
        schema::initialize_server_schema(&store)?;
        Ok(store)
    }

    pub(crate) fn prepare_page_writes(pages: Vec<CatalogPageWrite>) -> Result<Vec<PreparedCatalogPageWrite>, CatalogCacheError> {
        pages.into_iter().map(PreparedCatalogPageWrite::new).collect()
    }

    fn connection(&self) -> Result<catalog_core::server_extension::CatalogServerConnection, CatalogCacheError> {
        self.inner.server_connection()
    }
}

impl Deref for CatalogStore {
    type Target = SharedCatalogCache;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

mod cover_queue;
mod crawl_generation;
mod extraction_queue;
mod identity_review;
mod indexes;
mod ingestion;
mod inventory;
mod metadata_backfill;
mod page_search;
mod process_control;
mod publication_enrichment;
mod schema;
mod serving_queries;
mod subject_browse;
mod types;
mod work_supplement_backfill;

#[cfg(test)]
mod tests;

pub(crate) use cover_queue::queue_publication_cover;
pub(crate) use extraction_queue::{merge_extracted_author_names, merge_extracted_publication_authors, metadata_index_from_metadata, queue_publication_extraction};
pub(crate) use identity_review::{identity_review_inventory_in, record_identity_review, record_identity_review_failure};
pub(crate) use ingestion::{insert_staged_publication, upsert_active_publication};
pub(crate) use publication_enrichment::{catalog_subject_evidence, merge_catalog_subject_evidence};
pub(crate) use schema::bounded_crawl_error;
