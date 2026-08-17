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
    PersonEntity, PlaceEntity, RichIsbnMetadataResult, RichMetadataEnrichmentResponse, RichWorkMetadataMatch, SnapshotDescription, WorkClassificationMatch, WorkMetadataMatch, MAX_EDITION_QUERIES_PER_REQUEST, MAX_ISBNS_PER_REQUEST,
    MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES,
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
const MAX_REVERSE_TITLE_CANDIDATES: usize = 4_096;
const MAX_EDITION_IDENTITY_REVIEW_CANDIDATES: usize = 256;
const MAX_TITLE_SUBSTRINGS_PER_QUERY: usize = 64;
const OL_PLACE_KIND: i64 = 1;
const OL_PERSON_KIND: i64 = 2;
const DDC_SCHEME: i64 = 1;
const LCC_SCHEME: i64 = 2;
const MAX_NOTATION_BYTES: usize = 256;
const OPEN_LIBRARY_CHECKPOINT_RECORDS: usize = 250_000;
const WIKIDATA_CHECKPOINT_RECORDS: usize = 20_000;
const IMPORT_CACHE_KIB: usize = 1_048_576;
const PIPELINE_PARSE_RECORDS: usize = 25_000;
const WIKIDATA_PIPELINE_PARSE_RECORDS: usize = 5_000;
const PIPELINE_BUFFERED_BATCHES: usize = 1;
const QUERY_CONCURRENCY: u32 = 4;
const READ_CACHE_KIB: usize = 16_384;
const READ_MMAP_BYTES: usize = 67_108_864;
const BULK_EVIDENCE_IDS_PER_QUERY: usize = 30_000;

#[derive(Clone)]
pub struct MetadataService {
    state: Arc<ServiceState>,
}

struct ServiceState {
    database: PathBuf,
    pool: Pool<SqliteConnectionManager>,
    query_slots: Arc<Semaphore>,
    snapshot: SnapshotDescription,
    schema_version: i64,
    started_at: Instant,
    requests: AtomicU64,
    errors: AtomicU64,
    total_duration_ms: AtomicU64,
    processed_items: AtomicU64,
    item_processing_duration_ms: AtomicU64,
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
struct AuthorRecord {
    key: String,
    name: Option<String>,
    #[serde(default)]
    remote_ids: BTreeMap<String, serde_json::Value>,
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

impl MetadataService {
    pub fn open(database: impl Into<PathBuf>) -> Result<Self, MetadataError> {
        let database = database.into();
        let pool = open_read_pool(&database)?;
        let (schema_version, dump_date, imported_at_ms) = {
            let connection = pool.get().map_err(error)?;
            connection.query_row("SELECT schema_version,dump_date,imported_at_ms FROM snapshot WHERE singleton=1", (), |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?))).map_err(error)?
        };
        if !matches!(schema_version, LEGACY_QUERY_SCHEMA_VERSION | SCHEMA_VERSION | WIKIDATA_SCHEMA_VERSION) {
            return Err(MetadataError(format!("unsupported metadata database schema {schema_version}; expected {LEGACY_QUERY_SCHEMA_VERSION}, {SCHEMA_VERSION}, or {WIKIDATA_SCHEMA_VERSION}")));
        }
        let imported_at_ms = u64::try_from(imported_at_ms).map_err(|_| MetadataError("metadata snapshot has an invalid import timestamp".to_owned()))?;
        Ok(Self {
            state: Arc::new(ServiceState {
                database,
                pool,
                query_slots: Arc::new(Semaphore::new(QUERY_CONCURRENCY as usize)),
                snapshot: SnapshotDescription { dump_date, imported_at_ms },
                schema_version,
                started_at: Instant::now(),
                requests: AtomicU64::new(0),
                errors: AtomicU64::new(0),
                total_duration_ms: AtomicU64::new(0),
                processed_items: AtomicU64::new(0),
                item_processing_duration_ms: AtomicU64::new(0),
            }),
        })
    }

    pub fn lookup(&self, request: ClassificationRequest) -> Result<ClassificationResponse, MetadataError> {
        if request.isbns.is_empty() {
            return Err(MetadataError("at least one ISBN is required".to_owned()));
        }
        if request.isbns.len() > MAX_ISBNS_PER_REQUEST {
            return Err(MetadataError(format!("at most {MAX_ISBNS_PER_REQUEST} ISBNs may be requested")));
        }
        let connection = self.connection()?;
        let mut results = Vec::with_capacity(request.isbns.len());
        for requested_isbn in request.isbns {
            results.push(lookup_one(&connection, requested_isbn)?);
        }
        Ok(ClassificationResponse { snapshot: self.state.snapshot.clone(), results })
    }

    pub fn enrich(&self, request: MetadataEnrichmentRequest) -> Result<MetadataEnrichmentResponse, MetadataError> {
        validate_isbn_count(request.isbns.len())?;
        let connection = self.connection()?;
        let mut results = Vec::with_capacity(request.isbns.len());
        for requested_isbn in request.isbns {
            results.push(lookup_metadata_one(&connection, requested_isbn)?);
        }
        attach_authors(&connection, &mut results)?;
        Ok(MetadataEnrichmentResponse { snapshot: self.state.snapshot.clone(), results })
    }

    pub fn enrich_rich(&self, request: MetadataEnrichmentRequest) -> Result<RichMetadataEnrichmentResponse, MetadataError> {
        validate_isbn_count(request.isbns.len())?;
        let connection = self.connection()?;
        let mut metadata = Vec::with_capacity(request.isbns.len());
        for requested_isbn in request.isbns {
            metadata.push(lookup_metadata_one(&connection, requested_isbn)?);
        }
        attach_authors(&connection, &mut metadata)?;
        let results = attach_rich_fields(&connection, metadata)?;
        let (places, people) = collect_response_entities(&connection, &results)?;
        Ok(RichMetadataEnrichmentResponse { snapshot: self.state.snapshot.clone(), results, places, people })
    }

    pub fn classify_rich(&self, request: MetadataEnrichmentRequest) -> Result<RichMetadataEnrichmentResponse, MetadataError> {
        validate_isbn_count(request.isbns.len())?;
        let connection = self.connection()?;
        let mut classifications = Vec::with_capacity(request.isbns.len());
        for requested_isbn in request.isbns {
            classifications.push(lookup_one(&connection, requested_isbn)?);
        }
        let work_ids = classifications.iter().flat_map(|classification| &classification.matches).filter_map(|matched| matched.open_library_work_id.as_deref().and_then(|value| open_library_id(value, 'W'))).collect::<BTreeSet<_>>();
        let popularity = load_work_popularities(&connection, &work_ids)?;
        let mut results = Vec::with_capacity(classifications.len());
        for classification in classifications {
            let matches = classification
                .matches
                .into_iter()
                .map(|matched| {
                    let work_popularity = matched.open_library_work_id.as_deref().and_then(|value| open_library_id(value, 'W')).and_then(|work_id| popularity.get(&work_id).cloned());
                    RichWorkMetadataMatch {
                        open_library_work_id: matched.open_library_work_id,
                        exact_edition_ids: matched.exact_edition_ids,
                        classifications: matched.classifications,
                        authors: Vec::new(),
                        places: Vec::new(),
                        people: Vec::new(),
                        popularity: work_popularity,
                    }
                })
                .collect();
            results.push(RichIsbnMetadataResult { requested_isbn: classification.requested_isbn, canonical_isbn13: classification.canonical_isbn13, status: classification.status, matches });
        }
        Ok(RichMetadataEnrichmentResponse { snapshot: self.state.snapshot.clone(), results, places: Vec::new(), people: Vec::new() })
    }

    pub fn resolve_editions(&self, request: EditionIdentityRequest) -> Result<EditionIdentityResponse, MetadataError> {
        if request.queries.is_empty() || request.queries.len() > MAX_EDITION_QUERIES_PER_REQUEST {
            return Err(MetadataError(format!("between 1 and {MAX_EDITION_QUERIES_PER_REQUEST} edition queries are required")));
        }
        let connection = self.connection()?;
        let results = resolve_edition_batch(&connection, request.queries)?;
        Ok(EditionIdentityResponse { snapshot: self.state.snapshot.clone(), results })
    }

    fn connection(&self) -> Result<PooledConnection<SqliteConnectionManager>, MetadataError> {
        self.state.pool.get().map_err(error)
    }
}

async fn run_database<T, F>(service: MetadataService, operation: F) -> Result<Result<T, MetadataError>, tokio::task::JoinError>
where
    T: Send + 'static,
    F: FnOnce(&MetadataService) -> Result<T, MetadataError> + Send + 'static,
{
    let permit = match service.state.query_slots.clone().acquire_owned().await {
        Ok(permit) => permit,
        Err(_) => return Ok(Err(MetadataError("metadata query scheduler stopped".to_owned()))),
    };
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        operation(&service)
    })
    .await
}

pub fn app(service: MetadataService) -> Router {
    Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/internal/operations", get(operational_stats))
        .route("/v2/classifications", post(rich_classifications))
        .route("/v2/enrichment", post(rich_enrichment))
        .route("/v2/edition-identities", post(edition_identities))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES))
        .layer(middleware::from_fn_with_state(service.clone(), record_request_metrics))
        .with_state(service)
}

async fn record_request_metrics(State(service): State<MetadataService>, request: Request, next: Next) -> Response {
    let tracked = matches!(request.uri().path(), "/v2/classifications" | "/v2/enrichment" | "/v2/edition-identities");
    let started = Instant::now();
    let response = next.run(request).await;
    if tracked {
        service.state.requests.fetch_add(1, Ordering::Relaxed);
        if !response.status().is_success() {
            service.state.errors.fetch_add(1, Ordering::Relaxed);
        }
        let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        service.state.total_duration_ms.fetch_add(duration_ms, Ordering::Relaxed);
    }
    response
}

async fn operational_stats(State(service): State<MetadataService>) -> Response {
    match run_database(service, |service| load_operational_stats(&service.state)).await {
        Ok(Ok(stats)) => {
            let mut response = axum::Json(stats).into_response();
            response.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            response.headers_mut().insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
            response
        }
        Ok(Err(error)) => api_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()),
        Err(error) => api_error(StatusCode::INTERNAL_SERVER_ERROR, &format!("statistics worker failed: {error}")),
    }
}

fn load_operational_stats(state: &ServiceState) -> Result<OperationalStats, MetadataError> {
    let connection = state.pool.get().map_err(error)?;
    let (edition_records, work_records, author_records): (i64, i64, i64) =
        connection.query_row("SELECT edition_records,work_records,author_records FROM snapshot WHERE singleton=1", (), |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).map_err(error)?;
    let mut metrics = BTreeMap::new();
    if let Ok(mut statement) = connection.prepare("SELECT name,value FROM metadata_metric ORDER BY name") {
        if let Ok(rows) = statement.query_map((), |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))) {
            for row in rows.flatten() {
                metrics.insert(row.0, row.1);
            }
        }
    }
    let requests = state.requests.load(Ordering::Relaxed);
    let total_duration_ms = state.total_duration_ms.load(Ordering::Relaxed);
    let processed_items = state.processed_items.load(Ordering::Relaxed);
    let item_processing_duration_ms = state.item_processing_duration_ms.load(Ordering::Relaxed);
    let pool_state = state.pool.state();
    Ok(OperationalStats {
        status: "available",
        uptime_seconds: state.started_at.elapsed().as_secs(),
        schema_version: state.schema_version,
        dump_date: state.snapshot.dump_date.clone(),
        imported_at_ms: state.snapshot.imported_at_ms,
        database_bytes: fs::metadata(&state.database).map_err(error)?.len(),
        edition_records: u64::try_from(edition_records).unwrap_or_default(),
        work_records: u64::try_from(work_records).unwrap_or_default(),
        author_records: u64::try_from(author_records).unwrap_or_default(),
        requests,
        errors: state.errors.load(Ordering::Relaxed),
        average_duration_ms: if requests == 0 { 0.0 } else { total_duration_ms as f64 / requests as f64 },
        processed_items,
        average_duration_ms_per_item: if processed_items == 0 { 0.0 } else { item_processing_duration_ms as f64 / processed_items as f64 },
        items_per_processing_second: if item_processing_duration_ms == 0 { 0.0 } else { processed_items as f64 * 1_000.0 / item_processing_duration_ms as f64 },
        query_concurrency: QUERY_CONCURRENCY,
        pool_connections: pool_state.connections,
        pool_idle_connections: pool_state.idle_connections,
        metrics,
    })
}

async fn edition_identities(State(service): State<MetadataService>, headers: HeaderMap, body: Bytes) -> Response {
    let content_type = headers.get(header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).unwrap_or_default();
    let json = content_type.split(';').next().is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"));
    let binary = metadata_contract::is_binary_media_type_v2(content_type);
    let decoded = match (json, binary) {
        (true, _) => serde_json::from_slice(&body).map_err(|error| MetadataError(format!("invalid JSON request: {error}"))),
        (_, true) => metadata_contract::decode_v2_edition_identity_request(&body, MAX_REQUEST_BYTES).map_err(|error| MetadataError(error.to_string())),
        _ => return api_error(StatusCode::UNSUPPORTED_MEDIA_TYPE, "use application/json or application/vnd.bokheim.metadata+protobuf; version=2"),
    };
    let request = match decoded {
        Ok(request) => request,
        Err(error) => return api_error(StatusCode::BAD_REQUEST, &error.to_string()),
    };
    let item_count = u64::try_from(request.queries.len()).unwrap_or(u64::MAX);
    let metrics = service.state.clone();
    let processing_started = Instant::now();
    let result = match run_database(service, move |service| service.resolve_editions(request)).await {
        Ok(Ok(response)) => response,
        Ok(Err(error)) => return api_error(StatusCode::BAD_REQUEST, &error.to_string()),
        Err(error) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, &format!("lookup worker failed: {error}")),
    };
    record_processed_items(&metrics, item_count, processing_started.elapsed());
    let encoded = if binary { metadata_contract::encode_v2_edition_identity_response(&result) } else { serde_json::to_vec(&result).map_err(|error| metadata_contract::WireError::Encode(error.to_string())) };
    match encoded {
        Ok(encoded) if encoded.len() <= MAX_RESPONSE_BYTES => {
            let mut response = encoded.into_response();
            response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static(if binary { metadata_contract::MEDIA_TYPE_V2 } else { "application/json" }));
            response.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("public, max-age=86400"));
            response
        }
        Ok(_) => api_error(StatusCode::INTERNAL_SERVER_ERROR, "encoded response exceeded the response limit"),
        Err(error) => api_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()),
    }
}

fn validate_isbn_count(count: usize) -> Result<(), MetadataError> {
    if count == 0 {
        return Err(MetadataError("at least one ISBN is required".to_owned()));
    }
    if count > MAX_ISBNS_PER_REQUEST {
        return Err(MetadataError(format!("at most {MAX_ISBNS_PER_REQUEST} ISBNs may be requested")));
    }
    Ok(())
}

async fn rich_classifications(State(service): State<MetadataService>, headers: HeaderMap, body: Bytes) -> Response {
    let binary = headers.get(header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).is_some_and(metadata_contract::is_binary_media_type_v2);
    let request = if binary {
        metadata_contract::decode_v2_request(&body, MAX_REQUEST_BYTES).map_err(|error| MetadataError(error.to_string()))
    } else if headers.get(header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).is_some_and(|value| value.split(';').next().is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"))) {
        serde_json::from_slice(&body).map_err(|error| MetadataError(format!("invalid JSON request: {error}")))
    } else {
        return api_error(StatusCode::UNSUPPORTED_MEDIA_TYPE, "use application/json or application/vnd.bokheim.metadata+protobuf; version=2");
    };
    let request = match request {
        Ok(request) => request,
        Err(error) => return api_error(StatusCode::BAD_REQUEST, &error.to_string()),
    };
    let item_count = u64::try_from(request.isbns.len()).unwrap_or(u64::MAX);
    let metrics = service.state.clone();
    let processing_started = Instant::now();
    let result = match run_database(service, move |service| service.classify_rich(request)).await {
        Ok(Ok(response)) => response,
        Ok(Err(error)) => return api_error(StatusCode::BAD_REQUEST, &error.to_string()),
        Err(error) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, &format!("lookup worker failed: {error}")),
    };
    record_processed_items(&metrics, item_count, processing_started.elapsed());
    let (content_type, encoded) = if binary {
        match metadata_contract::encode_v2_response(&result) {
            Ok(encoded) => (metadata_contract::MEDIA_TYPE_V2, encoded),
            Err(error) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()),
        }
    } else {
        match serde_json::to_vec(&result) {
            Ok(encoded) => ("application/json", encoded),
            Err(error) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()),
        }
    };
    if encoded.len() > MAX_RESPONSE_BYTES {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "encoded response exceeded the response limit");
    }
    let mut response = encoded.into_response();
    response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    response.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("public, max-age=86400"));
    response
}

async fn rich_enrichment(State(service): State<MetadataService>, headers: HeaderMap, body: Bytes) -> Response {
    let binary = headers.get(header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).is_some_and(metadata_contract::is_binary_media_type_v2);
    let request = if binary {
        metadata_contract::decode_v2_request(&body, MAX_REQUEST_BYTES).map_err(|error| MetadataError(error.to_string()))
    } else if headers.get(header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).is_some_and(|value| value.split(';').next().is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"))) {
        serde_json::from_slice(&body).map_err(|error| MetadataError(format!("invalid JSON request: {error}")))
    } else {
        return api_error(StatusCode::UNSUPPORTED_MEDIA_TYPE, "use application/json or application/vnd.bokheim.metadata+protobuf; version=2");
    };
    let request = match request {
        Ok(request) => request,
        Err(error) => return api_error(StatusCode::BAD_REQUEST, &error.to_string()),
    };
    let item_count = u64::try_from(request.isbns.len()).unwrap_or(u64::MAX);
    let metrics = service.state.clone();
    let processing_started = Instant::now();
    let result = match run_database(service, move |service| service.enrich_rich(request)).await {
        Ok(Ok(response)) => response,
        Ok(Err(error)) => return api_error(StatusCode::BAD_REQUEST, &error.to_string()),
        Err(error) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, &format!("lookup worker failed: {error}")),
    };
    record_processed_items(&metrics, item_count, processing_started.elapsed());
    let (content_type, encoded) = if binary {
        match metadata_contract::encode_v2_response(&result) {
            Ok(encoded) => (metadata_contract::MEDIA_TYPE_V2, encoded),
            Err(error) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()),
        }
    } else {
        match serde_json::to_vec(&result) {
            Ok(encoded) => ("application/json", encoded),
            Err(error) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()),
        }
    };
    if encoded.len() > MAX_RESPONSE_BYTES {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "encoded response exceeded the response limit");
    }
    let mut response = encoded.into_response();
    response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    response.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("public, max-age=86400"));
    response
}

fn record_processed_items(state: &ServiceState, count: u64, duration: Duration) {
    state.processed_items.fetch_add(count, Ordering::Relaxed);
    state.item_processing_duration_ms.fetch_add(u64::try_from(duration.as_millis()).unwrap_or(u64::MAX), Ordering::Relaxed);
}

fn api_error(status: StatusCode, message: &str) -> Response {
    (status, [(header::CONTENT_TYPE, "text/plain; charset=utf-8")], message.to_owned()).into_response()
}

pub fn import_snapshot(editions_path: &Path, works_path: &Path, authors_path: &Path, output_path: &Path, record_limit: Option<usize>) -> Result<(), MetadataError> {
    import_snapshot_inner(editions_path, works_path, authors_path, None, None, output_path, record_limit)
}

pub fn import_snapshot_with_popularity(editions_path: &Path, works_path: &Path, authors_path: &Path, ratings_path: &Path, reading_log_path: &Path, output_path: &Path, record_limit: Option<usize>) -> Result<(), MetadataError> {
    import_snapshot_inner(editions_path, works_path, authors_path, None, Some((ratings_path, reading_log_path)), output_path, record_limit)
}

/// Creates a new schema-v8 snapshot from an immutable bibliography snapshot
/// while streaming only the ratings and reading-log dumps. The source is never
/// opened for writing, so this can safely run beside the serving process.
pub fn add_popularity_to_snapshot(source_path: &Path, ratings_path: &Path, reading_log_path: &Path, output_path: &Path, record_limit: Option<usize>) -> Result<(), MetadataError> {
    if output_path.exists() {
        return Err(MetadataError(format!("output already exists: {}", output_path.display())));
    }
    let building_path = building_path(output_path);
    if building_path.exists() {
        return Err(MetadataError(format!("unfinished popularity snapshot already exists: {}", building_path.display())));
    }
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).map_err(error)?;
    }
    fs::copy(source_path, &building_path).map_err(error)?;
    let result = (|| {
        let mut connection = Connection::open(&building_path).map_err(error)?;
        configure_import(&connection)?;
        let source_schema = connection.query_row("SELECT schema_version FROM snapshot WHERE singleton=1", (), |row| row.get::<_, i64>(0)).map_err(error)?;
        if !matches!(source_schema, 7 | SCHEMA_VERSION | WIKIDATA_SCHEMA_VERSION) {
            return Err(MetadataError(format!("cannot add popularity to metadata schema {source_schema}; expected 7, {SCHEMA_VERSION}, or {WIKIDATA_SCHEMA_VERSION}")));
        }
        let has_wikidata = connection.query_row("SELECT EXISTS(SELECT 1 FROM entity LIMIT 1)", (), |row| row.get::<_, bool>(0)).map_err(error)?;
        connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS work_popularity(
                    work_id INTEGER PRIMARY KEY,
                    want_to_read_count INTEGER NOT NULL DEFAULT 0,
                    currently_reading_count INTEGER NOT NULL DEFAULT 0,
                    already_read_count INTEGER NOT NULL DEFAULT 0,
                    rating_count INTEGER NOT NULL DEFAULT 0,
                    rating_sum INTEGER NOT NULL DEFAULT 0
                 );
                 DELETE FROM import_checkpoint WHERE phase IN ('ratings','reading_log','snapshot_finalize');",
            )
            .map_err(error)?;
        import_work_popularity(&mut connection, ratings_path, reading_log_path, record_limit)?;
        let imported_at_ms = i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).map_err(error)?.as_millis()).map_err(error)?;
        let output_schema = if has_wikidata { WIKIDATA_SCHEMA_VERSION } else { SCHEMA_VERSION };
        connection.execute("UPDATE snapshot SET schema_version=?1,imported_at_ms=?2 WHERE singleton=1", params![output_schema, imported_at_ms]).map_err(error)?;
        connection.execute("INSERT OR REPLACE INTO metadata_metric(name,value) VALUES('core_open_library_popularity',1)", ()).map_err(error)?;
        connection.execute_batch("VACUUM; ANALYZE; PRAGMA optimize;").map_err(error)?;
        let integrity: String = connection.query_row("PRAGMA integrity_check", (), |row| row.get(0)).map_err(error)?;
        if integrity != "ok" {
            return Err(MetadataError(format!("metadata database integrity check failed: {integrity}")));
        }
        connection.execute("INSERT INTO import_checkpoint(phase,records_read,completed) VALUES('snapshot_finalize',0,1) ON CONFLICT(phase) DO UPDATE SET records_read=0,completed=1", ()).map_err(error)?;
        connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA journal_mode=DELETE;").map_err(error)?;
        Ok(())
    })();
    if let Err(failure) = result {
        return Err(failure);
    }
    fs::rename(&building_path, output_path).map_err(error)?;
    Ok(())
}

pub fn import_rich_snapshot_with_popularity(
    editions_path: &Path, works_path: &Path, authors_path: &Path, wikidata_path: &Path, ratings_path: &Path, reading_log_path: &Path, output_path: &Path, record_limit: Option<usize>,
) -> Result<(), MetadataError> {
    import_snapshot_inner(editions_path, works_path, authors_path, Some(wikidata_path), Some((ratings_path, reading_log_path)), output_path, record_limit)
}

