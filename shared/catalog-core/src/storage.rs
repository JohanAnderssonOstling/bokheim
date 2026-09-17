use catalog_model::normalize_search_text;
use catalog_model::{CatalogBookFormat, CatalogLink, CatalogLinkTarget, CatalogPage, CatalogBook, CatalogBookAction, CatalogBookIdentity, CatalogSourceId, BookSubject};
#[cfg(not(target_arch = "wasm32"))]
use r2d2::Pool;
#[cfg(not(target_arch = "wasm32"))]
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;
use uuid::Uuid;

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone)]
struct CatalogPool(Pool<SqliteConnectionManager>);

#[cfg(not(target_arch = "wasm32"))]
impl CatalogPool {
    fn get(&self) -> Result<r2d2::PooledConnection<SqliteConnectionManager>, CatalogCacheError> {
        self.0.get().map_err(Into::into)
    }
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone)]
struct CatalogPool(std::rc::Rc<std::cell::RefCell<Connection>>);

#[cfg(target_arch = "wasm32")]
impl CatalogPool {
    fn get(&self) -> Result<std::cell::RefMut<'_, Connection>, CatalogCacheError> {
        self.0.try_borrow_mut().map_err(|_| CatalogCacheError::new("catalogue cache connection is already borrowed"))
    }
}

// Keep SQLite's file version at 1 because the additive v2 schema remains
// writable by the previous binary during an automated deployment rollback.
// The logical version lives in catalog_cache_metadata instead.
pub const CATALOG_CACHE_FILE_VERSION: i64 = 1;
// Version 30 removes client-era Gutenberg presentation state from the neutral
// cache; Gutenberg normalization and edition preference are server-owned.
// Version 29 moves persisted server-registry state into catalog-client; the
// shared schema no longer installs any client-only value tables.
// Version 28 moves the partial subject book cache into catalog-client;
// server and shared cache schemas no longer install client-only tables.
// Version 27 removes complete subject-collection synchronization; clients keep
// only immutable book payloads for server-authoritative result ranges.
// Version 26 stores provider rich HTML directly, without a generated
// plain-text copy.
// Version 25 records whether a logical page was compacted from provider
// pagination so replacement ingestion knows when every upstream page must be
// fetched again.
// Version 24 stores locator-scoped subject collections for local querying.
// Version 23 adopts shared sourced title/property evidence in persisted EPUB
// enrichment payloads.
// Version 22 introduced the now-retired Gutenberg edition preference used to
// canonicalize works before subject facets, ordering, totals, and ranges.
// Version 21 adopts the shared rich subject representation.
// Version 20 adopts the shared bibliographic collection representation.
// Version 19 adopts the shared bibliographic identifier representation.
// Version 18 stored provider-native popularity alongside normalized
// book inventory so catalogue ordering is complete before range
// selection.
// The cache is rehydratable, so replacing it is safer than attempting to
// decode an incompatible persisted representation during startup.
#[cfg(not(target_arch = "wasm32"))]
pub const CATALOG_CACHE_SCHEMA_VERSION: i64 = 30;
pub const CATALOG_SEARCH_INDEX_VERSION: i64 = 1;
pub const CATALOG_SUBJECT_BROWSE_INDEX_VERSION: i64 = 1;

const CREATE_SCHEMA: &str = include_str!("cache/schema.sql");
#[derive(Debug)]
pub struct CatalogCacheError(String);

impl CatalogCacheError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    pub fn is_database_locked(&self) -> bool {
        let message = self.0.to_ascii_lowercase();
        message.contains("database is locked") || message.contains("database table is locked")
    }
}

impl fmt::Display for CatalogCacheError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for CatalogCacheError {}

impl From<rusqlite::Error> for CatalogCacheError {
    fn from(error: rusqlite::Error) -> Self {
        Self::new(error.to_string())
    }
}

impl From<r2d2::Error> for CatalogCacheError {
    fn from(error: r2d2::Error) -> Self {
        Self::new(error.to_string())
    }
}

