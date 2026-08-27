use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use bzip2::read::MultiBzDecoder;
use flate2::read::MultiGzDecoder;
use metadata_contract::{
    AuthorIdentifier, AuthorMetadata, AuthorSource, Classification, ClassificationRequest, ClassificationResponse, ClassificationScheme, ClassificationSource, EditionIdentityMatch, EditionIdentityQuery, EditionIdentityRequest,
    EditionIdentityResponse, EditionIdentityResult, EditionIdentityStatus, EditionTitleMatch, EntityReference, EntitySource, IsbnClassificationResult, IsbnMetadataResult, LookupStatus, MetadataEnrichmentRequest, MetadataEnrichmentResponse,
    PersonEntity, PlaceEntity, RichIsbnMetadataResult, RichMetadataEnrichmentResponse, RichWorkMetadataMatch, SnapshotDescription, SubjectHeading, WorkClassificationMatch, WorkMetadataMatch, MAX_EDITION_QUERIES_PER_REQUEST,
    MAX_ISBNS_PER_REQUEST, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES,
};
use r2d2::{Pool, PooledConnection};
use r2d2_sqlite::SqliteConnectionManager;
use rayon::prelude::*;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::Semaphore;
use unicode_normalization::UnicodeNormalization;

const SCHEMA_VERSION: i64 = 8;
const WIKIDATA_SCHEMA_VERSION: i64 = 9;
const LEGACY_QUERY_SCHEMA_VERSION: i64 = 7;
const BUILDER_SCHEMA_VERSION: i64 = 9;
const DESCRIPTION_SCHEMA_VERSION: i64 = 2;
const DESCRIPTION_BUILDER_VERSION: i64 = 2;
const AUTHOR_PROFILE_SCHEMA_VERSION: i64 = 1;
const AUTHOR_PROFILE_BUILDER_VERSION: i64 = 1;
const MAX_REVERSE_TITLE_CANDIDATES: usize = 4_096;
// One row beyond the public candidate ceiling is enough to prove that the
// result is truncated without allowing a broad title to build an unbounded
// intermediate result.
const MAX_RAW_TITLE_CANDIDATES: usize = MAX_REVERSE_TITLE_CANDIDATES + 1;
const MAX_EDITION_IDENTITY_REVIEW_CANDIDATES: usize = 256;
const MAX_TITLE_SUBSTRINGS_PER_QUERY: usize = 64;
const OL_PLACE_KIND: i64 = 1;
const OL_PERSON_KIND: i64 = 2;
const DDC_SCHEME: i64 = 1;
const LCC_SCHEME: i64 = 2;
const MAX_NOTATION_BYTES: usize = 256;
const MAX_DESCRIPTION_BYTES: usize = 16 * 1024;
const MAX_AUTHOR_BIOGRAPHY_BYTES: usize = 32 * 1024;
const OPEN_LIBRARY_CHECKPOINT_RECORDS: usize = 250_000;
const WIKIDATA_CHECKPOINT_RECORDS: usize = 20_000;
const IMPORT_CACHE_KIB: usize = 1_048_576;
const DESCRIPTION_IMPORT_CACHE_KIB: usize = 131_072;
const PIPELINE_PARSE_RECORDS: usize = 25_000;
const WIKIDATA_PIPELINE_PARSE_RECORDS: usize = 5_000;
const PIPELINE_BUFFERED_BATCHES: usize = 1;
const QUERY_CONCURRENCY: u32 = 4;
const QUERY_EXECUTION_TIMEOUT: Duration = Duration::from_secs(8);
// Check often enough to retain the query deadline without adding noticeable
// callback overhead to large enrichment batches.
const QUERY_PROGRESS_OPS: i32 = 1_000;
const READ_CACHE_KIB: usize = 16_384;
const READ_MMAP_BYTES: usize = 67_108_864;
const BULK_EVIDENCE_IDS_PER_QUERY: usize = 30_000;

#[derive(Clone)]
pub struct MetadataService {
    state: Arc<ServiceState>,
    library_of_congress: Option<library_of_congress::LibraryOfCongressClient>,
}