fn import_snapshot_inner(editions_path: &Path, works_path: &Path, authors_path: &Path, wikidata_path: Option<&Path>, popularity_paths: Option<(&Path, &Path)>, output_path: &Path, record_limit: Option<usize>) -> Result<(), MetadataError> {
    if output_path.exists() {
        return Err(MetadataError(format!("output already exists: {}", output_path.display())));
    }
    let building_path = building_path(output_path);
    let resuming = building_path.exists();
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).map_err(error)?;
    }
    let mut connection = Connection::open(&building_path).map_err(error)?;
    configure_import(&connection)?;
    if resuming {
        validate_import_inputs(&connection, &building_path, editions_path, works_path, authors_path, wikidata_path, popularity_paths, record_limit)?;
        println!("resuming metadata snapshot build {}", building_path.display());
    } else {
        let transaction = connection.transaction().map_err(error)?;
        create_schema(&transaction)?;
        register_import_inputs(&transaction, editions_path, works_path, authors_path, wikidata_path, popularity_paths, record_limit)?;
        transaction.commit().map_err(error)?;
    }
    let result = (|| {
        let edition_count = import_editions(&mut connection, editions_path, record_limit)?;
        import_edition_identities(&mut connection, editions_path, record_limit)?;
        connection.execute("CREATE INDEX IF NOT EXISTS edition_by_work ON edition(work_id)", ()).map_err(error)?;
        connection
            .execute(
                "DELETE FROM edition_work_classification
                 WHERE NOT EXISTS (
                     SELECT 1 FROM edition
                     WHERE edition.work_id=edition_work_classification.work_id
                 )",
                (),
            )
            .map_err(error)?;
        let work_count = import_works(&mut connection, works_path, record_limit)?;
        import_work_titles(&mut connection, works_path, record_limit)?;
        if let Some((ratings_path, reading_log_path)) = popularity_paths {
            import_work_popularity(&mut connection, ratings_path, reading_log_path, record_limit)?;
        }
        connection.execute_batch("CREATE INDEX IF NOT EXISTS edition_author_by_author ON edition_author(author_id); CREATE INDEX IF NOT EXISTS work_author_by_author ON work_author(author_id);").map_err(error)?;
        let author_count = import_authors(&mut connection, authors_path, record_limit)?;
        if let Some(wikidata_path) = wikidata_path {
            import_wikidata(&mut connection, wikidata_path, record_limit)?;
        }
        // `edition_by_work` is also required by the work-title branch of the
        // edition-identity lookup, so it is part of the shipped read schema.
        connection.execute_batch("DROP INDEX IF EXISTS edition_author_by_author; DROP INDEX IF EXISTS work_author_by_author;").map_err(error)?;
        if completed_phase(&connection, "snapshot_finalize")?.is_none() {
            let dump_date = dump_date(editions_path).or_else(|| dump_date(works_path)).or_else(|| dump_date(authors_path)).unwrap_or_else(|| "unknown".to_owned());
            let imported_at_ms = SystemTime::now().duration_since(UNIX_EPOCH).map_err(error)?.as_millis();
            let imported_at_ms = i64::try_from(imported_at_ms).map_err(error)?;
            let snapshot_schema_version = if wikidata_path.is_some() { WIKIDATA_SCHEMA_VERSION } else { SCHEMA_VERSION };
            connection
                .execute(
                    "INSERT OR REPLACE INTO snapshot(singleton,schema_version,dump_date,imported_at_ms,edition_records,work_records,author_records) VALUES(1,?1,?2,?3,?4,?5,?6)",
                    params![snapshot_schema_version, dump_date, imported_at_ms, edition_count, work_count, author_count],
                )
                .map_err(error)?;
            connection.execute_batch("VACUUM; ANALYZE; PRAGMA optimize;").map_err(error)?;
            let integrity: String = connection.query_row("PRAGMA integrity_check", (), |row| row.get(0)).map_err(error)?;
            if integrity != "ok" {
                return Err(MetadataError(format!("metadata database integrity check failed: {integrity}")));
            }
            connection.execute("INSERT INTO import_checkpoint(phase,records_read,completed) VALUES('snapshot_finalize',0,1) ON CONFLICT(phase) DO UPDATE SET completed=1", ()).map_err(error)?;
        }
        connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA journal_mode=DELETE;").map_err(error)?;
        Ok(())
    })();
    drop(connection);
    result?;
    fs::rename(&building_path, output_path).map_err(error)?;
    let size = fs::metadata(output_path).map_err(error)?.len();
    println!("created metadata snapshot {} ({size} bytes)", output_path.display());
    Ok(())
}

fn completed_phase(connection: &Connection, phase: &str) -> Result<Option<i64>, MetadataError> {
    connection.query_row("SELECT records_read FROM import_checkpoint WHERE phase=?1 AND completed=1", [phase], |row| row.get(0)).optional().map_err(error)
}

fn register_import_inputs(connection: &Connection, editions: &Path, works: &Path, authors: &Path, wikidata: Option<&Path>, popularity: Option<(&Path, &Path)>, record_limit: Option<usize>) -> Result<(), MetadataError> {
    connection.execute("INSERT INTO import_setting(singleton,record_limit,rich) VALUES(1,?1,?2)", params![record_limit.map(i64::try_from).transpose().map_err(error)?, i64::from(wikidata.is_some())]).map_err(error)?;
    for (name, path) in [("editions", Some(editions)), ("works", Some(works)), ("authors", Some(authors)), ("wikidata", wikidata), ("ratings", popularity.map(|value| value.0)), ("reading_log", popularity.map(|value| value.1))] {
        if let Some(path) = path {
            let (path, size_bytes, modified_ms) = import_input_identity(path)?;
            connection.execute("INSERT INTO import_input(name,path,size_bytes,modified_ms) VALUES(?1,?2,?3,?4)", params![name, path, size_bytes, modified_ms]).map_err(error)?;
        }
    }
    Ok(())
}

fn validate_import_inputs(connection: &Connection, building: &Path, editions: &Path, works: &Path, authors: &Path, wikidata: Option<&Path>, popularity: Option<(&Path, &Path)>, record_limit: Option<usize>) -> Result<(), MetadataError> {
    let builder_version = connection.query_row("SELECT value FROM metadata_metric WHERE name='builder_schema_version'", (), |row| row.get::<_, i64>(0)).optional().map_err(error)?;
    if builder_version != Some(SCHEMA_VERSION) {
        return Err(MetadataError(format!(
            "unfinished snapshot {} uses builder schema {}; current schema v{SCHEMA_VERSION} must be rebuilt from its pinned inputs after the stale building file is removed",
            building.display(),
            builder_version.map_or_else(|| "unknown".to_owned(), |version| format!("v{version}")),
        )));
    }
    let expected_limit = record_limit.map(i64::try_from).transpose().map_err(error)?;
    let settings = connection
        .query_row("SELECT record_limit,rich FROM import_setting WHERE singleton=1", (), |row| Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, bool>(1)?)))
        .map_err(|failure| MetadataError(format!("unfinished snapshot cannot be resumed (missing compatible checkpoint schema): {failure}")))?;
    if settings != (expected_limit, wikidata.is_some()) {
        return Err(MetadataError("unfinished snapshot was created with a different record limit or import mode".to_owned()));
    }
    for (name, path) in [("editions", Some(editions)), ("works", Some(works)), ("authors", Some(authors)), ("wikidata", wikidata), ("ratings", popularity.map(|value| value.0)), ("reading_log", popularity.map(|value| value.1))] {
        let Some(path) = path else { continue };
        let identity = import_input_identity(path)?;
        let stored = connection.query_row("SELECT path,size_bytes,modified_ms FROM import_input WHERE name=?1", [name], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?))).optional().map_err(error)?;
        if stored.as_ref() != Some(&identity) {
            return Err(MetadataError(format!("cannot resume: {name} dump path, size, or modification time changed")));
        }
    }
    Ok(())
}

fn import_input_identity(path: &Path) -> Result<(String, i64, i64), MetadataError> {
    let canonical = fs::canonicalize(path).map_err(error)?;
    let metadata = fs::metadata(&canonical).map_err(error)?;
    let size_bytes = i64::try_from(metadata.len()).map_err(error)?;
    let modified_ms = metadata.modified().map_err(error)?.duration_since(UNIX_EPOCH).map_err(error)?.as_millis();
    Ok((canonical.to_string_lossy().into_owned(), size_bytes, i64::try_from(modified_ms).map_err(error)?))
}

fn configure_import(connection: &Connection) -> Result<(), MetadataError> {
    connection.execute_batch(&format!("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA temp_store=FILE; PRAGMA cache_size=-{IMPORT_CACHE_KIB}; PRAGMA locking_mode=EXCLUSIVE;")).map_err(error)
}

fn create_schema(connection: &Connection) -> Result<(), MetadataError> {
    connection
        .execute_batch(
            "CREATE TABLE snapshot(
                singleton INTEGER PRIMARY KEY CHECK(singleton=1),
                schema_version INTEGER NOT NULL,
                dump_date TEXT NOT NULL,
                imported_at_ms INTEGER NOT NULL,
                edition_records INTEGER NOT NULL,
                work_records INTEGER NOT NULL,
                author_records INTEGER NOT NULL
             ) STRICT;
             CREATE TABLE import_setting(
                singleton INTEGER PRIMARY KEY CHECK(singleton=1),
                record_limit INTEGER,
                rich INTEGER NOT NULL CHECK(rich IN (0,1))
             ) STRICT;
             CREATE TABLE import_input(
                name TEXT PRIMARY KEY,
                path TEXT NOT NULL,
                size_bytes INTEGER NOT NULL,
                modified_ms INTEGER NOT NULL
             ) WITHOUT ROWID;
             CREATE TABLE import_checkpoint(
                phase TEXT PRIMARY KEY,
                records_read INTEGER NOT NULL DEFAULT 0,
                completed INTEGER NOT NULL DEFAULT 0 CHECK(completed IN (0,1))
             ) WITHOUT ROWID;
             CREATE TABLE metadata_metric(
                name TEXT PRIMARY KEY,
                value INTEGER NOT NULL
             ) WITHOUT ROWID;
             CREATE TABLE edition(
                edition_id INTEGER PRIMARY KEY,
                work_id INTEGER
             ) STRICT;
             CREATE TABLE edition_isbn(
                isbn13 INTEGER NOT NULL,
                edition_id INTEGER NOT NULL,
                PRIMARY KEY(isbn13,edition_id)
             ) WITHOUT ROWID;
             CREATE INDEX edition_isbn_by_edition ON edition_isbn(edition_id,isbn13);
             CREATE TABLE edition_bibliography(
                edition_id INTEGER PRIMARY KEY,
                title TEXT NOT NULL,
                normalized_title TEXT NOT NULL,
                publication_year INTEGER
             ) STRICT;
             CREATE INDEX edition_bibliography_by_title ON edition_bibliography(normalized_title,edition_id);
             CREATE TABLE work_bibliography(
                work_id INTEGER PRIMARY KEY,
                title TEXT NOT NULL,
                normalized_title TEXT NOT NULL
             ) STRICT;
             CREATE INDEX work_bibliography_by_title ON work_bibliography(normalized_title,work_id);
             CREATE TABLE edition_publisher(
                edition_id INTEGER NOT NULL,
                position INTEGER NOT NULL,
                name TEXT NOT NULL,
                normalized_name TEXT NOT NULL,
                PRIMARY KEY(edition_id,position)
             ) WITHOUT ROWID;
             CREATE TABLE edition_classification(
                edition_id INTEGER NOT NULL,
                scheme INTEGER NOT NULL CHECK(scheme IN (1,2)),
                notation TEXT NOT NULL,
                PRIMARY KEY(edition_id,scheme,notation)
             ) WITHOUT ROWID;
             CREATE TABLE edition_work_classification(
                work_id INTEGER NOT NULL,
                scheme INTEGER NOT NULL CHECK(scheme IN (1,2)),
                notation TEXT NOT NULL,
                PRIMARY KEY(work_id,scheme,notation)
             ) WITHOUT ROWID;
             CREATE TABLE work_classification(
                work_id INTEGER NOT NULL,
                scheme INTEGER NOT NULL CHECK(scheme IN (1,2)),
                notation TEXT NOT NULL,
                PRIMARY KEY(work_id,scheme,notation)
             ) WITHOUT ROWID;
             CREATE TABLE work_popularity(
                work_id INTEGER PRIMARY KEY,
                want_to_read_count INTEGER NOT NULL DEFAULT 0,
                currently_reading_count INTEGER NOT NULL DEFAULT 0,
                already_read_count INTEGER NOT NULL DEFAULT 0,
                rating_count INTEGER NOT NULL DEFAULT 0,
                rating_sum INTEGER NOT NULL DEFAULT 0
             ) STRICT;
             CREATE TABLE edition_entity_evidence(
                edition_id INTEGER NOT NULL,
                kind INTEGER NOT NULL CHECK(kind IN (1,2)),
                name TEXT NOT NULL,
                normalized_name TEXT NOT NULL,
                wikidata_id INTEGER,
                PRIMARY KEY(edition_id,kind,name)
             ) WITHOUT ROWID;
             CREATE TABLE work_entity_evidence(
                work_id INTEGER NOT NULL,
                kind INTEGER NOT NULL CHECK(kind IN (1,2)),
                name TEXT NOT NULL,
                normalized_name TEXT NOT NULL,
                wikidata_id INTEGER,
                PRIMARY KEY(work_id,kind,name)
             ) WITHOUT ROWID;
             CREATE TABLE edition_author(
                edition_id INTEGER NOT NULL,
                position INTEGER NOT NULL,
                author_id INTEGER NOT NULL,
                PRIMARY KEY(edition_id,position)
             ) WITHOUT ROWID;
             CREATE TABLE work_author(
                work_id INTEGER NOT NULL,
                position INTEGER NOT NULL,
                author_id INTEGER NOT NULL,
                PRIMARY KEY(work_id,position)
             ) WITHOUT ROWID;
             CREATE TABLE author(
                author_id INTEGER PRIMARY KEY,
                name TEXT
             ) STRICT;
             CREATE TABLE author_identifier(
                author_id INTEGER NOT NULL,
                authority TEXT NOT NULL,
                external_id TEXT NOT NULL,
                PRIMARY KEY(author_id,authority,external_id)
             ) WITHOUT ROWID;
             CREATE TABLE entity(
                wikidata_id INTEGER PRIMARY KEY,
                kind INTEGER NOT NULL CHECK(kind IN (1,2)),
                label TEXT NOT NULL,
                birth_year INTEGER,
                death_year INTEGER,
                fictional INTEGER NOT NULL DEFAULT 0 CHECK(fictional IN (0,1)),
                latitude_e7 INTEGER,
                longitude_e7 INTEGER
             ) STRICT;
             CREATE TABLE entity_alias(
                wikidata_id INTEGER NOT NULL,
                alias TEXT NOT NULL,
                PRIMARY KEY(wikidata_id,alias)
             ) WITHOUT ROWID;
             CREATE TABLE place_parent(
                place_id INTEGER NOT NULL,
                parent_id INTEGER NOT NULL,
                PRIMARY KEY(place_id,parent_id)
             ) WITHOUT ROWID;
             CREATE TABLE place_country(
                place_id INTEGER NOT NULL,
                country_id INTEGER NOT NULL,
                PRIMARY KEY(place_id,country_id)
             ) WITHOUT ROWID;
             CREATE TABLE wikidata_candidate(
                kind INTEGER NOT NULL,
                normalized_name TEXT NOT NULL,
                wikidata_id INTEGER NOT NULL,
                PRIMARY KEY(kind,normalized_name,wikidata_id)
             ) WITHOUT ROWID;
             CREATE TABLE wikidata_place_candidate(
                normalized_name TEXT NOT NULL,
                wikidata_id INTEGER NOT NULL,
                PRIMARY KEY(normalized_name,wikidata_id)
             ) WITHOUT ROWID;
             CREATE TABLE wikidata_instance(
                wikidata_id INTEGER NOT NULL,
                class_id INTEGER NOT NULL,
                PRIMARY KEY(wikidata_id,class_id)
             ) WITHOUT ROWID;
             CREATE TABLE wikidata_subclass(
                child_id INTEGER NOT NULL,
                parent_id INTEGER NOT NULL,
                PRIMARY KEY(child_id,parent_id)
             ) WITHOUT ROWID;

             -- Import staging tables intentionally have no indexes. Dump rows
             -- are appended cheaply and then sorted/deduplicated into the
             -- query schema once per phase.
             CREATE TABLE edition_stage(edition_id INTEGER NOT NULL, work_id INTEGER);
             CREATE TABLE edition_isbn_stage(isbn13 INTEGER NOT NULL, edition_id INTEGER NOT NULL);
             CREATE TABLE edition_bibliography_stage(edition_id INTEGER NOT NULL, title TEXT NOT NULL, normalized_title TEXT NOT NULL, publication_year INTEGER);
             CREATE TABLE edition_publisher_stage(edition_id INTEGER NOT NULL, position INTEGER NOT NULL, name TEXT NOT NULL, normalized_name TEXT NOT NULL);
             CREATE TABLE edition_classification_stage(edition_id INTEGER NOT NULL, scheme INTEGER NOT NULL, notation TEXT NOT NULL);
             CREATE TABLE edition_work_classification_stage(work_id INTEGER NOT NULL, scheme INTEGER NOT NULL, notation TEXT NOT NULL);
             CREATE TABLE edition_author_stage(edition_id INTEGER NOT NULL, position INTEGER NOT NULL, author_id INTEGER NOT NULL);
             CREATE TABLE edition_entity_evidence_stage(edition_id INTEGER NOT NULL, kind INTEGER NOT NULL, name TEXT NOT NULL, normalized_name TEXT NOT NULL);
             CREATE TABLE work_classification_stage(work_id INTEGER NOT NULL, scheme INTEGER NOT NULL, notation TEXT NOT NULL);
             CREATE TABLE work_bibliography_stage(work_id INTEGER NOT NULL, title TEXT NOT NULL, normalized_title TEXT NOT NULL);
             CREATE TABLE work_author_stage(work_id INTEGER NOT NULL, position INTEGER NOT NULL, author_id INTEGER NOT NULL);
             CREATE TABLE work_entity_evidence_stage(work_id INTEGER NOT NULL, kind INTEGER NOT NULL, name TEXT NOT NULL, normalized_name TEXT NOT NULL);
             CREATE TABLE author_stage(author_id INTEGER NOT NULL, name TEXT);
             CREATE TABLE author_identifier_stage(author_id INTEGER NOT NULL, authority TEXT NOT NULL, external_id TEXT NOT NULL);
             CREATE TABLE entity_stage(wikidata_id INTEGER NOT NULL, kind INTEGER NOT NULL, label TEXT NOT NULL, birth_year INTEGER, death_year INTEGER, fictional INTEGER NOT NULL, latitude_e7 INTEGER, longitude_e7 INTEGER);
             CREATE TABLE entity_alias_stage(wikidata_id INTEGER NOT NULL, alias TEXT NOT NULL);
             CREATE TABLE place_parent_stage(place_id INTEGER NOT NULL, parent_id INTEGER NOT NULL);
             CREATE TABLE place_country_stage(place_id INTEGER NOT NULL, country_id INTEGER NOT NULL);
             CREATE TABLE wikidata_candidate_stage(kind INTEGER NOT NULL, normalized_name TEXT NOT NULL, wikidata_id INTEGER NOT NULL);
             CREATE TABLE wikidata_place_candidate_stage(normalized_name TEXT NOT NULL, wikidata_id INTEGER NOT NULL);
             CREATE TABLE wikidata_instance_stage(wikidata_id INTEGER NOT NULL, class_id INTEGER NOT NULL);
             CREATE TABLE wikidata_subclass_stage(child_id INTEGER NOT NULL, parent_id INTEGER NOT NULL);
             INSERT INTO metadata_metric(name,value) VALUES('builder_schema_version',8);",
        )
        .map_err(error)
}

fn import_editions(connection: &mut Connection, path: &Path, limit: Option<usize>) -> Result<usize, MetadataError> {
    let count = read_open_library_resumable::<EditionRecord>(connection, path, limit, "editions", |transaction, records| {
        let mut edition_insert = transaction.prepare_cached("INSERT INTO edition_stage(edition_id,work_id) VALUES(?1,?2)").map_err(error)?;
        let mut isbn_insert = transaction.prepare_cached("INSERT INTO edition_isbn_stage(isbn13,edition_id) VALUES(?1,?2)").map_err(error)?;
        let mut bibliography_insert = transaction.prepare_cached("INSERT INTO edition_bibliography_stage(edition_id,title,normalized_title,publication_year) VALUES(?1,?2,?3,?4)").map_err(error)?;
        let mut publisher_insert = transaction.prepare_cached("INSERT INTO edition_publisher_stage(edition_id,position,name,normalized_name) VALUES(?1,?2,?3,?4)").map_err(error)?;
        let mut classification_insert = transaction.prepare_cached("INSERT INTO edition_classification_stage(edition_id,scheme,notation) VALUES(?1,?2,?3)").map_err(error)?;
        let mut work_classification_insert = transaction.prepare_cached("INSERT INTO edition_work_classification_stage(work_id,scheme,notation) VALUES(?1,?2,?3)").map_err(error)?;
        let mut author_insert = transaction.prepare_cached("INSERT INTO edition_author_stage(edition_id,position,author_id) VALUES(?1,?2,?3)").map_err(error)?;
        let mut entity_insert = transaction.prepare_cached("INSERT INTO edition_entity_evidence_stage(edition_id,kind,name,normalized_name) VALUES(?1,?2,?3,?4)").map_err(error)?;
        for record in records {
            let Some(edition_id) = open_library_id(&record.key, 'M') else { continue };
            let mut isbns = BTreeSet::new();
            isbns.extend(record.isbn_10.iter().filter_map(|isbn| canonical_isbn13(isbn)));
            isbns.extend(record.isbn_13.iter().filter_map(|isbn| canonical_isbn13(isbn)));
            let work_id = record.works.first().and_then(|work| open_library_id(&work.key, 'W'));
            if let Some(work_id) = work_id {
                insert_classifications(&mut work_classification_insert, work_id, DDC_SCHEME, &record.dewey_decimal_class)?;
                insert_classifications(&mut work_classification_insert, work_id, LCC_SCHEME, &record.lc_classifications)?;
            }
            if isbns.is_empty() {
                continue;
            }
            edition_insert.execute(params![edition_id, work_id]).map_err(error)?;
            for isbn in isbns {
                isbn_insert.execute(params![isbn, edition_id]).map_err(error)?;
            }
            if let Some(title) = record.title.as_deref().map(compact_text).filter(|title| !title.is_empty() && title.len() <= 4_096) {
                let normalized_title = bibliographic_match_key(&title);
                if !normalized_title.is_empty() {
                    bibliography_insert.execute(params![edition_id, title, normalized_title, record.publish_date.as_deref().and_then(publication_year)]).map_err(error)?;
                    for (position, publisher) in record.publishers.into_iter().enumerate() {
                        let publisher = compact_text(&publisher);
                        let normalized = bibliographic_match_key(&publisher);
                        if !publisher.is_empty() && publisher.len() <= 4_096 && !normalized.is_empty() {
                            publisher_insert.execute(params![edition_id, i64::try_from(position).map_err(error)?, publisher, normalized]).map_err(error)?;
                        }
                    }
                }
            }
            insert_classifications(&mut classification_insert, edition_id, DDC_SCHEME, &record.dewey_decimal_class)?;
            insert_classifications(&mut classification_insert, edition_id, LCC_SCHEME, &record.lc_classifications)?;
            insert_author_references(&mut author_insert, edition_id, record.authors)?;
            insert_entity_evidence(&mut entity_insert, edition_id, OL_PLACE_KIND, record.subject_places)?;
            insert_entity_evidence(&mut entity_insert, edition_id, OL_PERSON_KIND, record.subject_people)?;
        }
        Ok(())
    })?;
    materialize_editions(connection)?;
    let retained: i64 = connection.query_row("SELECT COUNT(*) FROM edition", (), |row| row.get(0)).map_err(error)?;
    println!("read {count} edition records; retained {retained} with a valid ISBN");
    Ok(count)
}

fn import_edition_identities(connection: &mut Connection, path: &Path, limit: Option<usize>) -> Result<(), MetadataError> {
    if completed_phase(connection, "edition_identities_materialize")?.is_some() {
        return Ok(());
    }
    read_open_library_resumable::<EditionRecord>(connection, path, limit, "edition_identities", |transaction, records| {
        let mut bibliography_insert = transaction.prepare_cached("INSERT INTO edition_bibliography_stage(edition_id,title,normalized_title,publication_year) VALUES(?1,?2,?3,?4)").map_err(error)?;
        let mut publisher_insert = transaction.prepare_cached("INSERT INTO edition_publisher_stage(edition_id,position,name,normalized_name) VALUES(?1,?2,?3,?4)").map_err(error)?;
        let mut retained = transaction.prepare_cached("SELECT EXISTS(SELECT 1 FROM edition WHERE edition_id=?1)").map_err(error)?;
        for record in records {
            let Some(edition_id) = open_library_id(&record.key, 'M') else { continue };
            if !retained.query_row([edition_id], |row| row.get::<_, bool>(0)).map_err(error)? {
                continue;
            }
            let Some(title) = record.title.as_deref().map(compact_text).filter(|title| !title.is_empty() && title.len() <= 4_096) else { continue };
            let normalized_title = bibliographic_match_key(&title);
            if normalized_title.is_empty() {
                continue;
            }
            bibliography_insert.execute(params![edition_id, title, normalized_title, record.publish_date.as_deref().and_then(publication_year)]).map_err(error)?;
            for (position, publisher) in record.publishers.into_iter().enumerate() {
                let publisher = compact_text(&publisher);
                let normalized = bibliographic_match_key(&publisher);
                if !publisher.is_empty() && publisher.len() <= 4_096 && !normalized.is_empty() {
                    publisher_insert.execute(params![edition_id, i64::try_from(position).map_err(error)?, publisher, normalized]).map_err(error)?;
                }
            }
        }
        Ok(())
    })?;
    materialize_edition_identities(connection)
}