impl From<wire::WireError> for CatalogCacheError {
    fn from(error: wire::WireError) -> Self {
        Self::new(error.to_string())
    }
}

impl From<serde_json::Error> for CatalogCacheError {
    fn from(error: serde_json::Error) -> Self {
        Self::new(error.to_string())
    }
}

#[derive(Clone)]
pub struct CatalogCache {
    pool: CatalogPool,
}

#[cfg(all(feature = "server-extension", not(target_arch = "wasm32")))]
pub struct CatalogServerConnection(r2d2::PooledConnection<SqliteConnectionManager>);

#[cfg(all(feature = "server-extension", not(target_arch = "wasm32")))]
impl std::ops::Deref for CatalogServerConnection {
    type Target = Connection;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(all(feature = "server-extension", not(target_arch = "wasm32")))]
impl std::ops::DerefMut for CatalogServerConnection {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[cfg(all(feature = "server-extension", not(target_arch = "wasm32")))]
impl CatalogCache {
    #[doc(hidden)]
    pub fn server_connection(&self) -> Result<CatalogServerConnection, CatalogCacheError> {
        self.pool.get().map(CatalogServerConnection)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CachedCatalogPage {
    pub page: CatalogPage,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub provider_paginated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SequencedCatalogBook {
    pub book: CatalogBook,
    pub sequence: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CatalogSearchMatches {
    pub title_matches: Vec<SequencedCatalogBook>,
    pub author_matches: Vec<SequencedCatalogBook>,
    pub toc_matches: Vec<SequencedCatalogBook>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogSourceState {
    pub epoch: String,
    pub provider_epoch: Option<String>,
    pub active_generation: u64,
}

pub type CatalogSubjectAssignment = subject_projection::ProjectedSubjectAssignment;

pub fn prepare_catalog_subject_assignments(subjects: &[BookSubject]) -> Vec<CatalogSubjectAssignment> {
    if subjects.is_empty() {
        return Vec::new();
    }
    subject_projection::project_subjects(subjects).into_iter().chain(subject_projection::project_unified_subjects(subjects)).collect()
}

mod pages;
mod book_writes;
mod books;
mod schema;
mod sources;
mod subjects;
pub(crate) use pages::decode_page;
pub use pages::{allocate_sequence, decode_cached, optional_stored_u64, book_key, put_page_row, sequence_in, sql_decode_error, storage_i64, stored_u64};
pub use book_writes::upsert_book_with_sequence;
pub use books::{book_format_storage, catalog_book_format_from_storage, book_entry_id, book_extraction, book_inventory, replace_book_metadata};
pub use schema::{advance_catalog_transport_revision, unix_timestamp};
pub use sources::{active_generation_in, active_generation_optional, clear_source_rows, ensure_source_in, next_sequence_in, refresh_active_counts, source_state_in};
pub use subjects::{replace_feed_book_subjects_prepared, replace_book_subjects, replace_book_subjects_prepared, replace_book_toc};

#[cfg(feature = "server-extension")]
#[doc(hidden)]
pub mod server_extension {
    pub use super::subjects::{insert_prepared_subject_assignments, insert_subject_browse_memberships, rebuild_subject_assignments, ENRICHED_SUBJECT_POSITION_BASE};
    pub use super::{
        active_generation_in, active_generation_optional, advance_catalog_transport_revision, allocate_sequence, book_format_storage, catalog_book_format_from_storage, clear_source_rows, decode_cached, ensure_source_in, next_sequence_in,
        optional_stored_u64, book_entry_id, book_extraction, book_inventory, book_key, put_page_row, refresh_active_counts, replace_feed_book_subjects_prepared, replace_book_metadata,
        replace_book_subjects, replace_book_subjects_prepared, replace_book_toc, sequence_in, source_state_in, sql_decode_error, storage_i64, stored_u64, unix_timestamp, CatalogServerConnection,
        CATALOG_CACHE_FILE_VERSION, CATALOG_CACHE_SCHEMA_VERSION, CATALOG_SEARCH_INDEX_VERSION, CATALOG_SUBJECT_BROWSE_INDEX_VERSION,
    };
}