struct ServiceState {
    database: PathBuf,
    pool: Pool<SqliteConnectionManager>,
    description_pool: Option<Pool<SqliteConnectionManager>>,
    author_profile_pool: Option<Pool<SqliteConnectionManager>>,
    query_slots: Arc<Semaphore>,
    snapshot: SnapshotDescription,
    schema_version: i64,
    descriptions_available: bool,
    started_at: Instant,
    requests: AtomicU64,
    errors: AtomicU64,
    total_duration_ms: AtomicU64,
    processed_items: AtomicU64,
    item_processing_duration_ms: AtomicU64,
    edition_identities: EndpointMetrics,
    classifications: EndpointMetrics,
    enrichment: EndpointMetrics,
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

#[derive(Serialize)]
struct OperationalStats {
    status: &'static str,
    uptime_seconds: u64,
    schema_version: i64,
    dump_date: String,
    imported_at_ms: u64,
    database_bytes: u64,
    edition_records: u64,
    work_records: u64,
    author_records: u64,
    requests: u64,
    errors: u64,
    average_duration_ms: f64,
    processed_items: u64,
    average_duration_ms_per_item: f64,
    items_per_processing_second: f64,
    query_concurrency: u32,
    pool_connections: u32,
    pool_idle_connections: u32,
    metrics: BTreeMap<String, i64>,
    endpoint_metrics: BTreeMap<&'static str, EndpointMetricsSnapshot>,
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
    dewey_decimal_class: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_string_array")]
    lc_classifications: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_string_array")]
    subject_places: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_string_array")]
    subject_people: Vec<String>,
}

#[derive(Default, Deserialize)]
struct WorkRecord {
    key: String,
    #[serde(default, deserialize_with = "deserialize_optional_string")]
    title: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_description")]
    description: Option<String>,
    #[serde(default, deserialize_with = "deserialize_work_author_references")]
    authors: Vec<KeyReference>,
    #[serde(default, deserialize_with = "deserialize_string_array")]
    dewey_number: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_string_array")]
    lc_classifications: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_string_array")]
    subject_places: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_string_array")]
    subject_people: Vec<String>,
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

#[derive(Default, Deserialize)]
struct AuthorProfileRecord {
    key: String,
    #[serde(default, deserialize_with = "deserialize_optional_description")]
    bio: Option<String>,
}

#[derive(Default, Deserialize)]
struct WikidataRecord {
    id: String,
    #[serde(default)]
    labels: BTreeMap<String, WikidataLocalizedValue>,
    #[serde(default)]
    aliases: BTreeMap<String, Vec<WikidataLocalizedValue>>,
    #[serde(default)]
    claims: WikidataClaims,
}

#[derive(Deserialize)]
struct WikidataLocalizedValue {
    value: String,
}

#[derive(Default, Deserialize)]
struct WikidataClaims {
    #[serde(default, rename = "P31")]
    instances: Vec<WikidataClaim>,
    #[serde(default, rename = "P279")]
    subclasses: Vec<WikidataClaim>,
    #[serde(default, rename = "P131")]
    parents: Vec<WikidataClaim>,
    #[serde(default, rename = "P17")]
    countries: Vec<WikidataClaim>,
    #[serde(default, rename = "P30")]
    continents: Vec<WikidataClaim>,
    #[serde(default, rename = "P625")]
    coordinates: Vec<WikidataClaim>,
    #[serde(default, rename = "P569")]
    births: Vec<WikidataClaim>,
    #[serde(default, rename = "P570")]
    deaths: Vec<WikidataClaim>,
}

#[derive(Deserialize)]
struct WikidataClaim {
    mainsnak: WikidataSnak,
}

#[derive(Deserialize)]
struct WikidataSnak {
    datavalue: Option<WikidataDataValue>,
}

#[derive(Deserialize)]
struct WikidataDataValue {
    value: WikidataClaimValue,
}

#[derive(Default, Deserialize)]
struct WikidataClaimValue {
    id: Option<String>,
    #[serde(rename = "numeric-id")]
    numeric_id: Option<i64>,
    latitude: Option<f64>,
    longitude: Option<f64>,
    time: Option<String>,
}

#[derive(Deserialize)]
struct KeyReference {
    key: String,
}

mod enrichment;
mod http;
mod identity;
mod import;
mod library_of_congress;
mod openlibrary;
mod query;
mod service;

pub(crate) use enrichment::{
    attach_authors, attach_rich_fields, classification_similarity_keys, collect_response_entities, load_author_biographies, load_embedded_work_descriptions, load_work_bisac_classifications, load_work_descriptions_from_table,
    load_work_popularities, lookup_metadata_one, lookup_one, sql_placeholders,
};
pub use http::app;
pub(crate) use identity::resolve_edition_batch;
pub use import::{add_popularity_to_snapshot, import_author_profile_snapshot, import_description_snapshot, import_rich_snapshot_with_popularity, import_snapshot, import_snapshot_with_popularity};
pub(crate) use openlibrary::{
    building_path, canonical_isbn13, deserialize_key_references, deserialize_optional_description, deserialize_optional_string, deserialize_string_array, deserialize_work_author_references, dump_date, error, open_description_pool,
    open_library_id, open_read_pool, valid_dump_date, valid_open_library_author_biography, valid_open_library_description,
};
pub(crate) use query::query_connection;

#[cfg(test)]
mod tests;