fn import_works(connection: &mut Connection, path: &Path, limit: Option<usize>) -> Result<usize, MetadataError> {
    let count = read_open_library_resumable::<WorkRecord>(connection, path, limit, "works", |transaction, records| {
        let mut bibliography_insert = transaction.prepare_cached("INSERT INTO work_bibliography_stage(work_id,title,normalized_title) VALUES(?1,?2,?3)").map_err(error)?;
        let mut classification_insert = transaction.prepare_cached("INSERT INTO work_classification_stage(work_id,scheme,notation) VALUES(?1,?2,?3)").map_err(error)?;
        let mut author_insert = transaction.prepare_cached("INSERT INTO work_author_stage(work_id,position,author_id) VALUES(?1,?2,?3)").map_err(error)?;
        let mut entity_insert = transaction.prepare_cached("INSERT INTO work_entity_evidence_stage(work_id,kind,name,normalized_name) VALUES(?1,?2,?3,?4)").map_err(error)?;
        let mut referenced = transaction.prepare_cached("SELECT EXISTS(SELECT 1 FROM edition WHERE work_id=?1)").map_err(error)?;
        for record in records {
            let Some(work_id) = open_library_id(&record.key, 'W') else { continue };
            if !referenced.query_row([work_id], |row| row.get::<_, bool>(0)).map_err(error)? {
                continue;
            }
            if let Some(title) = record.title.as_deref().map(compact_text).filter(|title| !title.is_empty() && title.len() <= 4_096) {
                let normalized_title = bibliographic_match_key(&title);
                if !normalized_title.is_empty() {
                    bibliography_insert.execute(params![work_id, title, normalized_title]).map_err(error)?;
                }
            }
            insert_classifications(&mut classification_insert, work_id, DDC_SCHEME, &record.dewey_number)?;
            insert_classifications(&mut classification_insert, work_id, LCC_SCHEME, &record.lc_classifications)?;
            insert_author_references(&mut author_insert, work_id, record.authors)?;
            insert_entity_evidence(&mut entity_insert, work_id, OL_PLACE_KIND, record.subject_places)?;
            insert_entity_evidence(&mut entity_insert, work_id, OL_PERSON_KIND, record.subject_people)?;
        }
        Ok(())
    })?;
    materialize_works(connection)?;
    let retained: i64 = connection.query_row("SELECT COUNT(DISTINCT work_id) FROM edition WHERE work_id IS NOT NULL", (), |row| row.get(0)).map_err(error)?;
    println!("read {count} work records; retained {retained} referenced by an ISBN edition");
    Ok(count)
}

fn import_work_titles(connection: &mut Connection, path: &Path, limit: Option<usize>) -> Result<(), MetadataError> {
    if completed_phase(connection, "work_titles_materialize")?.is_some() {
        return Ok(());
    }
    read_open_library_resumable::<WorkRecord>(connection, path, limit, "work_titles", |transaction, records| {
        let mut insert = transaction.prepare_cached("INSERT INTO work_bibliography_stage(work_id,title,normalized_title) VALUES(?1,?2,?3)").map_err(error)?;
        let mut referenced = transaction.prepare_cached("SELECT EXISTS(SELECT 1 FROM edition WHERE work_id=?1)").map_err(error)?;
        for record in records {
            let Some(work_id) = open_library_id(&record.key, 'W') else { continue };
            if !referenced.query_row([work_id], |row| row.get::<_, bool>(0)).map_err(error)? {
                continue;
            }
            let Some(title) = record.title.as_deref().map(compact_text).filter(|title| !title.is_empty() && title.len() <= 4_096) else { continue };
            let normalized_title = bibliographic_match_key(&title);
            if !normalized_title.is_empty() {
                insert.execute(params![work_id, title, normalized_title]).map_err(error)?;
            }
        }
        Ok(())
    })?;
    materialize_work_titles(connection)
}

fn import_work_popularity(connection: &mut Connection, ratings_path: &Path, reading_log_path: &Path, limit: Option<usize>) -> Result<(), MetadataError> {
    read_tsv_resumable(connection, ratings_path, limit, "ratings", |transaction, lines| {
        let mut retained = transaction.prepare_cached("SELECT EXISTS(SELECT 1 FROM edition WHERE work_id=?1)").map_err(error)?;
        let mut update = transaction
            .prepare_cached(
                "INSERT INTO work_popularity(work_id,rating_count,rating_sum) VALUES(?1,1,?2)
                 ON CONFLICT(work_id) DO UPDATE SET rating_count=rating_count+1,rating_sum=rating_sum+excluded.rating_sum",
            )
            .map_err(error)?;
        for line in lines {
            let fields = line.trim_end().split('\t').collect::<Vec<_>>();
            let Some(work_id) = fields.first().and_then(|value| open_library_id(value, 'W')) else { continue };
            let Some(rating) = fields.get(2).and_then(|value| value.parse::<i64>().ok()).filter(|value| (1..=5).contains(value)) else { continue };
            if retained.query_row([work_id], |row| row.get::<_, bool>(0)).map_err(error)? {
                update.execute(params![work_id, rating]).map_err(error)?;
            }
        }
        Ok(())
    })?;
    read_tsv_resumable(connection, reading_log_path, limit, "reading_log", |transaction, lines| {
        let mut retained = transaction.prepare_cached("SELECT EXISTS(SELECT 1 FROM edition WHERE work_id=?1)").map_err(error)?;
        let mut update = transaction
            .prepare_cached(
                "INSERT INTO work_popularity(work_id,want_to_read_count,currently_reading_count,already_read_count)
                 VALUES(?1,?2,?3,?4)
                 ON CONFLICT(work_id) DO UPDATE SET
                   want_to_read_count=want_to_read_count+excluded.want_to_read_count,
                   currently_reading_count=currently_reading_count+excluded.currently_reading_count,
                   already_read_count=already_read_count+excluded.already_read_count",
            )
            .map_err(error)?;
        for line in lines {
            let fields = line.trim_end().split('\t').collect::<Vec<_>>();
            let Some(work_id) = fields.first().and_then(|value| open_library_id(value, 'W')) else { continue };
            let counts = match fields.get(2).copied() {
                Some("Want to Read") => (1_i64, 0_i64, 0_i64),
                Some("Currently Reading") => (0, 1, 0),
                Some("Already Read") => (0, 0, 1),
                _ => continue,
            };
            if retained.query_row([work_id], |row| row.get::<_, bool>(0)).map_err(error)? {
                update.execute(params![work_id, counts.0, counts.1, counts.2]).map_err(error)?;
            }
        }
        Ok(())
    })?;
    let retained: i64 = connection.query_row("SELECT COUNT(*) FROM work_popularity", (), |row| row.get(0)).map_err(error)?;
    println!("retained popularity aggregates for {retained} ISBN-linked works");
    Ok(())
}

fn read_tsv_resumable(connection: &mut Connection, path: &Path, limit: Option<usize>, phase: &str, mut import_batch: impl FnMut(&rusqlite::Transaction<'_>, Vec<String>) -> Result<(), MetadataError>) -> Result<usize, MetadataError> {
    if let Some(records) = completed_phase(connection, phase)? {
        return usize::try_from(records).map_err(error);
    }
    let resume_at = usize::try_from(checkpoint_records(connection, phase)?).map_err(error)?;
    let mut reader = BufReader::with_capacity(1024 * 1024, MultiGzDecoder::new(BufReader::with_capacity(1024 * 1024, File::open(path).map_err(error)?)));
    let mut line = String::new();
    let mut count = 0_usize;
    while count < resume_at {
        line.clear();
        if reader.read_line(&mut line).map_err(error)? == 0 {
            return Err(MetadataError(format!("cannot resume {phase} at record {resume_at}: dump ended after {count} records")));
        }
        count += 1;
    }
    loop {
        let mut lines = Vec::with_capacity(OPEN_LIBRARY_CHECKPOINT_RECORDS);
        let mut finished = limit.is_some_and(|limit| count >= limit);
        while lines.len() < OPEN_LIBRARY_CHECKPOINT_RECORDS && !finished {
            line.clear();
            if reader.read_line(&mut line).map_err(error)? == 0 {
                finished = true;
                break;
            }
            count += 1;
            lines.push(line.clone());
            if limit.is_some_and(|limit| count >= limit) {
                finished = true;
            }
        }
        let transaction = connection.transaction().map_err(error)?;
        import_batch(&transaction, lines)?;
        transaction
            .execute(
                "INSERT INTO import_checkpoint(phase,records_read,completed) VALUES(?1,?2,?3)
                 ON CONFLICT(phase) DO UPDATE SET records_read=excluded.records_read,completed=excluded.completed",
                params![phase, i64::try_from(count).map_err(error)?, i64::from(finished)],
            )
            .map_err(error)?;
        transaction.commit().map_err(error)?;
        if finished {
            return Ok(count);
        }
    }
}

fn import_authors(connection: &mut Connection, path: &Path, limit: Option<usize>) -> Result<usize, MetadataError> {
    let count = read_open_library_resumable::<AuthorRecord>(connection, path, limit, "authors", |transaction, records| {
        let mut author_insert = transaction.prepare_cached("INSERT INTO author_stage(author_id,name) VALUES(?1,?2)").map_err(error)?;
        let mut identifier_insert = transaction.prepare_cached("INSERT INTO author_identifier_stage(author_id,authority,external_id) VALUES(?1,?2,?3)").map_err(error)?;
        let mut referenced = transaction.prepare_cached("SELECT EXISTS(SELECT 1 FROM edition_author WHERE author_id=?1 UNION ALL SELECT 1 FROM work_author WHERE author_id=?1)").map_err(error)?;
        for record in records {
            let Some(author_id) = open_library_id(&record.key, 'A') else { continue };
            if !referenced.query_row([author_id], |row| row.get::<_, bool>(0)).map_err(error)? {
                continue;
            }
            let name = record.name.map(|name| name.split_whitespace().collect::<Vec<_>>().join(" ")).filter(|name| !name.is_empty() && name.len() <= 4_096);
            author_insert.execute(params![author_id, name]).map_err(error)?;
            for (authority, value) in record.remote_ids {
                let authority = authority.trim().to_ascii_lowercase();
                let value = match value {
                    serde_json::Value::String(value) => value,
                    serde_json::Value::Number(value) => value.to_string(),
                    _ => continue,
                };
                let value = value.trim();
                if !authority.is_empty() && authority.len() <= 64 && !value.is_empty() && value.len() <= 512 {
                    identifier_insert.execute(params![author_id, authority, value]).map_err(error)?;
                }
            }
        }
        Ok(())
    })?;
    materialize_authors(connection)?;
    let retained: i64 = connection.query_row("SELECT COUNT(*) FROM author", (), |row| row.get(0)).map_err(error)?;
    println!("read {count} author records; retained {retained} referenced by an ISBN edition or work");
    Ok(count)
}

fn materialize_editions(connection: &mut Connection) -> Result<(), MetadataError> {
    if completed_phase(connection, "editions_materialize")?.is_some() {
        return Ok(());
    }
    let transaction = connection.transaction().map_err(error)?;
    transaction
        .execute_batch(
            "INSERT OR REPLACE INTO edition(edition_id,work_id)
                 SELECT edition_id,work_id FROM edition_stage ORDER BY edition_id;
             INSERT OR IGNORE INTO edition_isbn(isbn13,edition_id)
                 SELECT isbn13,edition_id FROM edition_isbn_stage ORDER BY isbn13,edition_id;
             INSERT OR REPLACE INTO edition_bibliography(edition_id,title,normalized_title,publication_year)
                 SELECT edition_id,title,normalized_title,publication_year FROM edition_bibliography_stage ORDER BY normalized_title,edition_id;
             INSERT OR IGNORE INTO edition_publisher(edition_id,position,name,normalized_name)
                 SELECT edition_id,position,name,normalized_name FROM edition_publisher_stage ORDER BY edition_id,position;
             INSERT OR IGNORE INTO edition_classification(edition_id,scheme,notation)
                 SELECT edition_id,scheme,notation FROM edition_classification_stage ORDER BY edition_id,scheme,notation;
             INSERT OR IGNORE INTO edition_work_classification(work_id,scheme,notation)
                 SELECT work_id,scheme,notation FROM edition_work_classification_stage ORDER BY work_id,scheme,notation;
             INSERT OR IGNORE INTO edition_author(edition_id,position,author_id)
                 SELECT edition_id,position,author_id FROM edition_author_stage ORDER BY edition_id,position;
             INSERT OR IGNORE INTO edition_entity_evidence(edition_id,kind,name,normalized_name)
                 SELECT edition_id,kind,name,normalized_name FROM edition_entity_evidence_stage ORDER BY edition_id,kind,name;
             DROP TABLE edition_stage;
             DROP TABLE edition_isbn_stage;
             DROP TABLE edition_bibliography_stage;
             DROP TABLE edition_publisher_stage;
             DROP TABLE edition_classification_stage;
             DROP TABLE edition_work_classification_stage;
             DROP TABLE edition_author_stage;
             DROP TABLE edition_entity_evidence_stage;",
        )
        .map_err(error)?;
    mark_materialized(&transaction, "editions_materialize")?;
    mark_materialized(&transaction, "edition_identities_materialize")?;
    transaction.commit().map_err(error)
}

fn materialize_edition_identities(connection: &mut Connection) -> Result<(), MetadataError> {
    if completed_phase(connection, "edition_identities_materialize")?.is_some() {
        return Ok(());
    }
    let transaction = connection.transaction().map_err(error)?;
    transaction
        .execute_batch(
            "INSERT OR REPLACE INTO edition_bibliography(edition_id,title,normalized_title,publication_year)
                 SELECT edition_id,title,normalized_title,publication_year FROM edition_bibliography_stage ORDER BY normalized_title,edition_id;
             INSERT OR IGNORE INTO edition_publisher(edition_id,position,name,normalized_name)
                 SELECT edition_id,position,name,normalized_name FROM edition_publisher_stage ORDER BY edition_id,position;
             DROP TABLE edition_bibliography_stage;
             DROP TABLE edition_publisher_stage;",
        )
        .map_err(error)?;
    mark_materialized(&transaction, "edition_identities_materialize")?;
    transaction.commit().map_err(error)
}

fn materialize_works(connection: &mut Connection) -> Result<(), MetadataError> {
    if completed_phase(connection, "works_materialize")?.is_some() {
        return Ok(());
    }
    let transaction = connection.transaction().map_err(error)?;
    transaction
        .execute_batch(
            "INSERT OR IGNORE INTO work_classification(work_id,scheme,notation)
                 SELECT work_id,scheme,notation FROM work_classification_stage ORDER BY work_id,scheme,notation;
             INSERT OR IGNORE INTO work_author(work_id,position,author_id)
                 SELECT work_id,position,author_id FROM work_author_stage ORDER BY work_id,position;
             INSERT OR IGNORE INTO work_entity_evidence(work_id,kind,name,normalized_name)
                 SELECT work_id,kind,name,normalized_name FROM work_entity_evidence_stage ORDER BY work_id,kind,name;
             INSERT OR REPLACE INTO work_bibliography(work_id,title,normalized_title)
                 SELECT work_id,title,normalized_title FROM work_bibliography_stage ORDER BY normalized_title,work_id;
             DROP TABLE work_classification_stage;
             DROP TABLE work_bibliography_stage;
             DROP TABLE work_author_stage;
             DROP TABLE work_entity_evidence_stage;",
        )
        .map_err(error)?;
    mark_materialized(&transaction, "works_materialize")?;
    mark_materialized(&transaction, "work_titles_materialize")?;
    transaction.commit().map_err(error)
}

fn materialize_work_titles(connection: &mut Connection) -> Result<(), MetadataError> {
    if completed_phase(connection, "work_titles_materialize")?.is_some() {
        return Ok(());
    }
    let transaction = connection.transaction().map_err(error)?;
    transaction
        .execute_batch(
            "INSERT OR REPLACE INTO work_bibliography(work_id,title,normalized_title)
                 SELECT work_id,title,normalized_title FROM work_bibliography_stage ORDER BY normalized_title,work_id;
             DROP TABLE work_bibliography_stage;",
        )
        .map_err(error)?;
    mark_materialized(&transaction, "work_titles_materialize")?;
    transaction.commit().map_err(error)
}

fn materialize_authors(connection: &mut Connection) -> Result<(), MetadataError> {
    if completed_phase(connection, "authors_materialize")?.is_some() {
        return Ok(());
    }
    let transaction = connection.transaction().map_err(error)?;
    transaction
        .execute_batch(
            "INSERT OR REPLACE INTO author(author_id,name)
                 SELECT author_id,name FROM author_stage ORDER BY author_id;
             INSERT OR IGNORE INTO author_identifier(author_id,authority,external_id)
                 SELECT author_id,authority,external_id FROM author_identifier_stage ORDER BY author_id,authority,external_id;
             DROP TABLE author_stage;
             DROP TABLE author_identifier_stage;",
        )
        .map_err(error)?;
    mark_materialized(&transaction, "authors_materialize")?;
    transaction.commit().map_err(error)
}

fn mark_materialized(transaction: &rusqlite::Transaction<'_>, phase: &str) -> Result<(), MetadataError> {
    transaction.execute("INSERT INTO import_checkpoint(phase,records_read,completed) VALUES(?1,0,1) ON CONFLICT(phase) DO UPDATE SET completed=1", [phase]).map(|_| ()).map_err(error)
}

fn insert_author_references(statement: &mut rusqlite::CachedStatement<'_>, owner_id: i64, authors: Vec<KeyReference>) -> Result<(), MetadataError> {
    for (position, author) in authors.into_iter().enumerate() {
        let Some(author_id) = open_library_id(&author.key, 'A') else { continue };
        statement.execute(params![owner_id, i64::try_from(position).map_err(error)?, author_id]).map_err(error)?;
    }
    Ok(())
}

fn insert_classifications(statement: &mut rusqlite::CachedStatement<'_>, owner_id: i64, scheme: i64, values: &[String]) -> Result<(), MetadataError> {
    for notation in values {
        let notation = notation.split_whitespace().collect::<Vec<_>>().join(" ");
        if notation.is_empty() || notation.len() > MAX_NOTATION_BYTES {
            continue;
        }
        statement.execute(params![owner_id, scheme, notation]).map_err(error)?;
    }
    Ok(())
}

fn insert_entity_evidence(statement: &mut rusqlite::CachedStatement<'_>, owner_id: i64, kind: i64, values: Vec<String>) -> Result<(), MetadataError> {
    for name in values {
        let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
        if name.is_empty() || name.len() > 4_096 {
            continue;
        }
        let normalized_name = normalize_entity_heading(kind, &name);
        if normalized_name.is_empty() {
            continue;
        }
        statement.execute(params![owner_id, kind, name, normalized_name]).map_err(error)?;
    }
    Ok(())
}

fn normalize_entity_heading(kind: i64, value: &str) -> String {
    let value = if kind == OL_PERSON_KIND { strip_person_dates(value) } else { value };
    value.nfkc().flat_map(char::to_lowercase).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

fn strip_person_dates(value: &str) -> &str {
    let trimmed = value.trim();
    let Some(open) = trimmed.rfind(" (") else { return trimmed };
    let Some(dates) = trimmed.get(open + 2..trimmed.len().saturating_sub(1)) else { return trimmed };
    if trimmed.ends_with(')') && dates.chars().all(|character| character.is_ascii_digit() || matches!(character, '-' | '?' | ' ')) && dates.contains('-') {
        trimmed[..open].trim()
    } else {
        trimmed
    }
}

fn compact_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Locale-independent key for conservative equality matching. Punctuation and
/// accents are not edition identity; token order is retained.
fn bibliographic_match_key(value: &str) -> String {
    use unicode_normalization::char::is_combining_mark;
    let mut key = String::new();
    let mut separator = false;
    for character in value.nfkd().flat_map(char::to_lowercase) {
        if is_combining_mark(character) {
            continue;
        }
        if character.is_alphanumeric() {
            if separator && !key.is_empty() {
                key.push(' ');
            }
            key.push(character);
            separator = false;
        } else {
            separator = true;
        }
    }
    key
}

fn title_substring_keys(value: &str) -> Vec<(String, usize)> {
    let normalized = bibliographic_match_key(value);
    let words = normalized.split_whitespace().take(MAX_TITLE_SUBSTRINGS_PER_QUERY).collect::<Vec<_>>();
    let mut substrings = BTreeMap::<String, usize>::new();
    for start in 0..words.len() {
        for end in start + 1..=words.len() {
            let substring = words[start..end].join(" ");
            let specificity = substring.chars().count();
            substrings.entry(substring).or_insert(specificity);
        }
    }
    let mut substrings = substrings.into_iter().collect::<Vec<_>>();
    substrings.sort_by(|(left_title, left_specificity), (right_title, right_specificity)| right_specificity.cmp(left_specificity).then_with(|| left_title.cmp(right_title)));
    substrings.truncate(MAX_TITLE_SUBSTRINGS_PER_QUERY);
    substrings
}

fn author_match_keys(value: &str) -> BTreeSet<String> {
    let mut keys = BTreeSet::from([bibliographic_match_key(value)]);
    if let Some((family, given)) = value.split_once(',') {
        keys.insert(bibliographic_match_key(&format!("{given} {family}")));
    }
    keys.remove("");
    keys
}

fn publication_year(value: &str) -> Option<i32> {
    let mut years = BTreeSet::new();
    for run in value.split(|character: char| !character.is_ascii_digit()) {
        if run.len() == 4 {
            if let Ok(year) = run.parse::<i32>() {
                if (1000..=2100).contains(&year) {
                    years.insert(year);
                }
            }
        }
    }
    (years.len() == 1).then(|| *years.first().expect("one year exists"))
}

fn import_wikidata(connection: &mut Connection, path: &Path, limit: Option<usize>) -> Result<usize, MetadataError> {
    let place_targets = evidence_targets(connection, OL_PLACE_KIND)?;
    let person_targets = evidence_targets(connection, OL_PERSON_KIND)?;
    let extract = |record| extract_wikidata_entity(record, &place_targets, &person_targets);
    let count =
        read_json_resumable_mapped::<WikidataRecord, WikidataExtract>(connection, path, limit, "wikidata_scan", WIKIDATA_CHECKPOINT_RECORDS, WIKIDATA_PIPELINE_PARSE_RECORDS, DumpFormat::Wikidata, &extract, &mut |transaction, records| {
            let mut entity_insert = transaction.prepare_cached("INSERT INTO entity_stage(wikidata_id,kind,label,birth_year,death_year,fictional,latitude_e7,longitude_e7) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)").map_err(error)?;
            let mut alias_insert = transaction.prepare_cached("INSERT INTO entity_alias_stage(wikidata_id,alias) VALUES(?1,?2)").map_err(error)?;
            let mut parent_insert = transaction.prepare_cached("INSERT INTO place_parent_stage(place_id,parent_id) VALUES(?1,?2)").map_err(error)?;
            let mut country_insert = transaction.prepare_cached("INSERT INTO place_country_stage(place_id,country_id) VALUES(?1,?2)").map_err(error)?;
            let mut candidate_insert = transaction.prepare_cached("INSERT INTO wikidata_candidate_stage(kind,normalized_name,wikidata_id) VALUES(?1,?2,?3)").map_err(error)?;
            let mut place_candidate_insert = transaction.prepare_cached("INSERT INTO wikidata_place_candidate_stage(normalized_name,wikidata_id) VALUES(?1,?2)").map_err(error)?;
            let mut instance_insert = transaction.prepare_cached("INSERT INTO wikidata_instance_stage(wikidata_id,class_id) VALUES(?1,?2)").map_err(error)?;
            let mut subclass_insert = transaction.prepare_cached("INSERT INTO wikidata_subclass_stage(child_id,parent_id) VALUES(?1,?2)").map_err(error)?;
            for extracted in records {
                let qid = extracted.qid;
                if let Some(entity) = extracted.entity {
                    entity_insert.execute(params![qid, entity.kind, entity.label, entity.birth_year, entity.death_year, i64::from(entity.fictional), entity.latitude_e7, entity.longitude_e7]).map_err(error)?;
                }
                for alias in extracted.aliases {
                    alias_insert.execute(params![qid, alias]).map_err(error)?;
                }
                for parent in extracted.parents {
                    parent_insert.execute(params![qid, parent]).map_err(error)?;
                }
                for country in extracted.countries {
                    country_insert.execute(params![qid, country]).map_err(error)?;
                }
                for (kind, name) in extracted.candidates {
                    candidate_insert.execute(params![kind, name, qid]).map_err(error)?;
                }
                for name in extracted.place_candidates {
                    place_candidate_insert.execute(params![name, qid]).map_err(error)?;
                }
                for class_id in extracted.instances {
                    instance_insert.execute(params![qid, class_id]).map_err(error)?;
                }
                for parent in extracted.subclasses {
                    subclass_insert.execute(params![qid, parent]).map_err(error)?;
                }
            }
            Ok(())
        })?;
    materialize_wikidata(connection)?;
    if completed_phase(connection, "wikidata_finalize")?.is_none() {
        let transaction = connection.transaction().map_err(error)?;
        transaction
            .execute_batch(
                "CREATE TEMP TABLE geographic_class(
                     class_id INTEGER PRIMARY KEY
                 ) WITHOUT ROWID;
                 INSERT OR IGNORE INTO geographic_class(class_id) VALUES
                     (486972),  /* human settlement */
                     (82794),   /* geographic region */
                     (6256),    /* country */
                     (5107),    /* continent */
                     (618123);  /* geographical object */
                 WITH RECURSIVE descendants(class_id) AS (
                     SELECT class_id FROM geographic_class
                     UNION
                     SELECT relation.child_id
                     FROM wikidata_subclass AS relation
                     JOIN descendants AS parent ON parent.class_id=relation.parent_id
                 )
                 INSERT OR IGNORE INTO geographic_class(class_id)
                     SELECT class_id FROM descendants;
                 INSERT OR IGNORE INTO wikidata_candidate(kind,normalized_name,wikidata_id)
                     SELECT 1,candidate.normalized_name,candidate.wikidata_id
                     FROM wikidata_place_candidate AS candidate
                     JOIN wikidata_instance AS instance ON instance.wikidata_id=candidate.wikidata_id
                     JOIN geographic_class ON geographic_class.class_id=instance.class_id;
                 DROP TABLE geographic_class;
                 INSERT OR REPLACE INTO metadata_metric(name,value)
                     SELECT 'place_candidate_headings',COUNT(DISTINCT normalized_name) FROM wikidata_candidate WHERE kind=1;
                 INSERT OR REPLACE INTO metadata_metric(name,value)
                     SELECT 'place_ambiguous_headings',COUNT(*) FROM (SELECT normalized_name FROM wikidata_candidate WHERE kind=1 GROUP BY normalized_name HAVING COUNT(*)>1);
                 INSERT OR REPLACE INTO metadata_metric(name,value)
                     SELECT 'person_candidate_headings',COUNT(DISTINCT normalized_name) FROM wikidata_candidate WHERE kind=2;
                 INSERT OR REPLACE INTO metadata_metric(name,value)
                     SELECT 'person_ambiguous_headings',COUNT(*) FROM (SELECT normalized_name FROM wikidata_candidate WHERE kind=2 GROUP BY normalized_name HAVING COUNT(*)>1);
                 UPDATE edition_entity_evidence AS evidence SET wikidata_id=(
             SELECT MIN(candidate.wikidata_id) FROM wikidata_candidate AS candidate
             WHERE candidate.kind=evidence.kind AND candidate.normalized_name=evidence.normalized_name
             HAVING COUNT(*)=1
         );
         UPDATE work_entity_evidence AS evidence SET wikidata_id=(
             SELECT MIN(candidate.wikidata_id) FROM wikidata_candidate AS candidate
             WHERE candidate.kind=evidence.kind AND candidate.normalized_name=evidence.normalized_name
             HAVING COUNT(*)=1
         );
         CREATE TEMP TABLE retained_entity(
             wikidata_id INTEGER PRIMARY KEY
         ) WITHOUT ROWID;
         INSERT OR IGNORE INTO retained_entity(wikidata_id)
             SELECT wikidata_id FROM edition_entity_evidence WHERE wikidata_id IS NOT NULL
             UNION
             SELECT wikidata_id FROM work_entity_evidence WHERE wikidata_id IS NOT NULL;
         WITH RECURSIVE place_closure(wikidata_id) AS (
             SELECT retained.wikidata_id
             FROM retained_entity AS retained
             JOIN entity ON entity.wikidata_id=retained.wikidata_id
             WHERE entity.kind=1
             UNION
             SELECT relation.parent_id
             FROM place_parent AS relation
             JOIN place_closure AS retained ON retained.wikidata_id=relation.place_id
             JOIN entity ON entity.wikidata_id=relation.parent_id AND entity.kind=1
             UNION
             SELECT relation.country_id
             FROM place_country AS relation
             JOIN place_closure AS retained ON retained.wikidata_id=relation.place_id
             JOIN entity ON entity.wikidata_id=relation.country_id AND entity.kind=1
         )
         INSERT OR IGNORE INTO retained_entity(wikidata_id)
             SELECT wikidata_id FROM place_closure;
         DELETE FROM entity_alias WHERE wikidata_id NOT IN (SELECT wikidata_id FROM retained_entity);
         DELETE FROM place_parent
             WHERE place_id NOT IN (SELECT wikidata_id FROM retained_entity)
                OR parent_id NOT IN (SELECT wikidata_id FROM retained_entity);
         DELETE FROM place_country
             WHERE place_id NOT IN (SELECT wikidata_id FROM retained_entity)
                OR country_id NOT IN (SELECT wikidata_id FROM retained_entity);
         DELETE FROM entity WHERE wikidata_id NOT IN (SELECT wikidata_id FROM retained_entity);
         INSERT OR REPLACE INTO metadata_metric(name,value)
             SELECT 'place_evidence',COUNT(*) FROM edition_entity_evidence WHERE kind=1;
         INSERT OR REPLACE INTO metadata_metric(name,value)
             SELECT 'person_evidence',COUNT(*) FROM edition_entity_evidence WHERE kind=2;
         INSERT OR REPLACE INTO metadata_metric(name,value)
             SELECT 'work_place_evidence',COUNT(*) FROM work_entity_evidence WHERE kind=1;
         INSERT OR REPLACE INTO metadata_metric(name,value)
             SELECT 'work_person_evidence',COUNT(*) FROM work_entity_evidence WHERE kind=2;
         INSERT OR REPLACE INTO metadata_metric(name,value)
             SELECT 'resolved_place_evidence',COUNT(*) FROM (
                 SELECT wikidata_id FROM edition_entity_evidence WHERE kind=1 AND wikidata_id IS NOT NULL
                 UNION ALL SELECT wikidata_id FROM work_entity_evidence WHERE kind=1 AND wikidata_id IS NOT NULL
             );
         INSERT OR REPLACE INTO metadata_metric(name,value)
             SELECT 'resolved_person_evidence',COUNT(*) FROM (
                 SELECT wikidata_id FROM edition_entity_evidence WHERE kind=2 AND wikidata_id IS NOT NULL
                 UNION ALL SELECT wikidata_id FROM work_entity_evidence WHERE kind=2 AND wikidata_id IS NOT NULL
             );
         INSERT OR REPLACE INTO metadata_metric(name,value)
             SELECT 'retained_places',COUNT(*) FROM entity WHERE kind=1;
         INSERT OR REPLACE INTO metadata_metric(name,value)
             SELECT 'retained_people',COUNT(*) FROM entity WHERE kind=2;
         INSERT OR REPLACE INTO metadata_metric(name,value)
             SELECT 'retained_aliases',COUNT(*) FROM entity_alias;
         INSERT OR REPLACE INTO metadata_metric(name,value)
             SELECT 'retained_parent_edges',COUNT(*) FROM place_parent;
         INSERT OR REPLACE INTO metadata_metric(name,value)
             SELECT 'retained_country_edges',COUNT(*) FROM place_country;
         DROP TABLE retained_entity;
         DROP TABLE wikidata_candidate;
         DROP TABLE wikidata_place_candidate;
         DROP TABLE wikidata_instance;
         DROP TABLE wikidata_subclass;",
            )
            .map_err(error)?;
        transaction
            .execute("INSERT INTO import_checkpoint(phase,records_read,completed) VALUES('wikidata_finalize',?1,1) ON CONFLICT(phase) DO UPDATE SET records_read=excluded.records_read,completed=1", [i64::try_from(count).map_err(error)?])
            .map_err(error)?;
        transaction.commit().map_err(error)?;
    }
    println!("read {count} Wikidata entities; resolved unique exact headings conservatively");
    Ok(count)
}

fn materialize_wikidata(connection: &mut Connection) -> Result<(), MetadataError> {
    if completed_phase(connection, "wikidata_materialize")?.is_some() {
        return Ok(());
    }
    let transaction = connection.transaction().map_err(error)?;
    transaction
        .execute_batch(
            "INSERT OR REPLACE INTO entity(wikidata_id,kind,label,birth_year,death_year,fictional,latitude_e7,longitude_e7)
                 SELECT wikidata_id,kind,label,birth_year,death_year,fictional,latitude_e7,longitude_e7 FROM entity_stage ORDER BY wikidata_id;
             INSERT OR IGNORE INTO entity_alias(wikidata_id,alias)
                 SELECT wikidata_id,alias FROM entity_alias_stage ORDER BY wikidata_id,alias;
             INSERT OR IGNORE INTO place_parent(place_id,parent_id)
                 SELECT place_id,parent_id FROM place_parent_stage ORDER BY place_id,parent_id;
             INSERT OR IGNORE INTO place_country(place_id,country_id)
                 SELECT place_id,country_id FROM place_country_stage ORDER BY place_id,country_id;
             INSERT OR IGNORE INTO wikidata_candidate(kind,normalized_name,wikidata_id)
                 SELECT kind,normalized_name,wikidata_id FROM wikidata_candidate_stage ORDER BY kind,normalized_name,wikidata_id;
             INSERT OR IGNORE INTO wikidata_place_candidate(normalized_name,wikidata_id)
                 SELECT normalized_name,wikidata_id FROM wikidata_place_candidate_stage ORDER BY normalized_name,wikidata_id;
             INSERT OR IGNORE INTO wikidata_instance(wikidata_id,class_id)
                 SELECT wikidata_id,class_id FROM wikidata_instance_stage ORDER BY wikidata_id,class_id;
             INSERT OR IGNORE INTO wikidata_subclass(child_id,parent_id)
                 SELECT child_id,parent_id FROM wikidata_subclass_stage ORDER BY child_id,parent_id;
             DROP TABLE entity_stage;
             DROP TABLE entity_alias_stage;
             DROP TABLE place_parent_stage;
             DROP TABLE place_country_stage;
             DROP TABLE wikidata_candidate_stage;
             DROP TABLE wikidata_place_candidate_stage;
             DROP TABLE wikidata_instance_stage;
             DROP TABLE wikidata_subclass_stage;",
        )
        .map_err(error)?;
    mark_materialized(&transaction, "wikidata_materialize")?;
    transaction.commit().map_err(error)
}

fn checkpoint_records(connection: &Connection, phase: &str) -> Result<i64, MetadataError> {
    Ok(connection.query_row("SELECT records_read FROM import_checkpoint WHERE phase=?1", [phase], |row| row.get(0)).optional().map_err(error)?.unwrap_or(0))
}

fn wikidata_json_line(line: &str) -> Option<&str> {
    let raw = line.trim().trim_start_matches('[').trim_end_matches(']').trim_end_matches(',').trim();
    (!raw.is_empty()).then_some(raw)
}

fn evidence_targets(connection: &Connection, kind: i64) -> Result<HashSet<String>, MetadataError> {
    let mut statement = connection.prepare("SELECT normalized_name FROM edition_entity_evidence WHERE kind=?1 UNION SELECT normalized_name FROM work_entity_evidence WHERE kind=?1").map_err(error)?;
    let targets = statement.query_map([kind], |row| row.get(0)).map_err(error)?.collect::<Result<HashSet<_>, _>>().map_err(error)?;
    Ok(targets)
}

struct ChildReader {
    child: Child,
    stdout: ChildStdout,
    finished: bool,
}

impl Read for ChildReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = self.stdout.read(buffer)?;
        if count == 0 && !self.finished {
            let status = self.child.wait()?;
            self.finished = true;
            if !status.success() {
                return Err(io::Error::other(format!("parallel decompressor exited with {status}")));
            }
        }
        Ok(count)
    }
}

