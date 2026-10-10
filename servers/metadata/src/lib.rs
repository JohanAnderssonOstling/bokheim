use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use flate2::read::MultiGzDecoder;
use metadata_contract::{
    AuthorIdentifier, AuthorMetadata, AuthorSource, Classification, ClassificationScheme, ClassificationSource, EditionIdentityMatch, EditionIdentityQuery, EditionIdentityResult, EditionIdentityStatus, EditionTitleMatch,
    IsbnClassificationResult, LookupStatus, RichIsbnMetadataResult, RichWorkMetadataMatch, SnapshotDescription, SubjectHeading, WorkClassificationMatch, MAX_EDITION_QUERIES_PER_REQUEST,
    MAX_ISBNS_PER_REQUEST, MAX_REQUEST_BYTES,
};
use r2d2::{Pool, PooledConnection};
use r2d2_sqlite::SqliteConnectionManager;
use rayon::prelude::*;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const SCHEMA_VERSION: i64 = 1;
const BUILDER_SCHEMA_VERSION: i64 = 1;
const DESCRIPTION_SCHEMA_VERSION: i64 = 1;
const DESCRIPTION_BUILDER_VERSION: i64 = 3;
const MAX_REVERSE_TITLE_CANDIDATES: usize = 4_096;

// One row beyond the public candidate ceiling is enough to prove that the
// result is truncated without allowing a broad title to build an unbounded
// intermediate result.
const MAX_EDITION_IDENTITY_REVIEW_CANDIDATES: usize = 256;
const MAX_TITLE_SUBSTRINGS_PER_QUERY: usize = 64;
// Scheme 1 is reserved for legacy Dewey rows; readers ignore it.
const LCC_SCHEME: i64 = 2;
const MAX_NOTATION_BYTES: usize = 256;
const MAX_DESCRIPTION_BYTES: usize = 16 * 1024;
const OPEN_LIBRARY_CHECKPOINT_RECORDS: usize = 250_000;
const IMPORT_CACHE_KIB: usize = 1_048_576;
const DESCRIPTION_IMPORT_CACHE_KIB: usize = 131_072;
const PIPELINE_PARSE_RECORDS: usize = 25_000;
const PIPELINE_BUFFERED_BATCHES: usize = 1;
const QUERY_CONCURRENCY: u32 = 4;

// Check often enough to retain the query deadline without adding noticeable
// callback overhead to large enrichment batches.

const READ_CACHE_KIB: usize = 16_384;
const READ_MMAP_BYTES: usize = 67_108_864;
const BULK_EVIDENCE_IDS_PER_QUERY: usize = 30_000;

#[derive(Clone)]
pub(crate) struct Resolver {
    state: Arc<ResolverState>,
}
struct ResolverState {
    database: PathBuf,
    pool: Pool<SqliteConnectionManager>,
    description_pool: Option<Pool<SqliteConnectionManager>>,
    snapshot: SnapshotDescription,
}

#[derive(Default)]
struct EndpointMetrics {
    requests: AtomicU64,
    errors: AtomicU64,
    in_flight: AtomicU64,
    total_duration_ms: AtomicU64,
}

type QueryConnection = PooledConnection<SqliteConnectionManager>;

#[derive(Serialize)]
struct EndpointMetricsSnapshot {
    requests: u64,
    errors: u64,
    in_flight: u64,
    total_duration_ms: u64,
    average_duration_ms: f64,
}

#[derive(Debug)]
pub struct MetadataError(String);

impl fmt::Display for MetadataError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for MetadataError {}

#[derive(Default, Deserialize)]
struct EditionRecord {
    key: String,
    #[serde(default, deserialize_with = "deserialize_optional_string")]
    title: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_string")]
    subtitle: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_string")]
    publish_date: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_array")]
    publishers: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_key_references")]
    works: Vec<KeyReference>,
    #[serde(default, deserialize_with = "deserialize_key_references")]
    authors: Vec<KeyReference>,
    #[serde(default, deserialize_with = "deserialize_string_array")]
    isbn_10: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_string_array")]
    isbn_13: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_string_array")]
    lc_classifications: Vec<String>,
}

#[derive(Default, Deserialize)]
struct WorkRecord {
    key: String,
    #[serde(default, deserialize_with = "deserialize_optional_string")]
    title: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_string")]
    subtitle: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_description")]
    description: Option<String>,
    #[serde(default, deserialize_with = "deserialize_work_author_references")]
    authors: Vec<KeyReference>,
    #[serde(default, deserialize_with = "deserialize_string_array")]
    lc_classifications: Vec<String>,
}

#[derive(Default, Deserialize)]
struct DescriptionWorkRecord {
    key: String,
    #[serde(default, deserialize_with = "deserialize_optional_description")]
    description: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_array")]
    subjects: Vec<String>,
}

#[derive(Default, Deserialize)]
struct AuthorRecord {
    key: String,
    name: Option<String>,
    #[serde(default)]
    remote_ids: BTreeMap<String, serde_json::Value>,
}

#[derive(Deserialize)]
struct KeyReference {
    key: String,
}

mod query;

pub(crate) use build::enrichment::{attach_authors, attach_rich_fields, classification_similarity_keys, load_embedded_work_descriptions, load_work_bisac_classifications, load_work_descriptions_from_table, lookup_one, sql_placeholders};
pub use serve::http::app;
pub use build::import::{import_description_snapshot, import_snapshot};
pub(crate) use build::openlibrary::{
    building_path, deserialize_key_references, deserialize_optional_description, deserialize_optional_string, deserialize_string_array, deserialize_work_author_references, dump_date, error, open_description_pool,
    open_library_id, open_read_pool, valid_dump_date, valid_open_library_description,
};
pub(crate) use query::query_connection;
mod isbn;
pub(crate) use isbn::Isbn13;

#[cfg(test)]
pub(crate) use build::authority_ingestion::ingest_wikidata_authorities;

mod schema;
mod build;
pub use build::{build_answers, build_answers_with_ratings, import_authorities, BuildReport};
mod serve;
pub use serve::MetadataService;
mod crawl;
pub use crawl::crawl;