impl Drop for ChildReader {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn child_reader(mut child: Child) -> Result<Box<dyn BufRead>, MetadataError> {
    let stdout = child.stdout.take().ok_or_else(|| MetadataError("decompressor did not provide stdout".to_owned()))?;
    Ok(Box::new(BufReader::with_capacity(1024 * 1024, ChildReader { child, stdout, finished: false })))
}

fn wikidata_reader(path: &Path, resume_at: usize) -> Result<(Box<dyn BufRead>, bool), MetadataError> {
    if resume_at > 0 {
        if let (Some(seeker), Some(index)) = (std::env::var_os("BOKHEIM_WIKIDATA_SEEKER"), std::env::var_os("BOKHEIM_WIKIDATA_SEEK_INDEX")) {
            if !Path::new(&index).is_file() {
                return wikidata_sequential_reader(path);
            }
            let child = Command::new(seeker).arg("stream").arg(path).arg(index).arg(resume_at.to_string()).args(["--threads", "2"]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::inherit()).spawn().map_err(error)?;
            return child_reader(child).map(|reader| (reader, true));
        }
    }
    wikidata_sequential_reader(path)
}

fn wikidata_sequential_reader(path: &Path) -> Result<(Box<dyn BufRead>, bool), MetadataError> {
    if path.extension().and_then(|extension| extension.to_str()) == Some("bz2") {
        if let Some(decompressor) = std::env::var_os("BOKHEIM_WIKIDATA_DECOMPRESSOR") {
            let child = Command::new(decompressor).args(["-n", "2", "-dc"]).arg(path).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::inherit()).spawn().map_err(error)?;
            return child_reader(child).map(|reader| (reader, false));
        }
    }
    let file = File::open(path).map_err(error)?;
    let reader: Box<dyn Read> = match path.extension().and_then(|extension| extension.to_str()) {
        Some("bz2") => Box::new(MultiBzDecoder::new(BufReader::with_capacity(1024 * 1024, file))),
        Some("gz") => Box::new(MultiGzDecoder::new(BufReader::with_capacity(1024 * 1024, file))),
        _ => Box::new(file),
    };
    Ok((Box::new(BufReader::with_capacity(1024 * 1024, reader)), false))
}

struct WikidataEntityRow {
    kind: i64,
    label: String,
    birth_year: Option<i32>,
    death_year: Option<i32>,
    fictional: bool,
    latitude_e7: Option<i32>,
    longitude_e7: Option<i32>,
}

struct WikidataExtract {
    qid: i64,
    entity: Option<WikidataEntityRow>,
    aliases: Vec<String>,
    parents: Vec<i64>,
    countries: Vec<i64>,
    candidates: Vec<(i64, String)>,
    place_candidates: Vec<String>,
    instances: Vec<i64>,
    subclasses: Vec<i64>,
}

fn extract_wikidata_entity(value: WikidataRecord, place_targets: &HashSet<String>, person_targets: &HashSet<String>) -> Option<WikidataExtract> {
    let qid = wikidata_id(&value.id)?;
    let labels = value.labels.values().map(|label| &label.value).collect::<Vec<_>>();
    let aliases = value.aliases.values().flatten().map(|alias| &alias.value).collect::<Vec<_>>();
    let label = preferred_wikidata_label(&value).unwrap_or_else(|| format!("Q{qid}"));
    let instances = claim_entity_ids(&value.claims.instances);
    let subclasses = claim_entity_ids(&value.claims.subclasses);
    let parents = claim_entity_ids(&value.claims.parents);
    let countries = claim_entity_ids(&value.claims.countries);
    let continents = claim_entity_ids(&value.claims.continents);
    let coordinates = claim_coordinate(&value.claims.coordinates);
    let is_human = instances.contains(&5);
    let fictional = instances.contains(&95074);
    // P17/P30 alone also occur on organizations and events. Keep those rows
    // temporarily for ancestor traversal, but do not let them seed a match.
    let explicit_place_type = instances.iter().any(|id| matches!(id, 515 | 6256 | 5107 | 82794 | 486972 | 35657));
    let is_geography_record = coordinates.is_some() || !parents.is_empty() || !countries.is_empty() || !continents.is_empty() || explicit_place_type;
    let is_place_candidate = explicit_place_type || !parents.is_empty() || (coordinates.is_some() && (!countries.is_empty() || !continents.is_empty()));
    let mut matched_place_names = BTreeSet::new();
    let mut matched_person_names = BTreeSet::new();
    for name in labels.iter().chain(&aliases) {
        let normalized = normalize_entity_heading(OL_PLACE_KIND, name);
        if place_targets.contains(&normalized) {
            matched_place_names.insert(normalized.clone());
        }
        if person_targets.contains(&normalized) {
            matched_person_names.insert(normalized);
        }
    }
    let matches_place = !matched_place_names.is_empty();
    let matches_person = !matched_person_names.is_empty();
    let kind = if is_geography_record || (matches_place && !instances.is_empty()) {
        Some(OL_PLACE_KIND)
    } else if (is_human || fictional) && matches_person {
        Some(OL_PERSON_KIND)
    } else {
        None
    };
    let entity = kind.map(|kind| {
        let (latitude_e7, longitude_e7) = coordinates.unzip();
        WikidataEntityRow { kind, label, birth_year: claim_year(&value.claims.births), death_year: claim_year(&value.claims.deaths), fictional, latitude_e7, longitude_e7 }
    });
    let retained_aliases = if entity.is_some() { aliases.into_iter().filter(|alias| !alias.is_empty() && alias.len() <= 4_096).cloned().collect() } else { Vec::new() };
    let (retained_parents, retained_countries) = if kind == Some(OL_PLACE_KIND) { (parents.into_iter().chain(continents).collect(), countries) } else { (Vec::new(), Vec::new()) };
    let mut candidates = Vec::new();
    if is_place_candidate {
        candidates.extend(matched_place_names.iter().cloned().map(|name| (OL_PLACE_KIND, name)));
    }
    if is_human || fictional {
        candidates.extend(matched_person_names.into_iter().map(|name| (OL_PERSON_KIND, name)));
    }
    let extracted = WikidataExtract {
        qid,
        entity,
        aliases: retained_aliases,
        parents: retained_parents,
        countries: retained_countries,
        candidates,
        place_candidates: matched_place_names.into_iter().collect(),
        instances: if matches_place { instances } else { Vec::new() },
        subclasses,
    };
    (extracted.entity.is_some() || !extracted.candidates.is_empty() || !extracted.place_candidates.is_empty() || !extracted.instances.is_empty() || !extracted.subclasses.is_empty()).then_some(extracted)
}

fn preferred_wikidata_label(value: &WikidataRecord) -> Option<String> {
    for language in ["sv", "en"] {
        if let Some(label) = value.labels.get(language) {
            return Some(label.value.clone());
        }
    }
    value.labels.values().next().map(|label| label.value.clone())
}

fn claim_values(claims: &[WikidataClaim]) -> impl Iterator<Item = &WikidataClaimValue> {
    claims.iter().filter_map(|claim| claim.mainsnak.datavalue.as_ref().map(|datavalue| &datavalue.value))
}

fn claim_entity_ids(claims: &[WikidataClaim]) -> Vec<i64> {
    claim_values(claims).filter_map(|value| value.id.as_deref().and_then(wikidata_id).or(value.numeric_id)).collect()
}

fn claim_coordinate(claims: &[WikidataClaim]) -> Option<(i32, i32)> {
    let coordinate = claim_values(claims).next()?;
    let latitude = (coordinate.latitude? * 10_000_000.0).round();
    let longitude = (coordinate.longitude? * 10_000_000.0).round();
    Some((latitude as i32, longitude as i32))
}

fn claim_year(claims: &[WikidataClaim]) -> Option<i32> {
    let time = claim_values(claims).next()?.time.as_deref()?;
    let digits = time.trim_start_matches('+').split('-').next()?;
    digits.parse().ok()
}

fn wikidata_id(value: &str) -> Option<i64> {
    value.strip_prefix('Q')?.parse().ok()
}

#[derive(Clone, Copy)]
enum DumpFormat {
    OpenLibraryGzip,
    Wikidata,
}

struct RawJsonBatch {
    end_count: usize,
    finished: bool,
    records: Vec<String>,
}

struct ParsedJsonBatch<T> {
    end_count: usize,
    finished: bool,
    records: Vec<T>,
}

fn read_open_library_resumable<T: DeserializeOwned + Send>(
    connection: &mut Connection, path: &Path, limit: Option<usize>, phase: &str, mut import_batch: impl FnMut(&rusqlite::Transaction<'_>, Vec<T>) -> Result<(), MetadataError>,
) -> Result<usize, MetadataError> {
    read_json_resumable(connection, path, limit, phase, OPEN_LIBRARY_CHECKPOINT_RECORDS, DumpFormat::OpenLibraryGzip, &mut import_batch)
}

fn read_json_resumable<T: DeserializeOwned + Send>(
    connection: &mut Connection, path: &Path, limit: Option<usize>, phase: &str, batch_size: usize, format: DumpFormat, import_batch: &mut impl FnMut(&rusqlite::Transaction<'_>, Vec<T>) -> Result<(), MetadataError>,
) -> Result<usize, MetadataError> {
    read_json_resumable_mapped(connection, path, limit, phase, batch_size, PIPELINE_PARSE_RECORDS, format, &Some, import_batch)
}

#[allow(clippy::too_many_arguments)]
fn read_json_resumable_mapped<T: DeserializeOwned + Send, U: Send>(
    connection: &mut Connection, path: &Path, limit: Option<usize>, phase: &str, batch_size: usize, parse_batch_size: usize, format: DumpFormat, transform: &(impl Fn(T) -> Option<U> + Sync),
    import_batch: &mut impl FnMut(&rusqlite::Transaction<'_>, Vec<U>) -> Result<(), MetadataError>,
) -> Result<usize, MetadataError> {
    if let Some(records) = completed_phase(connection, phase)? {
        println!("resuming after completed {phase} phase ({records} records)");
        return usize::try_from(records).map_err(error);
    }
    let resume_at = usize::try_from(checkpoint_records(connection, phase)?).map_err(error)?;
    let path_owned = path.to_owned();
    let phase_owned = phase.to_owned();
    thread::scope(|scope| {
        let (raw_sender, raw_receiver) = mpsc::sync_channel(PIPELINE_BUFFERED_BATCHES);
        let (parsed_sender, parsed_receiver) = mpsc::sync_channel(PIPELINE_BUFFERED_BATCHES);
        let reader_path = path_owned.clone();
        let reader_phase = phase_owned.clone();
        let reader = scope.spawn(move || read_raw_json_batches(&reader_path, format, resume_at, limit, parse_batch_size, &reader_phase, raw_sender));
        let parser_path = path_owned.clone();
        let parser = scope.spawn(move || parse_json_batches::<T, U>(&parser_path, raw_receiver, parsed_sender, transform));

        let mut writer_result = Ok(resume_at);
        let mut last_commit = resume_at;
        let mut pending = Vec::new();
        while let Ok(batch) = parsed_receiver.recv() {
            let pending_end = batch.end_count;
            pending.extend(batch.records);
            if !batch.finished && pending_end.saturating_sub(last_commit) < batch_size {
                continue;
            }
            let result = (|| {
                let transaction = connection.transaction().map_err(error)?;
                import_batch(&transaction, std::mem::take(&mut pending))?;
                transaction
                    .execute(
                        "INSERT INTO import_checkpoint(phase,records_read,completed) VALUES(?1,?2,?3) ON CONFLICT(phase) DO UPDATE SET records_read=excluded.records_read,completed=excluded.completed",
                        params![phase, i64::try_from(pending_end).map_err(error)?, i64::from(batch.finished)],
                    )
                    .map_err(error)?;
                transaction.commit().map_err(error)?;
                last_commit = pending_end;
                if pending_end > 0 && pending_end.is_multiple_of(1_000_000) {
                    match format {
                        DumpFormat::OpenLibraryGzip => println!("read {pending_end} records from {}", path.display()),
                        DumpFormat::Wikidata => println!("read {pending_end} Wikidata entities from {}", path.display()),
                    }
                }
                Ok(pending_end)
            })();
            match result {
                Ok(count) => writer_result = Ok(count),
                Err(failure) => {
                    writer_result = Err(failure);
                    break;
                }
            }
        }
        drop(parsed_receiver);
        let reader_result = reader.join().map_err(|_| MetadataError("dump reader thread panicked".to_owned()))?;
        let parser_result = parser.join().map_err(|_| MetadataError("JSON parser thread panicked".to_owned()))?;
        reader_result?;
        parser_result?;
        writer_result?;
        usize::try_from(checkpoint_records(connection, phase)?).map_err(error)
    })
}

fn read_raw_json_batches(path: &Path, format: DumpFormat, resume_at: usize, limit: Option<usize>, batch_size: usize, phase: &str, sender: mpsc::SyncSender<RawJsonBatch>) -> Result<(), MetadataError> {
    let (mut reader, directly_resumed): (Box<dyn BufRead>, bool) = match format {
        DumpFormat::OpenLibraryGzip => (Box::new(BufReader::with_capacity(1024 * 1024, MultiGzDecoder::new(BufReader::with_capacity(1024 * 1024, File::open(path).map_err(error)?)))), false),
        DumpFormat::Wikidata => wikidata_reader(path, resume_at)?,
    };
    let mut line = String::new();
    let mut count = if directly_resumed { resume_at } else { 0 };
    while count < resume_at {
        line.clear();
        if reader.read_line(&mut line).map_err(error)? == 0 {
            return Err(MetadataError(format!("cannot resume {phase} at record {resume_at}: dump ended after {count} records")));
        }
        if dump_json_line(format, &line).is_some() {
            count += 1;
        }
    }
    if resume_at > 0 {
        if directly_resumed {
            println!("resumed {phase} stream directly at indexed record {resume_at}");
        } else {
            println!("resumed {phase} stream at record {resume_at} after replaying the compressed input");
        }
    }
    loop {
        let mut records = Vec::with_capacity(batch_size);
        let mut finished = limit.is_some_and(|limit| count >= limit);
        while records.len() < batch_size && !finished {
            line.clear();
            if reader.read_line(&mut line).map_err(error)? == 0 {
                finished = true;
                break;
            }
            let Some(json) = dump_json_line(format, &line) else { continue };
            records.push(json.to_owned());
            count += 1;
            if limit.is_some_and(|limit| count >= limit) {
                finished = true;
            }
        }
        if sender.send(RawJsonBatch { end_count: count, finished, records }).is_err() {
            return Ok(());
        }
        if finished {
            return Ok(());
        }
    }
}

fn parse_json_batches<T: DeserializeOwned + Send, U: Send>(path: &Path, receiver: mpsc::Receiver<RawJsonBatch>, sender: mpsc::SyncSender<ParsedJsonBatch<U>>, transform: &(impl Fn(T) -> Option<U> + Sync)) -> Result<(), MetadataError> {
    while let Ok(batch) = receiver.recv() {
        let first_record = batch.end_count.saturating_sub(batch.records.len());
        let records = batch
            .records
            .into_par_iter()
            .enumerate()
            .filter_map(|(index, json)| match serde_json::from_str::<T>(&json) {
                Ok(record) => transform(record),
                Err(parse_error) => {
                    if first_record + index < 10 {
                        tracing::warn!(path = %path.display(), record = first_record + index + 1, error = %parse_error, "skipping malformed dump record");
                    }
                    None
                }
            })
            .collect();
        if sender.send(ParsedJsonBatch { end_count: batch.end_count, finished: batch.finished, records }).is_err() {
            return Ok(());
        }
    }
    Ok(())
}

fn dump_json_line(format: DumpFormat, line: &str) -> Option<&str> {
    match format {
        DumpFormat::OpenLibraryGzip => open_library_json_line(line),
        DumpFormat::Wikidata => wikidata_json_line(line),
    }
}

fn open_library_json_line(line: &str) -> Option<&str> {
    line.splitn(5, '\t').nth(4).map(str::trim_end)
}

struct PreparedEditionQuery {
    query: EditionIdentityQuery,
    author_keys: BTreeSet<String>,
    publisher_keys: BTreeSet<String>,
}

struct EditionCandidate {
    edition_id: i64,
    work_id: Option<i64>,
    edition_title: Option<String>,
    work_title: Option<String>,
    publication_year: Option<i32>,
    title_specificity: usize,
    matched_edition_title: bool,
    matched_work_title: bool,
}

fn resolve_edition_batch(connection: &Connection, queries: Vec<EditionIdentityQuery>) -> Result<Vec<EditionIdentityResult>, MetadataError> {
    let mut prepared = Vec::<Option<PreparedEditionQuery>>::with_capacity(queries.len());
    let mut completed = Vec::<Option<EditionIdentityResult>>::with_capacity(queries.len());
    let mut title_rows = Vec::<(usize, String, usize)>::new();
    for (query_index, query) in queries.into_iter().enumerate() {
        if query.query_id.is_empty() || query.query_id.len() > 256 || query.title.len() > 4_096 || query.authors.len() > 32 || query.publishers.len() > 32 {
            return Err(MetadataError("edition query contains too many or oversized fields".to_owned()));
        }
        let title_keys = title_substring_keys(&title_without_parenthetical_segments(&query.title));
        let author_keys = query.authors.iter().flat_map(|author| author_match_keys(author)).collect::<BTreeSet<_>>();
        let publisher_keys = query.publishers.iter().map(|publisher| bibliographic_match_key(publisher)).filter(|key| !key.is_empty()).collect::<BTreeSet<_>>();
        if title_keys.is_empty() {
            completed.push(Some(identity_result(&query.query_id, EditionIdentityStatus::InsufficientMetadata, None, Vec::new(), false)));
            prepared.push(None);
            continue;
        }
        for (title, specificity) in title_keys {
            title_rows.push((query_index, title, specificity));
        }
        completed.push(None);
        prepared.push(Some(PreparedEditionQuery { query, author_keys, publisher_keys }));
    }

    let mut candidates = load_edition_candidates(connection, &title_rows)?;
    let truncated = candidates.iter().filter(|(_, values)| values.len() > MAX_REVERSE_TITLE_CANDIDATES).map(|(index, _)| *index).collect::<BTreeSet<_>>();
    for query_index in &truncated {
        let query_id = &prepared[*query_index].as_ref().expect("only prepared queries have candidates").query.query_id;
        completed[*query_index] = Some(identity_result(query_id, EditionIdentityStatus::Ambiguous, None, Vec::new(), true));
    }

    let edition_ids = candidates.iter().filter(|(query_index, _)| !truncated.contains(query_index)).flat_map(|(_, values)| values.iter().map(|candidate| candidate.edition_id)).collect::<BTreeSet<_>>();
    let work_ids = candidates.iter().filter(|(query_index, _)| !truncated.contains(query_index)).flat_map(|(_, values)| values.iter().filter_map(|candidate| candidate.work_id)).collect::<BTreeSet<_>>();
    let edition_authors = load_edition_authors(connection, &edition_ids)?;
    let work_authors = load_work_authors(connection, &work_ids)?;
    let publishers = load_edition_publishers(connection, &edition_ids)?;
    let isbns = load_edition_isbns(connection, &edition_ids)?;
    let edition_classifications = load_classifications_by_ids(connection, "edition_classification", "edition_id", &edition_ids)?;
    let work_classifications = load_classifications_by_ids(connection, "work_classification", "work_id", &work_ids)?;
    let sibling_classifications = load_classifications_by_ids(connection, "edition_work_classification", "work_id", &work_ids)?;

    for (query_index, prepared_query) in prepared.iter().enumerate() {
        if completed[query_index].is_some() {
            continue;
        }
        let prepared_query = prepared_query.as_ref().expect("unfinished queries are prepared");
        let query_candidates = candidates.remove(&query_index).unwrap_or_default();
        completed[query_index] = Some(resolve_prepared_edition_query(prepared_query, query_candidates, &edition_authors, &work_authors, &publishers, &isbns, &edition_classifications, &work_classifications, &sibling_classifications));
    }
    Ok(completed.into_iter().map(|result| result.expect("every edition query is completed")).collect())
}

fn edition_candidate_query(title_rows: usize) -> String {
    let values = (0..title_rows).map(|_| "(?,?,?)").collect::<Vec<_>>().join(",");
    format!(
        "WITH request_title(query_index,normalized_title,specificity) AS (VALUES {values}),
         candidate_source(query_index,edition_id,specificity,edition_match,work_match) AS (
             SELECT request.query_index,bibliography.edition_id,request.specificity,1,0
             FROM request_title request
             JOIN edition_bibliography AS bibliography INDEXED BY edition_bibliography_by_title
               ON bibliography.normalized_title=request.normalized_title
             UNION ALL
             SELECT request.query_index,edition.edition_id,request.specificity,0,1
             FROM request_title request
             JOIN work_bibliography AS work_bibliography INDEXED BY work_bibliography_by_title
               ON work_bibliography.normalized_title=request.normalized_title
             JOIN edition AS edition INDEXED BY edition_by_work ON edition.work_id=work_bibliography.work_id
         ),
         candidate_specificity AS (
             SELECT query_index,edition_id,MAX(specificity) AS specificity
             FROM candidate_source GROUP BY query_index,edition_id
         ),
         candidate AS (
             SELECT source.query_index,source.edition_id,score.specificity,MAX(source.edition_match) AS edition_match,MAX(source.work_match) AS work_match
             FROM candidate_source source
             JOIN candidate_specificity score ON score.query_index=source.query_index AND score.edition_id=source.edition_id AND score.specificity=source.specificity
             GROUP BY source.query_index,source.edition_id,score.specificity
         ),
         query_best_specificity AS (
             SELECT query_index,MAX(specificity) AS specificity
             FROM candidate GROUP BY query_index
         ),
         ranked AS (
             SELECT candidate.query_index,candidate.edition_id,candidate.specificity,candidate.edition_match,candidate.work_match,
                    ROW_NUMBER() OVER (PARTITION BY candidate.query_index ORDER BY candidate.edition_id) AS candidate_number
             FROM candidate
             JOIN query_best_specificity best ON best.query_index=candidate.query_index AND best.specificity=candidate.specificity
         )
         SELECT ranked.query_index,edition.edition_id,edition.work_id,bibliography.title,work_bibliography.title,bibliography.publication_year,ranked.specificity,ranked.edition_match,ranked.work_match
         FROM ranked
         JOIN edition ON edition.edition_id=ranked.edition_id
         LEFT JOIN edition_bibliography bibliography ON bibliography.edition_id=edition.edition_id
         LEFT JOIN work_bibliography ON work_bibliography.work_id=edition.work_id
         WHERE ranked.candidate_number<=? ORDER BY ranked.query_index,ranked.edition_id"
    )
}

fn load_edition_candidates(connection: &Connection, title_rows: &[(usize, String, usize)]) -> Result<BTreeMap<usize, Vec<EditionCandidate>>, MetadataError> {
    if title_rows.is_empty() {
        return Ok(BTreeMap::new());
    }
    let sql = edition_candidate_query(title_rows.len());
    let mut parameters = Vec::<rusqlite::types::Value>::with_capacity(title_rows.len() * 3 + 1);
    for (query_index, title, specificity) in title_rows {
        parameters.push(i64::try_from(*query_index).map_err(error)?.into());
        parameters.push(title.clone().into());
        parameters.push(i64::try_from(*specificity).map_err(error)?.into());
    }
    parameters.push(i64::try_from(MAX_REVERSE_TITLE_CANDIDATES + 1).map_err(error)?.into());
    let mut statement = connection.prepare(&sql).map_err(error)?;
    let rows = statement
        .query_map(rusqlite::params_from_iter(parameters), |row| {
            Ok((
                row.get::<_, i64>(0)?,
                EditionCandidate {
                    edition_id: row.get(1)?,
                    work_id: row.get(2)?,
                    edition_title: row.get(3)?,
                    work_title: row.get(4)?,
                    publication_year: row.get(5)?,
                    title_specificity: usize::try_from(row.get::<_, i64>(6)?).map_err(|error| rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Integer, Box::new(error)))?,
                    matched_edition_title: row.get(7)?,
                    matched_work_title: row.get(8)?,
                },
            ))
        })
        .map_err(error)?;
    let mut candidates = BTreeMap::<usize, Vec<EditionCandidate>>::new();
    for row in rows {
        let (query_index, candidate) = row.map_err(error)?;
        candidates.entry(usize::try_from(query_index).map_err(error)?).or_default().push(candidate);
    }
    Ok(candidates)
}

fn load_edition_authors(connection: &Connection, edition_ids: &BTreeSet<i64>) -> Result<HashMap<i64, Vec<String>>, MetadataError> {
    load_names_by_ids(connection, edition_ids, "edition_author", "edition_id")
}

fn load_work_authors(connection: &Connection, work_ids: &BTreeSet<i64>) -> Result<HashMap<i64, Vec<String>>, MetadataError> {
    load_names_by_ids(connection, work_ids, "work_author", "work_id")
}

fn load_names_by_ids(connection: &Connection, ids: &BTreeSet<i64>, relation_table: &str, id_column: &str) -> Result<HashMap<i64, Vec<String>>, MetadataError> {
    let mut names = HashMap::<i64, Vec<String>>::new();
    for chunk in ids.iter().copied().collect::<Vec<_>>().chunks(BULK_EVIDENCE_IDS_PER_QUERY) {
        let sql = format!(
            "SELECT reference.{id_column},author.name FROM {relation_table} reference JOIN author ON author.author_id=reference.author_id WHERE reference.{id_column} IN ({}) AND author.name IS NOT NULL ORDER BY reference.{id_column},reference.position",
            sql_placeholders(chunk.len())
        );
        let mut statement = connection.prepare(&sql).map_err(error)?;
        let rows = statement.query_map(rusqlite::params_from_iter(chunk.iter()), |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))).map_err(error)?;
        for row in rows {
            let (id, name) = row.map_err(error)?;
            names.entry(id).or_default().push(name);
        }
    }
    Ok(names)
}

fn load_edition_publishers(connection: &Connection, edition_ids: &BTreeSet<i64>) -> Result<HashMap<i64, Vec<(String, String)>>, MetadataError> {
    let mut publishers = HashMap::<i64, Vec<(String, String)>>::new();
    for chunk in edition_ids.iter().copied().collect::<Vec<_>>().chunks(BULK_EVIDENCE_IDS_PER_QUERY) {
        let sql = format!("SELECT edition_id,name,normalized_name FROM edition_publisher WHERE edition_id IN ({}) ORDER BY edition_id,position", sql_placeholders(chunk.len()));
        let mut statement = connection.prepare(&sql).map_err(error)?;
        let rows = statement.query_map(rusqlite::params_from_iter(chunk.iter()), |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))).map_err(error)?;
        for row in rows {
            let (edition_id, name, normalized_name) = row.map_err(error)?;
            publishers.entry(edition_id).or_default().push((name, normalized_name));
        }
    }
    Ok(publishers)
}

fn load_edition_isbns(connection: &Connection, edition_ids: &BTreeSet<i64>) -> Result<HashMap<i64, i64>, MetadataError> {
    let mut isbns = HashMap::<i64, i64>::new();
    for chunk in edition_ids.iter().copied().collect::<Vec<_>>().chunks(BULK_EVIDENCE_IDS_PER_QUERY) {
        let sql = format!("SELECT edition_id,isbn13 FROM edition_isbn WHERE edition_id IN ({}) ORDER BY edition_id,isbn13", sql_placeholders(chunk.len()));
        let mut statement = connection.prepare(&sql).map_err(error)?;
        let rows = statement.query_map(rusqlite::params_from_iter(chunk.iter()), |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))).map_err(error)?;
        for row in rows {
            let (edition_id, isbn) = row.map_err(error)?;
            isbns.entry(edition_id).or_insert(isbn);
        }
    }
    Ok(isbns)
}

fn load_classifications_by_ids(connection: &Connection, table: &str, id_column: &str, ids: &BTreeSet<i64>) -> Result<HashMap<i64, Vec<(ClassificationScheme, String)>>, MetadataError> {
    let mut classifications = HashMap::<i64, Vec<(ClassificationScheme, String)>>::new();
    for chunk in ids.iter().copied().collect::<Vec<_>>().chunks(BULK_EVIDENCE_IDS_PER_QUERY) {
        let sql = format!("SELECT {id_column},scheme,notation FROM {table} WHERE {id_column} IN ({}) ORDER BY {id_column},scheme,notation", sql_placeholders(chunk.len()));
        let mut statement = connection.prepare(&sql).map_err(error)?;
        let rows = statement.query_map(rusqlite::params_from_iter(chunk.iter()), |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?))).map_err(error)?;
        for row in rows {
            let (owner_id, scheme, notation) = row.map_err(error)?;
            let scheme = match scheme {
                DDC_SCHEME => ClassificationScheme::DeweyDecimal,
                LCC_SCHEME => ClassificationScheme::LibraryOfCongress,
                _ => continue,
            };
            classifications.entry(owner_id).or_default().push((scheme, notation));
        }
    }
    Ok(classifications)
}

fn resolve_prepared_edition_query(
    prepared: &PreparedEditionQuery, candidates: Vec<EditionCandidate>, edition_authors: &HashMap<i64, Vec<String>>, work_authors: &HashMap<i64, Vec<String>>, publishers: &HashMap<i64, Vec<(String, String)>>, isbns: &HashMap<i64, i64>,
    edition_classifications: &HashMap<i64, Vec<(ClassificationScheme, String)>>, work_classifications: &HashMap<i64, Vec<(ClassificationScheme, String)>>, sibling_classifications: &HashMap<i64, Vec<(ClassificationScheme, String)>>,
) -> EditionIdentityResult {
    let mut editions = Vec::<(usize, bool, usize, EditionIdentityMatch)>::new();
    for candidate in candidates {
        let authors = edition_authors.get(&candidate.edition_id).filter(|authors| !authors.is_empty()).or_else(|| candidate.work_id.and_then(|work_id| work_authors.get(&work_id))).cloned().unwrap_or_default();
        let matched_author = !prepared.author_keys.is_empty() && authors.iter().any(|author| author_match_keys(author).iter().any(|key| prepared.author_keys.contains(key)));
        let publisher_rows = publishers.get(&candidate.edition_id).cloned().unwrap_or_default();
        let matched_publisher = !prepared.publisher_keys.is_empty() && publisher_rows.iter().any(|(_, key)| prepared.publisher_keys.contains(key));
        let matched_publication_year = prepared.query.publication_year.is_some() && prepared.query.publication_year == candidate.publication_year;
        let title_match = match (candidate.matched_edition_title, candidate.matched_work_title) {
            (true, true) => EditionTitleMatch::Both,
            (true, false) => EditionTitleMatch::Edition,
            (false, true) => EditionTitleMatch::Work,
            (false, false) => continue,
        };
        let Some(isbn) = isbns.get(&candidate.edition_id) else { continue };
        let mut classifications = BTreeMap::<(ClassificationScheme, String), ClassificationSource>::new();
        for (scheme, notation) in edition_classifications.get(&candidate.edition_id).into_iter().flatten() {
            classifications.insert((*scheme, notation.clone()), ClassificationSource::ExactEdition);
        }
        if let Some(work_id) = candidate.work_id {
            for (scheme, notation) in work_classifications.get(&work_id).into_iter().flatten() {
                classifications.entry((*scheme, notation.clone())).or_insert(ClassificationSource::Work);
            }
            for (scheme, notation) in sibling_classifications.get(&work_id).into_iter().flatten() {
                classifications.entry((*scheme, notation.clone())).or_insert(ClassificationSource::Work);
            }
        }
        let classifications = metadata_contract::filter_classification_outliers(classifications.into_iter().map(|((scheme, notation), source)| Classification { scheme, notation, source }), classification_similarity_keys);
        editions.push((
            candidate.title_specificity,
            matched_author,
            usize::from(matched_publisher) + usize::from(matched_publication_year),
            EditionIdentityMatch {
                canonical_isbn13: format!("{isbn:013}"),
                open_library_edition_id: format!("OL{}M", candidate.edition_id),
                open_library_work_id: candidate.work_id.map(|work_id| format!("OL{work_id}W")),
                title: candidate.edition_title.clone().unwrap_or_else(|| candidate.work_title.clone().unwrap_or_default()),
                work_title: candidate.work_title,
                title_match,
                authors,
                publishers: publisher_rows.into_iter().map(|(name, _)| name).collect(),
                publication_year: candidate.publication_year,
                matched_publisher,
                matched_publication_year,
                classifications,
            },
        ));
    }
    if editions.len() > 1 {
        let best_specificity = editions.iter().map(|(specificity, _, _, _)| *specificity).max().unwrap_or(0);
        editions.retain(|(specificity, _, _, _)| *specificity == best_specificity);
    }
    if editions.len() > 1 && !prepared.author_keys.is_empty() && editions.iter().any(|(_, matched_author, _, _)| *matched_author) {
        editions.retain(|(_, matched_author, _, _)| *matched_author);
    }
    if editions.len() > 1 && !prepared.author_keys.is_empty() {
        let best_score = editions.iter().map(|(_, _, score, _)| *score).max().unwrap_or(0);
        if best_score > 0 {
            editions.retain(|(_, _, score, _)| *score == best_score);
        }
    }
    if editions.len() > 1 {
        let mut seen_works = BTreeSet::new();
        editions.retain(|(_, _, _, matched)| matched.open_library_work_id.as_ref().is_none_or(|work_id| seen_works.insert(work_id.clone())));
    }
    // Title agreement identifies candidates, but it is not enough to attach
    // an external edition automatically: generic titles such as "Complete
    // Works" routinely collide across unrelated authors. Publisher and year
    // may rank editions of the same work, while at least one author must agree
    // before the remaining candidate is safe to accept without review.
    let automatic_match = editions.len() == 1 && editions[0].1;
    let mut strong = editions.into_iter().map(|(_, _, _, matched)| matched).collect::<Vec<_>>();
    match strong.len() {
        0 => identity_result(&prepared.query.query_id, EditionIdentityStatus::NoMatch, None, Vec::new(), false),
        1 if automatic_match => identity_result(&prepared.query.query_id, EditionIdentityStatus::Matched, strong.pop(), Vec::new(), false),
        _ => {
            let candidates_truncated = strong.len() > MAX_EDITION_IDENTITY_REVIEW_CANDIDATES;
            strong.truncate(MAX_EDITION_IDENTITY_REVIEW_CANDIDATES);
            identity_result(&prepared.query.query_id, EditionIdentityStatus::Ambiguous, None, strong, candidates_truncated)
        }
    }
}

fn identity_result(query_id: &str, status: EditionIdentityStatus, matched: Option<EditionIdentityMatch>, candidates: Vec<EditionIdentityMatch>, candidates_truncated: bool) -> EditionIdentityResult {
    EditionIdentityResult { query_id: query_id.to_owned(), status, matched, candidates, candidates_truncated }
}

fn title_without_parenthetical_segments(title: &str) -> String {
    let mut depth = 0_u32;
    title
        .chars()
        .filter(|character| match character {
            '(' => {
                depth = depth.saturating_add(1);
                false
            }
            ')' if depth > 0 => {
                depth -= 1;
                false
            }
            _ => depth == 0,
        })
        .collect()
}

fn lookup_one(connection: &Connection, requested_isbn: String) -> Result<IsbnClassificationResult, MetadataError> {
    let Some(isbn13) = canonical_isbn13(&requested_isbn) else {
        return Ok(IsbnClassificationResult { requested_isbn, canonical_isbn13: None, status: LookupStatus::InvalidIsbn, matches: Vec::new() });
    };
    let mut editions = connection
        .prepare_cached("SELECT edition.edition_id,edition.work_id FROM edition_isbn JOIN edition ON edition.edition_id=edition_isbn.edition_id WHERE edition_isbn.isbn13=?1 ORDER BY edition.work_id,edition.edition_id")
        .map_err(error)?;
    let rows = editions.query_map([isbn13], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?))).map_err(error)?;
    let mut by_work = BTreeMap::<(Option<i64>, i64), Vec<i64>>::new();
    for row in rows {
        let (edition_id, work_id) = row.map_err(error)?;
        by_work.entry((work_id, work_id.unwrap_or(edition_id))).or_default().push(edition_id);
    }
    let mut matches = Vec::with_capacity(by_work.len());
    for ((work_id, _group_id), edition_ids) in by_work {
        let mut classifications = BTreeMap::<(ClassificationScheme, String), ClassificationSource>::new();
        for edition_id in &edition_ids {
            collect_classifications(connection, "edition_classification", "edition_id", *edition_id, ClassificationSource::ExactEdition, &mut classifications)?;
        }
        if let Some(work_id) = work_id {
            for classification in collect_sibling_edition_consensus(connection, work_id)? {
                classifications.entry((classification.scheme, classification.notation)).or_insert(ClassificationSource::Work);
            }
        }
        let classifications = metadata_contract::filter_classification_outliers(classifications.into_iter().map(|((scheme, notation), source)| Classification { scheme, notation, source }), classification_similarity_keys);
        matches.push(WorkClassificationMatch { open_library_work_id: work_id.map(|work_id| format!("OL{work_id}W")), exact_edition_ids: edition_ids.into_iter().map(|edition_id| format!("OL{edition_id}M")).collect(), classifications });
    }
    let status = match matches.len() {
        0 => LookupStatus::NoMatch,
        1 => LookupStatus::Matched,
        _ => LookupStatus::Ambiguous,
    };
    Ok(IsbnClassificationResult { requested_isbn, canonical_isbn13: Some(format!("{isbn13:013}")), status, matches })
}

fn lookup_metadata_one(connection: &Connection, requested_isbn: String) -> Result<IsbnMetadataResult, MetadataError> {
    let classification = lookup_one(connection, requested_isbn)?;
    let matches = classification
        .matches
        .into_iter()
        .map(|matched| WorkMetadataMatch { open_library_work_id: matched.open_library_work_id, exact_edition_ids: matched.exact_edition_ids, classifications: matched.classifications, authors: Vec::new() })
        .collect();
    Ok(IsbnMetadataResult { requested_isbn: classification.requested_isbn, canonical_isbn13: classification.canonical_isbn13, status: classification.status, matches })
}

fn attach_rich_fields(connection: &Connection, metadata: Vec<IsbnMetadataResult>) -> Result<Vec<RichIsbnMetadataResult>, MetadataError> {
    let edition_ids = metadata.iter().flat_map(|result| &result.matches).flat_map(|matched| &matched.exact_edition_ids).filter_map(|value| open_library_id(value, 'M')).collect::<BTreeSet<_>>();
    let work_ids = metadata.iter().flat_map(|result| &result.matches).filter_map(|matched| matched.open_library_work_id.as_deref().and_then(|value| open_library_id(value, 'W'))).collect::<BTreeSet<_>>();
    let edition_evidence = load_entity_evidence(connection, "edition_entity_evidence", "edition_id", &edition_ids)?;
    let work_evidence = load_entity_evidence(connection, "work_entity_evidence", "work_id", &work_ids)?;
    let popularity = load_work_popularities(connection, &work_ids)?;

    metadata
        .into_iter()
        .map(|result| {
            let matches = result
                .matches
                .into_iter()
                .map(|matched| {
                    let mut places = BTreeMap::<(String, Option<i64>), EntitySource>::new();
                    let mut people = BTreeMap::<(String, Option<i64>), EntitySource>::new();
                    for edition_id in matched.exact_edition_ids.iter().filter_map(|value| open_library_id(value, 'M')) {
                        merge_entity_evidence(edition_evidence.get(&edition_id), EntitySource::ExactEdition, &mut places, &mut people);
                    }
                    let work_id = matched.open_library_work_id.as_deref().and_then(|value| open_library_id(value, 'W'));
                    if let Some(work_id) = work_id {
                        merge_entity_evidence(work_evidence.get(&work_id), EntitySource::Work, &mut places, &mut people);
                    }
                    let references =
                        |evidence: BTreeMap<(String, Option<i64>), EntitySource>| evidence.into_iter().map(|((name, wikidata_id), source)| EntityReference { wikidata_id: wikidata_id.map(|id| format!("Q{id}")), name, source }).collect();
                    RichWorkMetadataMatch {
                        open_library_work_id: matched.open_library_work_id,
                        exact_edition_ids: matched.exact_edition_ids,
                        classifications: matched.classifications,
                        authors: matched.authors,
                        places: references(places),
                        people: references(people),
                        popularity: work_id.and_then(|work_id| popularity.get(&work_id).cloned()),
                    }
                })
                .collect();
            Ok(RichIsbnMetadataResult { requested_isbn: result.requested_isbn, canonical_isbn13: result.canonical_isbn13, status: result.status, matches })
        })
        .collect()
}

fn load_work_popularities(connection: &Connection, ids: &BTreeSet<i64>) -> Result<BTreeMap<i64, metadata_contract::WorkPopularity>, MetadataError> {
    let mut popularity = BTreeMap::new();
    let table_exists = connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='work_popularity')", (), |row| row.get::<_, bool>(0)).map_err(error)?;
    if !table_exists {
        return Ok(popularity);
    }
    for chunk in ids.iter().copied().collect::<Vec<_>>().chunks(500) {
        let sql = format!("SELECT work_id,want_to_read_count,currently_reading_count,already_read_count,rating_count,rating_sum FROM work_popularity WHERE work_id IN ({})", sql_placeholders(chunk.len()));
        let mut statement = connection.prepare(&sql).map_err(error)?;
        let rows = statement
            .query_map(rusqlite::params_from_iter(chunk.iter()), |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    metadata_contract::WorkPopularity {
                        want_to_read_count: u64::try_from(row.get::<_, i64>(1)?).unwrap_or(0),
                        currently_reading_count: u64::try_from(row.get::<_, i64>(2)?).unwrap_or(0),
                        already_read_count: u64::try_from(row.get::<_, i64>(3)?).unwrap_or(0),
                        rating_count: u64::try_from(row.get::<_, i64>(4)?).unwrap_or(0),
                        rating_sum: u64::try_from(row.get::<_, i64>(5)?).unwrap_or(0),
                    },
                ))
            })
            .map_err(error)?;
        for row in rows {
            let (work_id, value) = row.map_err(error)?;
            popularity.insert(work_id, value);
        }
    }
    Ok(popularity)
}

fn collect_response_entities(connection: &Connection, results: &[RichIsbnMetadataResult]) -> Result<(Vec<PlaceEntity>, Vec<PersonEntity>), MetadataError> {
    let mut pending_places = results.iter().flat_map(|result| &result.matches).flat_map(|matched| &matched.places).filter_map(|reference| reference.wikidata_id.as_deref().and_then(wikidata_id)).collect::<BTreeSet<_>>();
    let person_ids = results.iter().flat_map(|result| &result.matches).flat_map(|matched| &matched.people).filter_map(|reference| reference.wikidata_id.as_deref().and_then(wikidata_id)).collect::<BTreeSet<_>>();
    let mut seen_places = BTreeSet::new();
    let mut places = Vec::new();
    loop {
        let batch = pending_places.difference(&seen_places).copied().collect::<Vec<_>>();
        if batch.is_empty() {
            break;
        }
        seen_places.extend(batch.iter().copied());
        let loaded = load_place_entities(connection, &batch)?;
        for place in loaded {
            pending_places.extend(place.parent_ids.iter().chain(&place.country_ids).filter_map(|id| wikidata_id(id)));
            places.push(place);
        }
    }
    places.sort_by(|left, right| left.wikidata_id.cmp(&right.wikidata_id));
    let mut people = load_person_entities(connection, &person_ids.into_iter().collect::<Vec<_>>())?;
    people.sort_by(|left, right| left.wikidata_id.cmp(&right.wikidata_id));
    Ok((places, people))
}

fn load_place_entities(connection: &Connection, ids: &[i64]) -> Result<Vec<PlaceEntity>, MetadataError> {
    let mut entities = BTreeMap::new();
    for chunk in ids.chunks(500) {
        let sql = format!("SELECT wikidata_id,label,latitude_e7,longitude_e7 FROM entity WHERE kind={OL_PLACE_KIND} AND wikidata_id IN ({})", sql_placeholders(chunk.len()));
        let mut statement = connection.prepare(&sql).map_err(error)?;
        let rows = statement.query_map(rusqlite::params_from_iter(chunk.iter()), |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<i32>>(2)?, row.get::<_, Option<i32>>(3)?))).map_err(error)?;
        for row in rows {
            let (id, label, latitude, longitude) = row.map_err(error)?;
            entities.insert(id, (label, latitude, longitude));
        }
    }
    let aliases = load_entity_aliases(connection, ids)?;
    let parents = load_entity_edges(connection, "place_parent", "place_id", "parent_id", ids)?;
    let countries = load_entity_edges(connection, "place_country", "place_id", "country_id", ids)?;
    Ok(entities
        .into_iter()
        .map(|(id, (label, latitude_e7, longitude_e7))| PlaceEntity {
            wikidata_id: format!("Q{id}"),
            label,
            aliases: aliases.get(&id).cloned().unwrap_or_default(),
            parent_ids: parents.get(&id).into_iter().flatten().map(|value| format!("Q{value}")).collect(),
            country_ids: countries.get(&id).into_iter().flatten().map(|value| format!("Q{value}")).collect(),
            latitude_e7,
            longitude_e7,
        })
        .collect())
}

fn load_person_entities(connection: &Connection, ids: &[i64]) -> Result<Vec<PersonEntity>, MetadataError> {
    let aliases = load_entity_aliases(connection, ids)?;
    let mut people = Vec::new();
    for chunk in ids.chunks(500) {
        let sql = format!("SELECT wikidata_id,label,birth_year,death_year,fictional FROM entity WHERE kind={OL_PERSON_KIND} AND wikidata_id IN ({})", sql_placeholders(chunk.len()));
        let mut statement = connection.prepare(&sql).map_err(error)?;
        let rows =
            statement.query_map(rusqlite::params_from_iter(chunk.iter()), |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<i32>>(2)?, row.get::<_, Option<i32>>(3)?, row.get::<_, bool>(4)?))).map_err(error)?;
        for row in rows {
            let (id, label, birth_year, death_year, fictional) = row.map_err(error)?;
            people.push(PersonEntity { wikidata_id: format!("Q{id}"), label, aliases: aliases.get(&id).cloned().unwrap_or_default(), birth_year, death_year, fictional });
        }
    }
    Ok(people)
}

fn load_entity_aliases(connection: &Connection, ids: &[i64]) -> Result<BTreeMap<i64, Vec<String>>, MetadataError> {
    let mut aliases = BTreeMap::<i64, Vec<String>>::new();
    for chunk in ids.chunks(500) {
        let sql = format!("SELECT wikidata_id,alias FROM entity_alias WHERE wikidata_id IN ({}) ORDER BY wikidata_id,alias", sql_placeholders(chunk.len()));
        let mut statement = connection.prepare(&sql).map_err(error)?;
        let rows = statement.query_map(rusqlite::params_from_iter(chunk.iter()), |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))).map_err(error)?;
        for row in rows {
            let (id, alias) = row.map_err(error)?;
            aliases.entry(id).or_default().push(alias);
        }
    }
    Ok(aliases)
}

fn load_entity_edges(connection: &Connection, table: &str, source_column: &str, target_column: &str, ids: &[i64]) -> Result<BTreeMap<i64, Vec<i64>>, MetadataError> {
    let mut edges = BTreeMap::<i64, Vec<i64>>::new();
    for chunk in ids.chunks(500) {
        let sql = format!("SELECT {source_column},{target_column} FROM {table} WHERE {source_column} IN ({}) ORDER BY {source_column},{target_column}", sql_placeholders(chunk.len()));
        let mut statement = connection.prepare(&sql).map_err(error)?;
        let rows = statement.query_map(rusqlite::params_from_iter(chunk.iter()), |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))).map_err(error)?;
        for row in rows {
            let (source, target) = row.map_err(error)?;
            edges.entry(source).or_default().push(target);
        }
    }
    Ok(edges)
}

fn sql_placeholders(count: usize) -> String {
    std::iter::repeat_n("?", count).collect::<Vec<_>>().join(",")
}

fn load_entity_evidence(connection: &Connection, table: &str, id_column: &str, ids: &BTreeSet<i64>) -> Result<BTreeMap<i64, Vec<(i64, String, Option<i64>)>>, MetadataError> {
    let mut evidence = BTreeMap::<i64, Vec<(i64, String, Option<i64>)>>::new();
    for chunk in ids.iter().copied().collect::<Vec<_>>().chunks(500) {
        let sql = format!("SELECT {id_column},kind,name,wikidata_id FROM {table} WHERE {id_column} IN ({}) ORDER BY {id_column},kind,name", sql_placeholders(chunk.len()));
        let mut statement = connection.prepare(&sql).map_err(error)?;
        let rows = statement.query_map(rusqlite::params_from_iter(chunk.iter()), |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?, row.get::<_, Option<i64>>(3)?))).map_err(error)?;
        for row in rows {
            let (owner_id, kind, name, wikidata_id) = row.map_err(error)?;
            evidence.entry(owner_id).or_default().push((kind, name, wikidata_id));
        }
    }
    Ok(evidence)
}

fn merge_entity_evidence(evidence: Option<&Vec<(i64, String, Option<i64>)>>, source: EntitySource, places: &mut BTreeMap<(String, Option<i64>), EntitySource>, people: &mut BTreeMap<(String, Option<i64>), EntitySource>) {
    for (kind, name, wikidata_id) in evidence.into_iter().flatten() {
        let key = (name.clone(), *wikidata_id);
        match *kind {
            OL_PLACE_KIND => {
                places.entry(key).and_modify(|existing| *existing = (*existing).min(source)).or_insert(source);
            }
            OL_PERSON_KIND => {
                people.entry(key).and_modify(|existing| *existing = (*existing).min(source)).or_insert(source);
            }
            _ => {}
        }
    }
}

fn attach_authors(connection: &Connection, results: &mut [IsbnMetadataResult]) -> Result<(), MetadataError> {
    let edition_ids = results.iter().flat_map(|result| &result.matches).flat_map(|matched| &matched.exact_edition_ids).filter_map(|value| open_library_id(value, 'M')).collect::<BTreeSet<_>>();
    let work_ids = results.iter().flat_map(|result| &result.matches).filter_map(|matched| matched.open_library_work_id.as_deref().and_then(|value| open_library_id(value, 'W'))).collect::<BTreeSet<_>>();
    let edition_authors = load_author_relations(connection, "edition_author", "edition_id", &edition_ids)?;
    let work_authors = load_author_relations(connection, "work_author", "work_id", &work_ids)?;
    let author_ids = edition_authors.values().chain(work_authors.values()).flatten().map(|(author_id, _)| *author_id).collect::<BTreeSet<_>>();
    let names = load_author_names(connection, &author_ids)?;
    let identifiers = load_author_identifiers(connection, &author_ids)?;

    for matched in results.iter_mut().flat_map(|result| &mut result.matches) {
        let mut authors = BTreeMap::<i64, (AuthorSource, u32)>::new();
        for edition_id in matched.exact_edition_ids.iter().filter_map(|value| open_library_id(value, 'M')) {
            merge_author_relations(&mut authors, edition_authors.get(&edition_id), AuthorSource::ExactEdition);
        }
        if let Some(work_id) = matched.open_library_work_id.as_deref().and_then(|value| open_library_id(value, 'W')) {
            merge_author_relations(&mut authors, work_authors.get(&work_id), AuthorSource::Work);
        }
        matched.authors = authors
            .into_iter()
            .map(|(author_id, (source, position))| AuthorMetadata {
                open_library_author_id: format!("OL{author_id}A"),
                name: names.get(&author_id).cloned().flatten(),
                identifiers: identifiers.get(&author_id).cloned().unwrap_or_default(),
                source,
                position,
            })
            .collect();
        matched.authors.sort_by_key(|author| (author.source, author.position, author.open_library_author_id.clone()));
    }
    Ok(())
}

fn load_author_relations(connection: &Connection, table: &str, id_column: &str, ids: &BTreeSet<i64>) -> Result<BTreeMap<i64, Vec<(i64, u32)>>, MetadataError> {
    let mut relations = BTreeMap::<i64, Vec<(i64, u32)>>::new();
    for chunk in ids.iter().copied().collect::<Vec<_>>().chunks(500) {
        let sql = format!("SELECT {id_column},author_id,position FROM {table} WHERE {id_column} IN ({}) ORDER BY {id_column},position", sql_placeholders(chunk.len()));
        let mut statement = connection.prepare(&sql).map_err(error)?;
        let rows = statement.query_map(rusqlite::params_from_iter(chunk.iter()), |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?))).map_err(error)?;
        for row in rows {
            let (owner_id, author_id, position) = row.map_err(error)?;
            relations.entry(owner_id).or_default().push((author_id, u32::try_from(position).map_err(error)?));
        }
    }
    Ok(relations)
}

fn merge_author_relations(authors: &mut BTreeMap<i64, (AuthorSource, u32)>, relations: Option<&Vec<(i64, u32)>>, source: AuthorSource) {
    for &(author_id, position) in relations.into_iter().flatten() {
        authors
            .entry(author_id)
            .and_modify(|existing| {
                if (source, position) < *existing {
                    *existing = (source, position);
                }
            })
            .or_insert((source, position));
    }
}

fn load_author_names(connection: &Connection, ids: &BTreeSet<i64>) -> Result<BTreeMap<i64, Option<String>>, MetadataError> {
    let mut names = BTreeMap::new();
    for chunk in ids.iter().copied().collect::<Vec<_>>().chunks(500) {
        let sql = format!("SELECT author_id,name FROM author WHERE author_id IN ({})", sql_placeholders(chunk.len()));
        let mut statement = connection.prepare(&sql).map_err(error)?;
        let rows = statement.query_map(rusqlite::params_from_iter(chunk.iter()), |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?))).map_err(error)?;
        for row in rows {
            let (author_id, name) = row.map_err(error)?;
            names.insert(author_id, name);
        }
    }
    Ok(names)
}

fn load_author_identifiers(connection: &Connection, ids: &BTreeSet<i64>) -> Result<BTreeMap<i64, Vec<AuthorIdentifier>>, MetadataError> {
    let mut identifiers = BTreeMap::<i64, Vec<AuthorIdentifier>>::new();
    for chunk in ids.iter().copied().collect::<Vec<_>>().chunks(500) {
        let sql = format!("SELECT author_id,authority,external_id FROM author_identifier WHERE author_id IN ({}) ORDER BY author_id,authority,external_id", sql_placeholders(chunk.len()));
        let mut statement = connection.prepare(&sql).map_err(error)?;
        let rows = statement.query_map(rusqlite::params_from_iter(chunk.iter()), |row| Ok((row.get::<_, i64>(0)?, AuthorIdentifier { authority: row.get(1)?, value: row.get(2)? }))).map_err(error)?;
        for row in rows {
            let (author_id, identifier) = row.map_err(error)?;
            identifiers.entry(author_id).or_default().push(identifier);
        }
    }
    Ok(identifiers)
}

fn collect_classifications(connection: &Connection, table: &str, id_column: &str, id: i64, source: ClassificationSource, classifications: &mut BTreeMap<(ClassificationScheme, String), ClassificationSource>) -> Result<(), MetadataError> {
    let sql = format!("SELECT scheme,notation FROM {table} WHERE {id_column}=?1 ORDER BY scheme,notation");
    let mut statement = connection.prepare_cached(&sql).map_err(error)?;
    let rows = statement.query_map([id], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))).map_err(error)?;
    for row in rows {
        let (scheme, notation) = row.map_err(error)?;
        let scheme = match scheme {
            DDC_SCHEME => ClassificationScheme::DeweyDecimal,
            LCC_SCHEME => ClassificationScheme::LibraryOfCongress,
            _ => continue,
        };
        classifications.entry((scheme, notation)).and_modify(|existing| *existing = (*existing).min(source)).or_insert(source);
    }
    Ok(())
}

fn collect_sibling_edition_consensus(connection: &Connection, work_id: i64) -> Result<Vec<Classification>, MetadataError> {
    let mut statement = connection
        .prepare_cached(
            "SELECT edition_classification.edition_id,edition_classification.scheme,edition_classification.notation
             FROM edition_classification
             JOIN edition ON edition.edition_id=edition_classification.edition_id
             WHERE edition.work_id=?1
             ORDER BY edition_classification.edition_id,edition_classification.scheme,edition_classification.notation",
        )
        .map_err(error)?;
    let rows = statement.query_map([work_id], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?))).map_err(error)?;
    let mut support = BTreeMap::<(ClassificationScheme, String), BTreeSet<i64>>::new();
    for row in rows {
        let (edition_id, scheme, notation) = row.map_err(error)?;
        let scheme = match scheme {
            DDC_SCHEME => ClassificationScheme::DeweyDecimal,
            LCC_SCHEME => ClassificationScheme::LibraryOfCongress,
            _ => continue,
        };
        let Some(notation) = metadata_contract::classification_consensus_notation(scheme, &notation) else { continue };
        support.entry((scheme, notation)).or_default().insert(edition_id);
    }
    Ok(support.into_iter().filter_map(|((scheme, notation), editions)| (editions.len() >= 2).then_some(Classification { scheme, notation, source: ClassificationSource::Work })).collect())
}

fn classification_similarity_keys(scheme: ClassificationScheme, notation: &str) -> Vec<String> {
    let system_id = match scheme {
        ClassificationScheme::LibraryOfCongress => library_model::LCC_SYSTEM_ID,
        ClassificationScheme::DeweyDecimal => library_model::DDC_SYSTEM_ID,
    };
    library_model::unified_subject_similarity_keys(system_id, notation)
}

fn open_read_pool(path: &Path) -> Result<Pool<SqliteConnectionManager>, MetadataError> {
    let manager = SqliteConnectionManager::file(path).with_flags(OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).with_init(configure_read_only);
    Pool::builder().max_size(QUERY_CONCURRENCY).min_idle(Some(1)).build(manager).map_err(error)
}

fn configure_read_only(connection: &mut Connection) -> rusqlite::Result<()> {
    connection.execute_batch(&format!("PRAGMA query_only=ON; PRAGMA cache_size=-{READ_CACHE_KIB}; PRAGMA mmap_size={READ_MMAP_BYTES};"))
}

fn open_library_id(value: &str, suffix: char) -> Option<i64> {
    let digits = value
        .strip_prefix(if suffix == 'M' {
            "/books/OL"
        } else if suffix == 'W' {
            "/works/OL"
        } else if suffix == 'A' {
            "/authors/OL"
        } else {
            return None;
        })
        .or_else(|| value.strip_prefix("OL"))?
        .strip_suffix(suffix)?;
    digits.parse().ok()
}

fn deserialize_string_array<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Vec<String>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(match value {
        serde_json::Value::String(value) => vec![value],
        serde_json::Value::Array(values) => values.into_iter().filter_map(|value| value.as_str().map(ToOwned::to_owned)).collect(),
        _ => Vec::new(),
    })
}

fn deserialize_optional_string<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(match value {
        serde_json::Value::String(value) => Some(value),
        serde_json::Value::Number(value) => Some(value.to_string()),
        _ => None,
    })
}

fn deserialize_key_references<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Vec<KeyReference>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    let values = match value {
        serde_json::Value::Array(values) => values,
        value @ serde_json::Value::Object(_) => vec![value],
        _ => Vec::new(),
    };
    Ok(values.into_iter().filter_map(|value| value.get("key").and_then(serde_json::Value::as_str).map(|key| KeyReference { key: key.to_owned() })).collect())
}

fn deserialize_work_author_references<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Vec<KeyReference>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    let values = match value {
        serde_json::Value::Array(values) => values,
        value @ serde_json::Value::Object(_) => vec![value],
        _ => Vec::new(),
    };
    Ok(values.into_iter().filter_map(|value| value.get("author").and_then(|author| author.get("key")).or_else(|| value.get("key")).and_then(serde_json::Value::as_str).map(|key| KeyReference { key: key.to_owned() })).collect())
}

fn canonical_isbn13(value: &str) -> Option<i64> {
    let compact = value.chars().filter(|character| character.is_ascii_digit() || matches!(character, 'X' | 'x')).collect::<String>();
    let isbn13 = match compact.len() {
        10 if valid_isbn10(&compact) => {
            let mut first_twelve = format!("978{}", &compact[..9]);
            let sum = first_twelve.bytes().enumerate().map(|(index, byte)| i64::from(byte - b'0') * if index % 2 == 0 { 1 } else { 3 }).sum::<i64>();
            first_twelve.push(char::from(b'0' + ((10 - sum % 10) % 10) as u8));
            first_twelve
        }
        13 if compact.bytes().all(|byte| byte.is_ascii_digit()) && valid_isbn13(&compact) => compact,
        _ => return None,
    };
    isbn13.parse().ok()
}

fn valid_isbn10(value: &str) -> bool {
    value.bytes().enumerate().all(|(index, byte)| byte.is_ascii_digit() || (index == 9 && matches!(byte, b'X' | b'x')))
        && value.bytes().enumerate().map(|(index, byte)| (10 - index as u32) * if matches!(byte, b'X' | b'x') { 10 } else { u32::from(byte - b'0') }).sum::<u32>() % 11 == 0
}

fn valid_isbn13(value: &str) -> bool {
    value.bytes().enumerate().map(|(index, byte)| u32::from(byte - b'0') * if index % 2 == 0 { 1 } else { 3 }).sum::<u32>() % 10 == 0
}

fn building_path(output: &Path) -> PathBuf {
    let mut name = output.file_name().unwrap_or_default().to_os_string();
    name.push(".building");
    output.with_file_name(name)
}

fn dump_date(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_string_lossy();
    name.as_bytes().windows(10).find_map(|window| {
        let value = std::str::from_utf8(window).ok()?;
        (value.as_bytes().get(4) == Some(&b'-') && value.as_bytes().get(7) == Some(&b'-') && value.chars().enumerate().all(|(index, character)| matches!(index, 4 | 7) || character.is_ascii_digit())).then(|| value.to_owned())
    })
}

fn error(error: impl fmt::Display) -> MetadataError {
    MetadataError(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;

    fn dump(path: &Path, records: &[serde_json::Value]) {
        let file = File::create(path).unwrap();
        let mut gzip = GzEncoder::new(file, Compression::fast());
        for record in records {
            writeln!(gzip, "/type\t/key\t1\t2026-08-01T00:00:00.000000\t{record}").unwrap();
        }
        gzip.finish().unwrap();
    }

    fn tsv_dump(path: &Path, lines: &[&str]) {
        let file = File::create(path).unwrap();
        let mut gzip = GzEncoder::new(file, Compression::fast());
        for line in lines {
            writeln!(gzip, "{line}").unwrap();
        }
        gzip.finish().unwrap();
    }

    fn entity_claim(id: &str) -> serde_json::Value {
        serde_json::json!({"mainsnak":{"datavalue":{"value":{"id":id}}}})
    }

    fn snapshot_from(directory: &Path, edition_records: &[serde_json::Value], work_records: &[serde_json::Value], author_records: &[serde_json::Value]) -> PathBuf {
        let editions = directory.join("ol_dump_editions_2026-08-01.txt.gz");
        let works = directory.join("ol_dump_works_2026-08-01.txt.gz");
        let authors = directory.join("ol_dump_authors_2026-08-01.txt.gz");
        let database = directory.join("snapshot.sqlite");
        dump(&editions, edition_records);
        dump(&works, work_records);
        dump(&authors, author_records);
        import_snapshot(&editions, &works, &authors, &database, None).unwrap();
        database
    }

    fn edition_query(title: &str, publishers: Vec<String>, publication_year: Option<i32>) -> EditionIdentityRequest {
        EditionIdentityRequest { queries: vec![EditionIdentityQuery { query_id: "query".to_owned(), title: title.to_owned(), authors: vec!["Alex Smith".to_owned()], publishers, publication_year }] }
    }

    fn wikidata_dump(path: &Path, records: &[serde_json::Value]) {
        let mut file = File::create(path).unwrap();
        writeln!(file, "[").unwrap();
        for (index, record) in records.iter().enumerate() {
            writeln!(file, "{record}{}", if index + 1 == records.len() { "" } else { "," }).unwrap();
        }
        writeln!(file, "]").unwrap();
    }

    #[test]
    fn edition_identity_candidate_query_uses_title_and_work_indexes() {
        let temporary = tempfile::tempdir().unwrap();
        let database = snapshot_from(
            temporary.path(),
            &[serde_json::json!({
                "key": "/books/OL1M", "works": [{"key": "/works/OL10W"}],
                "title": "The Example Book", "isbn_13": ["9780306406157"]
            })],
            &[serde_json::json!({"key": "/works/OL10W", "title": "Canonical Example Work"})],
            &[],
        );
        let connection = Connection::open(database).unwrap();
        // A one-row fixture makes SQLite rationally scan the outer detail
        // table. Give the planner production-like cardinality so this test
        // distinguishes that harmless fixture choice from the old candidate
        // full scan.
        connection
            .execute_batch(
                "WITH RECURSIVE ids(value) AS (SELECT 2 UNION ALL SELECT value+1 FROM ids WHERE value<10000)
                 INSERT INTO edition(edition_id,work_id) SELECT value,NULL FROM ids;
                 ANALYZE;",
            )
            .unwrap();
        let query = edition_candidate_query(1);
        let mut statement = connection.prepare(&format!("EXPLAIN QUERY PLAN {query}")).unwrap();
        let plan = statement.query_map(params![0, "the example book", 16, MAX_REVERSE_TITLE_CANDIDATES + 1], |row| row.get::<_, String>(3)).unwrap().collect::<Result<Vec<_>, _>>().unwrap().join("\n");

        assert!(plan.contains("edition_bibliography_by_title"), "edition title index missing from plan:\n{plan}");
        assert!(plan.contains("work_bibliography_by_title"), "work title index missing from plan:\n{plan}");
        assert!(plan.contains("edition_by_work"), "work-to-edition index missing from plan:\n{plan}");
        assert!(!plan.lines().any(|line| line.contains("SCAN edition")), "candidate lookup full-scans edition:\n{plan}");
    }

    #[test]
    fn read_pool_bounds_connections_and_sqlite_memory_budgets() {
        let temporary = tempfile::tempdir().unwrap();
        let database = snapshot_from(temporary.path(), &[], &[], &[]);
        let service = MetadataService::open(database).unwrap();
        let connections = (0..QUERY_CONCURRENCY).map(|_| service.connection().unwrap()).collect::<Vec<_>>();

        assert!(service.state.pool.try_get().is_none(), "pool exceeded its configured connection ceiling");
        assert_eq!(service.state.query_slots.available_permits(), QUERY_CONCURRENCY as usize);
        assert_eq!(connections[0].query_row("PRAGMA cache_size", (), |row| row.get::<_, i64>(0)).unwrap(), -(READ_CACHE_KIB as i64));
        assert_eq!(connections[0].query_row("PRAGMA mmap_size", (), |row| row.get::<_, i64>(0)).unwrap(), READ_MMAP_BYTES as i64);
    }

    #[test]
    fn popularity_snapshot_is_schema_v8_without_resolved_wikidata_entities() {
        let temporary = tempfile::tempdir().unwrap();
        let editions = temporary.path().join("ol_dump_editions_2026-08-01.txt.gz");
        let works = temporary.path().join("ol_dump_works_2026-08-01.txt.gz");
        let authors = temporary.path().join("ol_dump_authors_2026-08-01.txt.gz");
        let ratings = temporary.path().join("ol_dump_ratings_2026-08-01.txt.gz");
        let reading_log = temporary.path().join("ol_dump_reading-log_2026-08-01.txt.gz");
        let database = temporary.path().join("popularity.sqlite");
        dump(
            &editions,
            &[serde_json::json!({
                "key": "/books/OL1M", "works": [{"key": "/works/OL10W"}],
                "title": "Example", "isbn_13": ["9780306406157"], "subject_places": ["Paris"]
            })],
        );
        dump(&works, &[serde_json::json!({"key": "/works/OL10W", "title": "Example"})]);
        dump(&authors, &[]);
        tsv_dump(&ratings, &["/works/OL10W\t/books/OL1M\t5\t2026-01-01"]);
        tsv_dump(&reading_log, &["/works/OL10W\t/books/OL1M\tWant to Read\t2026-01-01"]);

        import_snapshot_with_popularity(&editions, &works, &authors, &ratings, &reading_log, &database, None).unwrap();
        let validation = Connection::open(&database).unwrap();
        assert_eq!(validation.query_row("SELECT schema_version FROM snapshot WHERE singleton=1", (), |row| row.get::<_, i64>(0)).unwrap(), SCHEMA_VERSION);
        assert_eq!(validation.query_row("SELECT COUNT(*) FROM entity", (), |row| row.get::<_, i64>(0)).unwrap(), 0);
        assert_eq!(validation.query_row("SELECT rating_count FROM work_popularity WHERE work_id=10", (), |row| row.get::<_, i64>(0)).unwrap(), 1);
        drop(validation);

        let service = MetadataService::open(&database).unwrap();
        let response = service.enrich_rich(MetadataEnrichmentRequest { isbns: vec!["9780306406157".to_owned()] }).unwrap();
        assert_eq!(response.results[0].matches[0].popularity.as_ref().map(|value| value.rating_sum), Some(5));
        assert!(response.places.is_empty());
        assert!(response.people.is_empty());
    }

    #[test]
    fn popularity_can_be_added_without_rescanning_open_library() {
        let temporary = tempfile::tempdir().unwrap();
        let source = snapshot_from(temporary.path(), &[serde_json::json!({"key": "/books/OL1M", "works": [{"key": "/works/OL10W"}], "isbn_13": ["9780306406157"]})], &[serde_json::json!({"key": "/works/OL10W", "title": "Example"})], &[]);
        let source_connection = Connection::open(&source).unwrap();
        source_connection.execute("UPDATE snapshot SET schema_version=7 WHERE singleton=1", ()).unwrap();
        source_connection.execute("DROP TABLE work_popularity", ()).unwrap();
        drop(source_connection);
        let ratings = temporary.path().join("ratings.txt.gz");
        let reading_log = temporary.path().join("reading-log.txt.gz");
        let output = temporary.path().join("with-popularity.sqlite");
        tsv_dump(&ratings, &["/works/OL10W\t/books/OL1M\t4\t2026-01-01"]);
        tsv_dump(&reading_log, &["/works/OL10W\t/books/OL1M\tAlready Read\t2026-01-01"]);

        add_popularity_to_snapshot(&source, &ratings, &reading_log, &output, None).unwrap();

        let source_connection = Connection::open(&source).unwrap();
        assert_eq!(source_connection.query_row("SELECT schema_version FROM snapshot", (), |row| row.get::<_, i64>(0)).unwrap(), 7);
        let output_connection = Connection::open(&output).unwrap();
        assert_eq!(output_connection.query_row("SELECT schema_version FROM snapshot", (), |row| row.get::<_, i64>(0)).unwrap(), SCHEMA_VERSION);
        assert_eq!(output_connection.query_row("SELECT already_read_count FROM work_popularity WHERE work_id=10", (), |row| row.get::<_, i64>(0)).unwrap(), 1);
    }

    #[test]
    fn snapshot_enrichment_resolves_work_authors_and_external_ids() {
        let temporary = tempfile::tempdir().unwrap();
        let editions = temporary.path().join("ol_dump_editions_2026-08-01.txt.gz");
        let works = temporary.path().join("ol_dump_works_2026-08-01.txt.gz");
        let authors = temporary.path().join("ol_dump_authors_2026-08-01.txt.gz");
        let wikidata = temporary.path().join("wikidata-2026-08-01.json");
        let ratings = temporary.path().join("ol_dump_ratings_2026-08-01.txt.gz");
        let reading_log = temporary.path().join("ol_dump_reading-log_2026-08-01.txt.gz");
        let database = temporary.path().join("snapshot.sqlite");
        dump(
            &editions,
            &[
                serde_json::json!({
                    "key": "/books/OL1M", "works": [{"key": "/works/OL10W"}], "authors": [{"key": "/authors/OL100A"}],
                    "title": "The Example Book", "publishers": ["North Wind Press"], "publish_date": "2004",
                    "isbn_13": ["9780306406157"], "dewey_decimal_class": ["005.1"],
                    "subject_places": ["Paris"], "subject_people": ["Ada Lovelace (1815-1852)"]
                }),
                serde_json::json!({
                    "key": "/books/OL2M", "works": [{"key": "/works/OL10W"}],
                    "dewey_decimal_class": ["999"], "lc_classifications": ["QA76.73.R87"]
                }),
                serde_json::json!({
                    "key": "/books/OL3M", "works": [{"key": "/works/OL10W"}], "authors": [{"key": "/authors/OL100A"}],
                    "title": "The Example Book", "publishers": ["North Wind Press"], "publish_date": "2005", "isbn_13": ["9780821338278"]
                }),
            ],
        );
        dump(
            &works,
            &[serde_json::json!({
                "key": "/works/OL10W", "title": "Canonical Example Work", "authors": [{"author": {"key": "/authors/OL100A"}}], "lc_classifications": ["QA76.6"],
                "subject_places": ["France", "Paris"], "subject_people": ["Charles Babbage"]
            })],
        );
        dump(
            &authors,
            &[serde_json::json!({
                "key": "/authors/OL100A", "name": "Alex Smith", "remote_ids": {"viaf": "12345", "wikidata": "Q42"}
            })],
        );
        wikidata_dump(
            &wikidata,
            &[
                serde_json::json!({"id":"Q90","labels":{"en":{"value":"Paris"}},"aliases":{},"claims":{"P31":[entity_claim("Q515")],"P131":[entity_claim("Q161741")],"P17":[entity_claim("Q142")],"P625":[{"mainsnak":{"datavalue":{"value":{"latitude":48.8566,"longitude":2.3522}}}}]}}),
                serde_json::json!({"id":"Q161741","labels":{"en":{"value":"Île-de-France"}},"aliases":{},"claims":{"P131":[entity_claim("Q142")]}}),
                serde_json::json!({"id":"Q142","labels":{"en":{"value":"France"}},"aliases":{},"claims":{"P31":[entity_claim("Q6256")],"P30":[entity_claim("Q46")]}}),
                serde_json::json!({"id":"Q46","labels":{"en":{"value":"Europe"}},"aliases":{},"claims":{"P31":[entity_claim("Q5107")]}}),
                serde_json::json!({"id":"Q7259","labels":{"en":{"value":"Ada Lovelace"}},"aliases":{},"claims":{"P31":[entity_claim("Q5")]}}),
                serde_json::json!({"id":"Q1510","labels":{"en":{"value":"Charles Babbage"}},"aliases":{},"claims":{"P31":[entity_claim("Q5")]}}),
                serde_json::json!({"id":"Q999999999","labels":{"en":{"value":"Unreferenced City"}},"aliases":{"en":[{"value":"Unused place"}]},"claims":{"P31":[entity_claim("Q515")],"P17":[entity_claim("Q142")]}}),
            ],
        );
        tsv_dump(&ratings, &["/works/OL10W\t/books/OL1M\t5\t2026-01-01", "/works/OL10W\t\\N\t3\t2026-01-02", "/works/OL999W\t\\N\t5\t2026-01-03"]);
        tsv_dump(
            &reading_log,
            &[
                "/works/OL10W\t/books/OL1M\tWant to Read\t2026-01-01",
                "/works/OL10W\t\\N\tWant to Read\t2026-01-02",
                "/works/OL10W\t\\N\tCurrently Reading\t2026-01-03",
                "/works/OL10W\t\\N\tAlready Read\t2026-01-04",
                "/works/OL999W\t\\N\tAlready Read\t2026-01-05",
            ],
        );

        import_rich_snapshot_with_popularity(&editions, &works, &authors, &wikidata, &ratings, &reading_log, &database, None).unwrap();
        let validation = Connection::open(&database).unwrap();
        assert_eq!(validation.query_row("SELECT schema_version FROM snapshot WHERE singleton=1", (), |row| row.get::<_, i64>(0)).unwrap(), WIKIDATA_SCHEMA_VERSION);
        assert_eq!(
            validation.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name LIKE '%_stage'", (), |row| row.get::<_, i64>(0)).unwrap(),
            0,
            "all append-only import staging tables must be removed from the immutable snapshot"
        );
        assert_eq!(
            validation
                .query_row("SELECT COUNT(*) FROM import_checkpoint WHERE phase IN ('editions_materialize','works_materialize','work_titles_materialize','authors_materialize','wikidata_materialize') AND completed=1", (), |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            5
        );
        let service = MetadataService::open(&database).unwrap();
        let operations = load_operational_stats(&service.state).unwrap();
        assert_eq!(operations.status, "available");
        assert_eq!(operations.schema_version, WIKIDATA_SCHEMA_VERSION);
        assert!(operations.database_bytes > 0);
        assert!(operations.edition_records > 0);
        assert!(operations.work_records > 0);
        assert!(operations.author_records > 0);
        assert!(!operations.metrics.is_empty());
        let response = service.enrich(MetadataEnrichmentRequest { isbns: vec!["978-0-306-40615-7".to_owned()] }).unwrap();
        assert_eq!(response.snapshot.dump_date, "2026-08-01");
        assert_eq!(response.results[0].status, LookupStatus::Matched);
        let matched = &response.results[0].matches[0];
        assert_eq!(matched.open_library_work_id.as_deref(), Some("OL10W"));
        assert_eq!(matched.authors.len(), 1);
        assert_eq!(matched.authors[0].open_library_author_id, "OL100A");
        assert_eq!(matched.authors[0].name.as_deref(), Some("Alex Smith"));
        assert_eq!(matched.authors[0].source, AuthorSource::ExactEdition);
        assert_eq!(matched.authors[0].position, 0);
        assert_eq!(matched.authors[0].identifiers, vec![AuthorIdentifier { authority: "viaf".to_owned(), value: "12345".to_owned() }, AuthorIdentifier { authority: "wikidata".to_owned(), value: "Q42".to_owned() }]);
        assert!(matched.classifications.iter().any(|classification| classification.notation == "005.1" && classification.source == ClassificationSource::ExactEdition));
        assert!(matched.classifications.iter().any(|classification| classification.notation == "QA76.6" && classification.source == ClassificationSource::Work));
        assert!(matched.classifications.iter().any(|classification| classification.notation == "QA76.73.R87" && classification.source == ClassificationSource::Work));
        assert!(matched.classifications.iter().any(|classification| classification.notation == "999" && classification.source == ClassificationSource::Work));

        let rich = service.enrich_rich(MetadataEnrichmentRequest { isbns: vec!["9780306406157".to_owned()] }).unwrap();
        let matched = &rich.results[0].matches[0];
        assert_eq!(matched.places.iter().map(|place| (&place.name, place.source)).collect::<Vec<_>>(), vec![(&"France".to_owned(), EntitySource::Work), (&"Paris".to_owned(), EntitySource::ExactEdition)]);
        assert_eq!(matched.people.len(), 2);
        assert_eq!(matched.places.iter().find(|place| place.name == "Paris").and_then(|place| place.wikidata_id.as_deref()), Some("Q90"));
        assert_eq!(matched.people.iter().find(|person| person.name.starts_with("Ada Lovelace")).and_then(|person| person.wikidata_id.as_deref()), Some("Q7259"));
        assert!(rich.places.iter().any(|place| place.wikidata_id == "Q90" && place.country_ids == ["Q142"]));
        assert!(rich.places.iter().any(|place| place.wikidata_id == "Q46"));
        assert!(rich.people.iter().any(|person| person.wikidata_id == "Q7259"));
        assert_eq!(matched.popularity, Some(metadata_contract::WorkPopularity { want_to_read_count: 2, currently_reading_count: 1, already_read_count: 1, rating_count: 2, rating_sum: 8 }));
        let identity = service
            .resolve_editions(EditionIdentityRequest {
                queries: vec![
                    EditionIdentityQuery { query_id: "local-1".to_owned(), title: "The Example Book".to_owned(), authors: vec!["Smith, Alex".to_owned()], publishers: vec!["North-Wind Press".to_owned()], publication_year: Some(2004) },
                    EditionIdentityQuery {
                        query_id: "work-title".to_owned(),
                        title: "Canonical Example Work".to_owned(),
                        authors: vec!["Alex Smith".to_owned()],
                        publishers: vec!["North Wind Press".to_owned()],
                        publication_year: Some(2004),
                    },
                    EditionIdentityQuery { query_id: "ambiguous".to_owned(), title: "The Example Book".to_owned(), authors: vec!["Alex Smith".to_owned()], publishers: vec!["North Wind Press".to_owned()], publication_year: None },
                    EditionIdentityQuery { query_id: "insufficient".to_owned(), title: "The Example Book".to_owned(), authors: vec!["Alex Smith".to_owned()], publishers: Vec::new(), publication_year: None },
                ],
            })
            .unwrap();
        assert_eq!(identity.results[0].status, EditionIdentityStatus::Matched);
        let identity_match = identity.results[0].matched.as_ref().unwrap();
        assert_eq!(identity_match.canonical_isbn13, "9780306406157");
        assert_eq!(identity_match.open_library_edition_id, "OL1M");
        assert_eq!(identity_match.title_match, EditionTitleMatch::Edition);
        assert!(identity_match.matched_publisher && identity_match.matched_publication_year);
        assert_eq!(identity.results[1].status, EditionIdentityStatus::Matched);
        assert_eq!(identity.results[1].matched.as_ref().unwrap().title_match, EditionTitleMatch::Work);
        assert_eq!(identity.results[2].status, EditionIdentityStatus::Ambiguous);
        assert_eq!(identity.results[2].candidates.len(), 2);
        assert_eq!(identity.results[3].status, EditionIdentityStatus::Ambiguous);
        let retained = Connection::open(&database).unwrap().query_row("SELECT COUNT(*) FROM entity WHERE wikidata_id=999999999", (), |row| row.get::<_, i64>(0)).unwrap();
        assert_eq!(retained, 0, "unreferenced Wikidata geography must be pruned from the snapshot");
    }

    #[test]
    fn wikidata_scan_resumes_from_committed_record_checkpoint() {
        let temporary = tempfile::tempdir().unwrap();
        let wikidata = temporary.path().join("wikidata.json");
        wikidata_dump(
            &wikidata,
            &[
                serde_json::json!({"id":"Q90","labels":{"en":{"value":"Paris"}},"aliases":{},"claims":{"P31":[entity_claim("Q515")],"P17":[entity_claim("Q142")]}}),
                serde_json::json!({"id":"Q142","labels":{"en":{"value":"France"}},"aliases":{},"claims":{"P31":[entity_claim("Q6256")],"P30":[entity_claim("Q46")]}}),
                serde_json::json!({"id":"Q46","labels":{"en":{"value":"Europe"}},"aliases":{},"claims":{"P31":[entity_claim("Q5107")]}}),
            ],
        );
        let database = temporary.path().join("resumable.sqlite");
        let mut connection = Connection::open(&database).unwrap();
        configure_import(&connection).unwrap();
        create_schema(&connection).unwrap();
        connection
            .execute_batch(
                "INSERT INTO work_entity_evidence(work_id,kind,name,normalized_name) VALUES(1,1,'Paris','paris');
                 INSERT INTO entity_stage(wikidata_id,kind,label,fictional) VALUES(90,1,'Paris',0);
                 INSERT INTO place_country_stage(place_id,country_id) VALUES(90,142);
                 INSERT INTO wikidata_candidate_stage(kind,normalized_name,wikidata_id) VALUES(1,'paris',90);
                 INSERT INTO import_checkpoint(phase,records_read,completed) VALUES('wikidata_scan',1,0);",
            )
            .unwrap();

        assert_eq!(import_wikidata(&mut connection, &wikidata, None).unwrap(), 3);
        assert_eq!(completed_phase(&connection, "wikidata_scan").unwrap(), Some(3));
        assert_eq!(completed_phase(&connection, "wikidata_finalize").unwrap(), Some(3));
        assert_eq!(completed_phase(&connection, "wikidata_materialize").unwrap(), Some(0));
        let retained = connection.query_row("SELECT group_concat(wikidata_id,',') FROM (SELECT wikidata_id FROM entity ORDER BY wikidata_id)", (), |row| row.get::<_, String>(0)).unwrap();
        assert_eq!(retained, "46,90,142");
        assert_eq!(import_wikidata(&mut connection, &wikidata, None).unwrap(), 3, "a completed materialized phase is idempotent");
    }

    #[test]
    fn open_library_scan_resumes_from_committed_record_checkpoint() {
        let temporary = tempfile::tempdir().unwrap();
        let editions = temporary.path().join("editions.txt.gz");
        dump(&editions, &[serde_json::json!({"key":"/books/OL1M","isbn_13":["9780306406157"]}), serde_json::json!({"key":"/books/OL2M","isbn_13":["9780821338278"]})]);
        let database = temporary.path().join("resumable-open-library.sqlite");
        let mut connection = Connection::open(&database).unwrap();
        configure_import(&connection).unwrap();
        create_schema(&connection).unwrap();
        connection
            .execute_batch(
                "INSERT INTO edition_stage(edition_id) VALUES(1);
                 INSERT INTO edition_isbn_stage(isbn13,edition_id) VALUES(9780306406157,1);
                 INSERT INTO import_checkpoint(phase,records_read,completed) VALUES('editions',1,0);",
            )
            .unwrap();

        assert_eq!(import_editions(&mut connection, &editions, None).unwrap(), 2);
        assert_eq!(completed_phase(&connection, "editions").unwrap(), Some(2));
        assert_eq!(completed_phase(&connection, "editions_materialize").unwrap(), Some(0));
        let retained = connection.query_row("SELECT group_concat(edition_id,',') FROM (SELECT edition_id FROM edition ORDER BY edition_id)", (), |row| row.get::<_, String>(0)).unwrap();
        assert_eq!(retained, "1,2");
        assert_eq!(import_editions(&mut connection, &editions, None).unwrap(), 2, "a completed materialized phase is idempotent");
    }

    /// An edition with no `/works` link leaves `work_bibliography` unmatched by
    /// the candidate LEFT JOIN, so its title-comparison column is NULL rather
    /// than false. Reading it as a plain `bool` fails the entire batch request.
    #[test]
    fn edition_identity_resolves_an_edition_that_has_no_linked_work() {
        let temporary = tempfile::tempdir().unwrap();
        let database = snapshot_from(
            temporary.path(),
            &[serde_json::json!({
                "key": "/books/OL1M", "authors": [{"key": "/authors/OL100A"}],
                "title": "Standalone Edition", "publishers": ["North Wind Press"], "publish_date": "2004",
                "isbn_13": ["9780306406157"]
            })],
            &[],
            &[serde_json::json!({"key": "/authors/OL100A", "name": "Alex Smith"})],
        );
        let service = MetadataService::open(&database).unwrap();

        let response = service.resolve_editions(edition_query("Standalone Edition", vec!["North Wind Press".to_owned()], Some(2004))).expect("an edition without a work must not fail the request");
        assert_eq!(response.results[0].status, EditionIdentityStatus::Matched);
        let matched = response.results[0].matched.as_ref().unwrap();
        assert_eq!(matched.open_library_edition_id, "OL1M");
        assert_eq!(matched.open_library_work_id, None);
        assert_eq!(matched.title_match, EditionTitleMatch::Edition);
    }

    /// An untitled sibling edition of the same work has no `edition_bibliography`
    /// row, so its title-comparison column is NULL. One such row in the
    /// candidate set must not destroy the lookup for the edition that does match.
    #[test]
    fn edition_identity_survives_an_untitled_sibling_edition_of_the_same_work() {
        let temporary = tempfile::tempdir().unwrap();
        let database = snapshot_from(
            temporary.path(),
            &[
                serde_json::json!({
                    "key": "/books/OL1M", "works": [{"key": "/works/OL10W"}], "authors": [{"key": "/authors/OL100A"}],
                    "title": "The Example Book", "publishers": ["North Wind Press"], "publish_date": "2004",
                    "isbn_13": ["9780306406157"]
                }),
                serde_json::json!({
                    "key": "/books/OL2M", "works": [{"key": "/works/OL10W"}], "authors": [{"key": "/authors/OL100A"}],
                    "isbn_13": ["9780821338278"]
                }),
            ],
            &[serde_json::json!({"key": "/works/OL10W", "title": "Canonical Example Work", "authors": [{"author": {"key": "/authors/OL100A"}}]})],
            &[serde_json::json!({"key": "/authors/OL100A", "name": "Alex Smith"})],
        );
        let service = MetadataService::open(&database).unwrap();

        let response = service.resolve_editions(edition_query("Canonical Example Work", vec!["North Wind Press".to_owned()], Some(2004))).expect("an untitled candidate must not fail the request");
        assert_eq!(response.results[0].status, EditionIdentityStatus::Matched);
        let matched = response.results[0].matched.as_ref().unwrap();
        assert_eq!(matched.open_library_edition_id, "OL1M");
        assert_eq!(matched.title_match, EditionTitleMatch::Work);
    }

    /// Matches are emitted per ISBN but the match/ambiguity decision counts
    /// them, so a single edition that carries more than one ISBN is reported
    /// ambiguous and callers that require `Matched` silently give up on it.
    #[test]
    fn edition_identity_matches_a_single_edition_that_carries_several_isbns() {
        let temporary = tempfile::tempdir().unwrap();
        let database = snapshot_from(
            temporary.path(),
            &[serde_json::json!({
                "key": "/books/OL1M", "works": [{"key": "/works/OL10W"}], "authors": [{"key": "/authors/OL100A"}],
                "title": "The Example Book", "publishers": ["North Wind Press"], "publish_date": "2004",
                "isbn_13": ["9780306406157"], "isbn_10": ["0821338277"]
            })],
            &[serde_json::json!({"key": "/works/OL10W", "title": "Canonical Example Work", "authors": [{"author": {"key": "/authors/OL100A"}}]})],
            &[serde_json::json!({"key": "/authors/OL100A", "name": "Alex Smith"})],
        );
        let service = MetadataService::open(&database).unwrap();

        let response = service.resolve_editions(edition_query("The Example Book", vec!["North Wind Press".to_owned()], Some(2004))).unwrap();
        assert_eq!(response.results[0].status, EditionIdentityStatus::Matched, "two ISBNs on one edition are still one edition");
        let matched = response.results[0].matched.as_ref().unwrap();
        assert_eq!(matched.open_library_edition_id, "OL1M");
        assert!(matched.matched_publisher && matched.matched_publication_year);
    }

    #[test]
    fn edition_identity_accepts_a_local_subtitle_and_newer_publication_details() {
        let temporary = tempfile::tempdir().unwrap();
        let database = snapshot_from(
            temporary.path(),
            &[serde_json::json!({
                "key": "/books/OL1M", "works": [{"key": "/works/OL10W"}], "authors": [{"key": "/authors/OL100A"}],
                "title": "The Example Book", "publishers": ["Old Press"], "publish_date": "2004", "isbn_13": ["9780306406157"]
            })],
            &[serde_json::json!({"key": "/works/OL10W", "title": "The Example Book", "authors": [{"author": {"key": "/authors/OL100A"}}]})],
            &[serde_json::json!({"key": "/authors/OL100A", "name": "Alex Smith"})],
        );
        let service = MetadataService::open(&database).unwrap();

        let response = service.resolve_editions(edition_query("Cambridge concise histories: The Example Book (Routledge Classics)", vec!["New Press".to_owned()], Some(2026))).unwrap();
        assert_eq!(response.results[0].status, EditionIdentityStatus::Matched);
        let matched = response.results[0].matched.as_ref().unwrap();
        assert_eq!(matched.open_library_edition_id, "OL1M");
        assert!(!matched.matched_publisher);
        assert!(!matched.matched_publication_year);
    }

    #[test]
    fn edition_identity_prefers_the_longest_title_match_before_the_review_limit() {
        let temporary = tempfile::tempdir().unwrap();
        let mut editions = (1..=300)
            .map(|edition_id| {
                serde_json::json!({
                    "key": format!("/books/OL{edition_id}M"),
                    "authors": [{"key": "/authors/OL100A"}],
                    "title": "Physical Chemistry",
                    "isbn_13": ["9780306406157"]
                })
            })
            .collect::<Vec<_>>();
        editions.push(serde_json::json!({
            "key": "/books/OL1000M",
            "authors": [{"key": "/authors/OL100A"}],
            "title": "Physical Chemistry: A Very Short Introduction",
            "isbn_13": ["9780821338278"]
        }));
        let database = snapshot_from(temporary.path(), &editions, &[], &[serde_json::json!({"key": "/authors/OL100A", "name": "Alex Smith"})]);
        let service = MetadataService::open(&database).unwrap();

        let response = service.resolve_editions(edition_query("Physical Chemistry: A Very Short Introduction", Vec::new(), None)).unwrap();
        assert_eq!(response.results[0].status, EditionIdentityStatus::Matched);
        assert!(!response.results[0].candidates_truncated);
        assert_eq!(response.results[0].matched.as_ref().unwrap().open_library_edition_id, "OL1000M");
    }

    #[test]
    fn edition_identity_uses_author_only_to_break_a_longest_substring_tie() {
        let temporary = tempfile::tempdir().unwrap();
        let database = snapshot_from(
            temporary.path(),
            &[
                serde_json::json!({
                    "key": "/books/OL1M", "authors": [{"key": "/authors/OL100A"}],
                    "title": "A Concise History of Spain", "publishers": ["First Press"], "isbn_13": ["9780306406157"]
                }),
                serde_json::json!({
                    "key": "/books/OL2M", "authors": [{"key": "/authors/OL200A"}],
                    "title": "A Concise History of Spain", "publishers": ["Preferred Press"], "isbn_13": ["9780821338278"]
                }),
            ],
            &[],
            &[serde_json::json!({"key": "/authors/OL100A", "name": "Alex Smith"}), serde_json::json!({"key": "/authors/OL200A", "name": "Jamie Jones"})],
        );
        let service = MetadataService::open(&database).unwrap();
        let title = "Cambridge concise histories: A Concise History of Spain (New edition)".to_owned();
        let response = service
            .resolve_editions(EditionIdentityRequest {
                queries: vec![
                    EditionIdentityQuery { query_id: "title-only".to_owned(), title: title.clone(), authors: Vec::new(), publishers: vec!["Preferred Press".to_owned()], publication_year: None },
                    EditionIdentityQuery { query_id: "with-author".to_owned(), title, authors: vec!["Alex Smith".to_owned()], publishers: vec!["Preferred Press".to_owned()], publication_year: None },
                ],
            })
            .unwrap();

        assert_eq!(response.results[0].status, EditionIdentityStatus::Ambiguous);
        assert_eq!(response.results[0].candidates.len(), 2);
        assert_eq!(response.results[1].status, EditionIdentityStatus::Matched);
        assert_eq!(response.results[1].matched.as_ref().unwrap().open_library_edition_id, "OL1M");
    }

    #[test]
    fn edition_identity_requires_an_author_match_for_automatic_acceptance() {
        let temporary = tempfile::tempdir().unwrap();
        let database = snapshot_from(
            temporary.path(),
            &[serde_json::json!({
                "key": "/books/OL1M", "authors": [{"key": "/authors/OL200A"}],
                "title": "Complete Works Of", "isbn_13": ["9780306406157"]
            })],
            &[],
            &[serde_json::json!({"key": "/authors/OL200A", "name": "Jamie Jones"})],
        );
        let service = MetadataService::open(&database).unwrap();

        let response = service
            .resolve_editions(EditionIdentityRequest {
                queries: vec![EditionIdentityQuery { query_id: "wrong-author".to_owned(), title: "Delphi Complete Works Of Athenaeus".to_owned(), authors: vec!["Athenaeus".to_owned()], publishers: Vec::new(), publication_year: None }],
            })
            .unwrap();

        assert_eq!(response.results[0].status, EditionIdentityStatus::Ambiguous);
        assert!(response.results[0].matched.is_none());
        assert_eq!(response.results[0].candidates.len(), 1);
        assert_eq!(response.results[0].candidates[0].open_library_edition_id, "OL1M");
    }

    #[test]
    fn edition_identity_accepts_one_work_after_author_matched_editions_are_collapsed() {
        let temporary = tempfile::tempdir().unwrap();
        let database = snapshot_from(
            temporary.path(),
            &[
                serde_json::json!({
                    "key": "/books/OL1M", "works": [{"key": "/works/OL10W"}], "authors": [{"key": "/authors/OL100A"}],
                    "title": "Complete Works of Homer", "publishers": ["First Press"], "isbn_13": ["9780306406157"]
                }),
                serde_json::json!({
                    "key": "/books/OL2M", "works": [{"key": "/works/OL10W"}], "authors": [{"key": "/authors/OL100A"}],
                    "title": "Complete Works of Homer", "publishers": ["Second Press"], "isbn_13": ["9780821338278"]
                }),
            ],
            &[serde_json::json!({"key": "/works/OL10W", "title": "Complete Works of Homer", "authors": [{"author": {"key": "/authors/OL100A"}}]})],
            &[serde_json::json!({"key": "/authors/OL100A", "name": "Homer"})],
        );
        let service = MetadataService::open(&database).unwrap();

        let response = service
            .resolve_editions(EditionIdentityRequest {
                queries: vec![EditionIdentityQuery {
                    query_id: "same-work".to_owned(),
                    title: "Delphi Complete Works of Homer".to_owned(),
                    authors: vec!["Homer".to_owned()],
                    publishers: vec!["Delphi Classics".to_owned()],
                    publication_year: None,
                }],
            })
            .unwrap();

        assert_eq!(response.results[0].status, EditionIdentityStatus::Matched);
        assert_eq!(response.results[0].matched.as_ref().unwrap().open_library_work_id.as_deref(), Some("OL10W"));
        assert!(!response.results[0].matched.as_ref().unwrap().matched_publisher);
    }

    #[test]
    fn edition_identity_combines_inferred_author_and_year_to_remove_other_candidates() {
        let temporary = tempfile::tempdir().unwrap();
        let database = snapshot_from(
            temporary.path(),
            &[
                serde_json::json!({
                    "key": "/books/OL1M", "authors": [{"key": "/authors/OL100A"}],
                    "title": "Algorithms", "publish_date": "1999", "isbn_13": ["9780306406157"]
                }),
                serde_json::json!({
                    "key": "/books/OL2M", "authors": [{"key": "/authors/OL100A"}],
                    "title": "Algorithms", "publish_date": "2019", "isbn_13": ["9780821338278"]
                }),
                serde_json::json!({
                    "key": "/books/OL3M", "authors": [{"key": "/authors/OL200A"}],
                    "title": "Algorithms", "publish_date": "2019", "isbn_13": ["9781501168697"]
                }),
            ],
            &[],
            &[serde_json::json!({"key": "/authors/OL100A", "name": "Jeff Erickson"}), serde_json::json!({"key": "/authors/OL200A", "name": "Fethi Rabhi"})],
        );
        let service = MetadataService::open(&database).unwrap();
        let request = EditionIdentityRequest {
            queries: vec![EditionIdentityQuery { query_id: "noisy-filename".to_owned(), title: "Algorithms".to_owned(), authors: vec!["Jeff Erickson".to_owned()], publishers: Vec::new(), publication_year: Some(2019) }],
        };

        let response = service.resolve_editions(request).unwrap();
        assert_eq!(response.results[0].status, EditionIdentityStatus::Matched);
        assert_eq!(response.results[0].matched.as_ref().unwrap().open_library_edition_id, "OL2M");
    }

    #[test]
    fn edition_identity_batch_keeps_results_attached_to_each_query() {
        let temporary = tempfile::tempdir().unwrap();
        let database = snapshot_from(
            temporary.path(),
            &[
                serde_json::json!({
                    "key": "/books/OL1M", "authors": [{"key": "/authors/OL100A"}],
                    "title": "First Book", "isbn_13": ["9780306406157"]
                }),
                serde_json::json!({
                    "key": "/books/OL2M", "authors": [{"key": "/authors/OL100A"}],
                    "title": "Second Book", "isbn_13": ["9780821338278"]
                }),
            ],
            &[],
            &[serde_json::json!({"key": "/authors/OL100A", "name": "Alex Smith"})],
        );
        let service = MetadataService::open(&database).unwrap();
        let request = EditionIdentityRequest {
            queries: vec![
                EditionIdentityQuery { query_id: "second".to_owned(), title: "Second Book".to_owned(), authors: vec!["Alex Smith".to_owned()], publishers: Vec::new(), publication_year: None },
                EditionIdentityQuery { query_id: "missing".to_owned(), title: "Missing Book".to_owned(), authors: vec!["Alex Smith".to_owned()], publishers: Vec::new(), publication_year: None },
                EditionIdentityQuery { query_id: "first".to_owned(), title: "First Book".to_owned(), authors: vec!["Alex Smith".to_owned()], publishers: Vec::new(), publication_year: None },
            ],
        };

        let response = service.resolve_editions(request).unwrap();
        assert_eq!(response.results.iter().map(|result| result.query_id.as_str()).collect::<Vec<_>>(), vec!["second", "missing", "first"]);
        assert_eq!(response.results[0].matched.as_ref().unwrap().open_library_edition_id, "OL2M");
        assert_eq!(response.results[1].status, EditionIdentityStatus::NoMatch);
        assert_eq!(response.results[2].matched.as_ref().unwrap().open_library_edition_id, "OL1M");
    }

    #[test]
    fn edition_identity_uses_publisher_only_to_break_a_title_author_tie() {
        let temporary = tempfile::tempdir().unwrap();
        let database = snapshot_from(
            temporary.path(),
            &[
                serde_json::json!({
                    "key": "/books/OL1M", "works": [{"key": "/works/OL10W"}], "authors": [{"key": "/authors/OL100A"}],
                    "title": "The Example Book", "publishers": ["Old Press"], "publish_date": "2004", "isbn_13": ["9780306406157"]
                }),
                serde_json::json!({
                    "key": "/books/OL2M", "works": [{"key": "/works/OL10W"}], "authors": [{"key": "/authors/OL100A"}],
                    "title": "The Example Book", "publishers": ["Preferred Press"], "publish_date": "2010", "isbn_13": ["9780821338278"]
                }),
            ],
            &[serde_json::json!({"key": "/works/OL10W", "title": "The Example Book", "authors": [{"author": {"key": "/authors/OL100A"}}]})],
            &[serde_json::json!({"key": "/authors/OL100A", "name": "Alex Smith"})],
        );
        let service = MetadataService::open(&database).unwrap();

        let response = service.resolve_editions(edition_query("The Example Book", vec!["Preferred Press".to_owned()], None)).unwrap();
        assert_eq!(response.results[0].status, EditionIdentityStatus::Matched);
        assert_eq!(response.results[0].matched.as_ref().unwrap().open_library_edition_id, "OL2M");
    }

    #[test]
    fn edition_identity_combines_classification_sources_and_suggests_only_one_edition_per_work() {
        let temporary = tempfile::tempdir().unwrap();
        let database = snapshot_from(
            temporary.path(),
            &[
                serde_json::json!({
                    "key": "/books/OL1M", "works": [{"key": "/works/OL10W"}],
                    "title": "The Example Book", "isbn_13": ["9780306406157"],
                    "dewey_decimal_class": ["005.1"]
                }),
                serde_json::json!({
                    "key": "/books/OL2M", "works": [{"key": "/works/OL10W"}],
                    "title": "The Example Book", "isbn_13": ["9780821338278"],
                    "lc_classifications": ["QA76.73.R87"]
                }),
            ],
            &[serde_json::json!({"key": "/works/OL10W", "title": "The Example Book", "lc_classifications": ["QA76.6"]})],
            &[],
        );
        let service = MetadataService::open(&database).unwrap();

        let response = service.resolve_editions(edition_query("The Example Book", Vec::new(), None)).unwrap();
        assert_eq!(response.results[0].status, EditionIdentityStatus::Ambiguous);
        assert_eq!(response.results[0].candidates.len(), 1);
        assert_eq!(response.results[0].candidates[0].open_library_work_id.as_deref(), Some("OL10W"));
        assert!(response.results[0].candidates[0].classifications.iter().any(|classification| classification.notation == "005.1" && classification.source == ClassificationSource::ExactEdition));
        assert!(response.results[0].candidates[0].classifications.iter().any(|classification| classification.notation == "QA76.6" && classification.source == ClassificationSource::Work));
        assert!(response.results[0].candidates[0].classifications.iter().any(|classification| classification.notation == "QA76.73.R87" && classification.source == ClassificationSource::Work));
    }

    #[test]
    fn classification_lookup_requires_consensus_when_sibling_evidence_is_sparse() {
        let temporary = tempfile::tempdir().unwrap();
        let database = snapshot_from(
            temporary.path(),
            &[
                serde_json::json!({
                    "key": "/books/OL1M", "works": [{"key": "/works/OL10W"}],
                    "title": "The Example Book", "isbn_13": ["9780306406157"],
                    "dewey_decimal_class": ["005.1"]
                }),
                serde_json::json!({
                    "key": "/books/OL2M", "works": [{"key": "/works/OL10W"}],
                    "title": "The Example Book", "isbn_13": ["9780821338278"],
                    "lc_classifications": ["QA76.73.R87"]
                }),
            ],
            &[serde_json::json!({"key": "/works/OL10W", "title": "The Example Book"})],
            &[],
        );
        let service = MetadataService::open(&database).unwrap();

        let response = service.lookup(ClassificationRequest { isbns: vec!["9780306406157".to_owned()] }).unwrap();
        assert_eq!(response.results[0].matches[0].classifications, vec![Classification { scheme: ClassificationScheme::DeweyDecimal, notation: "005.1".to_owned(), source: ClassificationSource::ExactEdition }]);
    }

    #[test]
    fn classification_lookup_uses_all_supported_codes_for_a_large_work_union() {
        let temporary = tempfile::tempdir().unwrap();
        let editions = [
            serde_json::json!({"key":"/books/OL1M","works":[{"key":"/works/OL10W"}],"title":"Example","isbn_13":["9780306406157"]}),
            serde_json::json!({"key":"/books/OL2M","works":[{"key":"/works/OL10W"}],"title":"Example","isbn_13":["9780821338278"],"lc_classifications":["QA76.6.A1"],"dewey_decimal_class":["005.1"]}),
            serde_json::json!({"key":"/books/OL3M","works":[{"key":"/works/OL10W"}],"title":"Example","isbn_13":["9783161484100"],"lc_classifications":["QA76.6.B2"],"dewey_decimal_class":["005.1"]}),
            serde_json::json!({"key":"/books/OL4M","works":[{"key":"/works/OL10W"}],"title":"Example","isbn_13":["9781861972712"],"lc_classifications":["PN1993.A1"],"dewey_decimal_class":["791.43"]}),
            serde_json::json!({"key":"/books/OL5M","works":[{"key":"/works/OL10W"}],"title":"Example","isbn_13":["9780140328721"],"lc_classifications":["PN1993.B2"],"dewey_decimal_class":["791.43"]}),
        ];
        let noisy = (1..=13).map(|number| format!("B{number}")).collect::<Vec<_>>();
        let works = [serde_json::json!({"key":"/works/OL10W","title":"Example","lc_classifications":noisy})];
        let database = snapshot_from(temporary.path(), &editions, &works, &[]);
        let service = MetadataService::open(&database).unwrap();

        let response = service.lookup(ClassificationRequest { isbns: vec!["9780306406157".to_owned()] }).unwrap();
        let classifications = &response.results[0].matches[0].classifications;
        assert_eq!(
            classifications,
            &[
                Classification { scheme: ClassificationScheme::LibraryOfCongress, notation: "PN1993".to_owned(), source: ClassificationSource::Work },
                Classification { scheme: ClassificationScheme::LibraryOfCongress, notation: "QA76.6".to_owned(), source: ClassificationSource::Work },
                Classification { scheme: ClassificationScheme::DeweyDecimal, notation: "005.1".to_owned(), source: ClassificationSource::Work },
                Classification { scheme: ClassificationScheme::DeweyDecimal, notation: "791.43".to_owned(), source: ClassificationSource::Work },
            ]
        );
    }

    #[test]
    fn serving_rejects_a_snapshot_outside_the_supported_schema_set() {
        let temporary = tempfile::tempdir().unwrap();
        let database = snapshot_from(temporary.path(), &[], &[], &[]);
        let connection = Connection::open(&database).unwrap();
        connection.execute("UPDATE snapshot SET schema_version=?1 WHERE singleton=1", [LEGACY_QUERY_SCHEMA_VERSION - 1]).unwrap();
        drop(connection);

        let failure = match MetadataService::open(&database) {
            Ok(_) => panic!("an unsupported snapshot must not be served"),
            Err(failure) => failure,
        };
        assert!(failure.to_string().contains(&format!("schema {}", LEGACY_QUERY_SCHEMA_VERSION - 1)), "unexpected rejection: {failure}");
        assert!(failure.to_string().contains(&format!("expected {LEGACY_QUERY_SCHEMA_VERSION}, {SCHEMA_VERSION}, or {WIKIDATA_SCHEMA_VERSION}")), "unexpected rejection: {failure}");
    }

    /// Only the current builder version is supported, so an unfinished build
    /// from an older builder must be rejected without mutating its checkpoints.
    #[test]
    fn unfinished_build_from_an_older_builder_is_rejected_without_being_mutated() {
        let temporary = tempfile::tempdir().unwrap();
        let editions = temporary.path().join("ol_dump_editions_2026-08-01.txt.gz");
        let works = temporary.path().join("ol_dump_works_2026-08-01.txt.gz");
        let authors = temporary.path().join("ol_dump_authors_2026-08-01.txt.gz");
        dump(&editions, &[serde_json::json!({"key": "/books/OL1M", "works": [{"key": "/works/OL10W"}], "title": "The Example Book", "isbn_13": ["9780306406157"]})]);
        dump(&works, &[serde_json::json!({"key": "/works/OL10W", "title": "Canonical Example Work"})]);
        dump(&authors, &[]);
        let database = temporary.path().join("snapshot.sqlite");
        let building = building_path(&database);

        // Model an unfinished v6 build by removing the later work-title tables.
        let connection = Connection::open(&building).unwrap();
        configure_import(&connection).unwrap();
        create_schema(&connection).unwrap();
        register_import_inputs(&connection, &editions, &works, &authors, None, None, None).unwrap();
        connection
            .execute_batch(
                "DROP INDEX work_bibliography_by_title;
                 DROP TABLE work_bibliography;
                 DROP TABLE work_bibliography_stage;
                 UPDATE metadata_metric SET value=6 WHERE name='builder_schema_version';",
            )
            .unwrap();
        drop(connection);

        let failure = import_snapshot(&editions, &works, &authors, &database, None).expect_err("an unfinished build from an older builder must be rejected");
        assert!(failure.to_string().contains("builder schema"), "unexpected rejection: {failure}");

        let connection = Connection::open(&building).unwrap();
        assert_eq!(connection.query_row("SELECT value FROM metadata_metric WHERE name='builder_schema_version'", (), |row| row.get::<_, i64>(0)).unwrap(), 6, "a rejected resume must not restamp the builder version");
        assert_eq!(connection.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='work_bibliography'", (), |row| row.get::<_, i64>(0)).unwrap(), 0, "a rejected resume must not commit schema changes");
    }
}
