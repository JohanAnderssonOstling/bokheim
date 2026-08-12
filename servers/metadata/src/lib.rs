use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::Router;
use flate2::read::MultiGzDecoder;
use metadata_contract::{
    Classification, ClassificationRequest, ClassificationResponse, ClassificationScheme, ClassificationSource, IsbnClassificationResult, LookupStatus, SnapshotDescription, WorkClassificationMatch, MAX_ISBNS_PER_REQUEST, MAX_REQUEST_BYTES,
    MAX_RESPONSE_BYTES,
};
use rusqlite::{params, Connection, OpenFlags};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

const SCHEMA_VERSION: i64 = 1;
const DDC_SCHEME: i64 = 1;
const LCC_SCHEME: i64 = 2;
const MAX_NOTATION_BYTES: usize = 256;

#[derive(Clone)]
pub struct MetadataService {
    state: Arc<ServiceState>,
}

struct ServiceState {
    database: PathBuf,
    snapshot: SnapshotDescription,
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
    #[serde(default, deserialize_with = "deserialize_key_references")]
    works: Vec<KeyReference>,
    #[serde(default, deserialize_with = "deserialize_string_array")]
    isbn_10: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_string_array")]
    isbn_13: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_string_array")]
    dewey_decimal_class: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_string_array")]
    lc_classifications: Vec<String>,
}

#[derive(Default, Deserialize)]
struct WorkRecord {
    key: String,
    #[serde(default, deserialize_with = "deserialize_string_array")]
    dewey_number: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_string_array")]
    lc_classifications: Vec<String>,
}

#[derive(Deserialize)]
struct KeyReference {
    key: String,
}

impl MetadataService {
    pub fn open(database: impl Into<PathBuf>) -> Result<Self, MetadataError> {
        let database = database.into();
        let connection = open_read_only(&database)?;
        let (schema_version, dump_date, imported_at_ms) =
            connection.query_row("SELECT schema_version,dump_date,imported_at_ms FROM snapshot WHERE singleton=1", (), |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?))).map_err(error)?;
        if schema_version != SCHEMA_VERSION {
            return Err(MetadataError(format!("unsupported metadata database schema {schema_version}")));
        }
        let imported_at_ms = u64::try_from(imported_at_ms).map_err(|_| MetadataError("metadata snapshot has an invalid import timestamp".to_owned()))?;
        Ok(Self { state: Arc::new(ServiceState { database, snapshot: SnapshotDescription { dump_date, imported_at_ms } }) })
    }

    pub fn lookup(&self, request: ClassificationRequest) -> Result<ClassificationResponse, MetadataError> {
        if request.isbns.is_empty() {
            return Err(MetadataError("at least one ISBN is required".to_owned()));
        }
        if request.isbns.len() > MAX_ISBNS_PER_REQUEST {
            return Err(MetadataError(format!("at most {MAX_ISBNS_PER_REQUEST} ISBNs may be requested")));
        }
        let connection = open_read_only(&self.state.database)?;
        let mut results = Vec::with_capacity(request.isbns.len());
        for requested_isbn in request.isbns {
            results.push(lookup_one(&connection, requested_isbn)?);
        }
        Ok(ClassificationResponse { snapshot: self.state.snapshot.clone(), results })
    }
}

pub fn app(service: MetadataService) -> Router {
    Router::new().route("/v1/classifications", post(classifications)).layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES)).with_state(service)
}

async fn classifications(State(service): State<MetadataService>, headers: HeaderMap, body: Bytes) -> Response {
    let binary = headers.get(header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).is_some_and(metadata_contract::is_binary_media_type);
    let request = if binary {
        metadata_contract::decode(&body, MAX_REQUEST_BYTES).map_err(|error| MetadataError(error.to_string()))
    } else if headers.get(header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).is_some_and(|value| value.split(';').next().is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"))) {
        serde_json::from_slice(&body).map_err(|error| MetadataError(format!("invalid JSON request: {error}")))
    } else {
        return api_error(StatusCode::UNSUPPORTED_MEDIA_TYPE, "use application/json or application/vnd.bokheim.metadata+bincode; version=1");
    };
    let request = match request {
        Ok(request) => request,
        Err(error) => return api_error(StatusCode::BAD_REQUEST, &error.to_string()),
    };
    let result = match tokio::task::spawn_blocking(move || service.lookup(request)).await {
        Ok(Ok(response)) => response,
        Ok(Err(error)) => return api_error(StatusCode::BAD_REQUEST, &error.to_string()),
        Err(error) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, &format!("lookup worker failed: {error}")),
    };
    let (content_type, encoded) = if binary {
        match metadata_contract::encode(&result) {
            Ok(encoded) => (metadata_contract::MEDIA_TYPE, encoded),
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

fn api_error(status: StatusCode, message: &str) -> Response {
    (status, [(header::CONTENT_TYPE, "text/plain; charset=utf-8")], message.to_owned()).into_response()
}

pub fn import_snapshot(editions_path: &Path, works_path: &Path, output_path: &Path, record_limit: Option<usize>) -> Result<(), MetadataError> {
    if output_path.exists() {
        return Err(MetadataError(format!("output already exists: {}", output_path.display())));
    }
    let building_path = building_path(output_path);
    if building_path.exists() {
        return Err(MetadataError(format!("unfinished build already exists: {}", building_path.display())));
    }
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).map_err(error)?;
    }
    let mut connection = Connection::open(&building_path).map_err(error)?;
    configure_import(&connection)?;
    create_schema(&connection)?;
    let result = (|| {
        let edition_count = import_editions(&mut connection, editions_path, record_limit)?;
        let work_count = import_works(&mut connection, works_path, record_limit)?;
        let dump_date = dump_date(editions_path).or_else(|| dump_date(works_path)).unwrap_or_else(|| "unknown".to_owned());
        let imported_at_ms = SystemTime::now().duration_since(UNIX_EPOCH).map_err(error)?.as_millis();
        let imported_at_ms = i64::try_from(imported_at_ms).map_err(error)?;
        connection
            .execute("INSERT INTO snapshot(singleton,schema_version,dump_date,imported_at_ms,edition_records,work_records) VALUES(1,?1,?2,?3,?4,?5)", params![SCHEMA_VERSION, dump_date, imported_at_ms, edition_count, work_count])
            .map_err(error)?;
        connection.execute_batch("ANALYZE; PRAGMA optimize;").map_err(error)?;
        let integrity: String = connection.query_row("PRAGMA integrity_check", (), |row| row.get(0)).map_err(error)?;
        if integrity != "ok" {
            return Err(MetadataError(format!("metadata database integrity check failed: {integrity}")));
        }
        Ok(())
    })();
    drop(connection);
    result?;
    fs::rename(&building_path, output_path).map_err(error)?;
    println!("created metadata snapshot {}", output_path.display());
    Ok(())
}

fn configure_import(connection: &Connection) -> Result<(), MetadataError> {
    connection.execute_batch("PRAGMA journal_mode=OFF; PRAGMA synchronous=OFF; PRAGMA temp_store=MEMORY; PRAGMA cache_size=-262144; PRAGMA locking_mode=EXCLUSIVE;").map_err(error)
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
                work_records INTEGER NOT NULL
             ) STRICT;
             CREATE TABLE edition(
                edition_id INTEGER PRIMARY KEY,
                work_id INTEGER
             ) STRICT;
             CREATE TABLE edition_isbn(
                isbn13 INTEGER NOT NULL,
                edition_id INTEGER NOT NULL,
                PRIMARY KEY(isbn13,edition_id)
             ) WITHOUT ROWID;
             CREATE TABLE edition_classification(
                edition_id INTEGER NOT NULL,
                scheme INTEGER NOT NULL CHECK(scheme IN (1,2)),
                notation TEXT NOT NULL,
                PRIMARY KEY(edition_id,scheme,notation)
             ) WITHOUT ROWID;
             CREATE TABLE work_classification(
                work_id INTEGER NOT NULL,
                scheme INTEGER NOT NULL CHECK(scheme IN (1,2)),
                notation TEXT NOT NULL,
                PRIMARY KEY(work_id,scheme,notation)
             ) WITHOUT ROWID;",
        )
        .map_err(error)
}

fn import_editions(connection: &mut Connection, path: &Path, limit: Option<usize>) -> Result<usize, MetadataError> {
    let transaction = connection.transaction().map_err(error)?;
    let mut edition_insert = transaction.prepare_cached("INSERT OR REPLACE INTO edition(edition_id,work_id) VALUES(?1,?2)").map_err(error)?;
    let mut isbn_insert = transaction.prepare_cached("INSERT OR IGNORE INTO edition_isbn(isbn13,edition_id) VALUES(?1,?2)").map_err(error)?;
    let mut classification_insert = transaction.prepare_cached("INSERT OR IGNORE INTO edition_classification(edition_id,scheme,notation) VALUES(?1,?2,?3)").map_err(error)?;
    let count = read_dump::<EditionRecord>(path, limit, |record| {
        let Some(edition_id) = open_library_id(&record.key, 'M') else { return Ok(()) };
        let work_id = record.works.first().and_then(|work| open_library_id(&work.key, 'W'));
        edition_insert.execute(params![edition_id, work_id]).map_err(error)?;
        let mut isbns = BTreeSet::new();
        isbns.extend(record.isbn_10.iter().filter_map(|isbn| canonical_isbn13(isbn)));
        isbns.extend(record.isbn_13.iter().filter_map(|isbn| canonical_isbn13(isbn)));
        for isbn in isbns {
            isbn_insert.execute(params![isbn, edition_id]).map_err(error)?;
        }
        insert_classifications(&mut classification_insert, edition_id, DDC_SCHEME, record.dewey_decimal_class)?;
        insert_classifications(&mut classification_insert, edition_id, LCC_SCHEME, record.lc_classifications)
    })?;
    drop(classification_insert);
    drop(isbn_insert);
    drop(edition_insert);
    transaction.commit().map_err(error)?;
    println!("imported {count} edition records");
    Ok(count)
}

fn import_works(connection: &mut Connection, path: &Path, limit: Option<usize>) -> Result<usize, MetadataError> {
    let transaction = connection.transaction().map_err(error)?;
    let mut classification_insert = transaction.prepare_cached("INSERT OR IGNORE INTO work_classification(work_id,scheme,notation) VALUES(?1,?2,?3)").map_err(error)?;
    let count = read_dump::<WorkRecord>(path, limit, |record| {
        let Some(work_id) = open_library_id(&record.key, 'W') else { return Ok(()) };
        insert_classifications(&mut classification_insert, work_id, DDC_SCHEME, record.dewey_number)?;
        insert_classifications(&mut classification_insert, work_id, LCC_SCHEME, record.lc_classifications)
    })?;
    drop(classification_insert);
    transaction.commit().map_err(error)?;
    println!("imported {count} work records");
    Ok(count)
}

fn insert_classifications(statement: &mut rusqlite::CachedStatement<'_>, owner_id: i64, scheme: i64, values: Vec<String>) -> Result<(), MetadataError> {
    for notation in values {
        let notation = notation.split_whitespace().collect::<Vec<_>>().join(" ");
        if notation.is_empty() || notation.len() > MAX_NOTATION_BYTES {
            continue;
        }
        statement.execute(params![owner_id, scheme, notation]).map_err(error)?;
    }
    Ok(())
}

fn read_dump<T: for<'de> Deserialize<'de>>(path: &Path, limit: Option<usize>, mut visit: impl FnMut(T) -> Result<(), MetadataError>) -> Result<usize, MetadataError> {
    let file = File::open(path).map_err(error)?;
    let mut reader = BufReader::with_capacity(1024 * 1024, MultiGzDecoder::new(file));
    let mut line = String::new();
    let mut count = 0_usize;
    loop {
        line.clear();
        let bytes = reader.read_line(&mut line).map_err(error)?;
        if bytes == 0 || limit.is_some_and(|limit| count >= limit) {
            break;
        }
        let Some(json) = line.splitn(5, '\t').nth(4) else { continue };
        let record = match serde_json::from_str::<T>(json.trim_end()) {
            Ok(record) => record,
            Err(parse_error) => {
                if count < 10 {
                    tracing::warn!(path = %path.display(), record = count + 1, error = %parse_error, "skipping malformed Open Library record");
                }
                count += 1;
                continue;
            }
        };
        visit(record)?;
        count += 1;
        if count.is_multiple_of(1_000_000) {
            println!("read {count} records from {}", path.display());
        }
    }
    Ok(count)
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
            collect_classifications(connection, "work_classification", "work_id", work_id, ClassificationSource::Work, &mut classifications)?;
        }
        matches.push(WorkClassificationMatch {
            open_library_work_id: work_id.map(|work_id| format!("OL{work_id}W")),
            exact_edition_ids: edition_ids.into_iter().map(|edition_id| format!("OL{edition_id}M")).collect(),
            classifications: classifications.into_iter().map(|((scheme, notation), source)| Classification { scheme, notation, source }).collect(),
        });
    }
    let status = match matches.len() {
        0 => LookupStatus::NoMatch,
        1 => LookupStatus::Matched,
        _ => LookupStatus::Ambiguous,
    };
    Ok(IsbnClassificationResult { requested_isbn, canonical_isbn13: Some(format!("{isbn13:013}")), status, matches })
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

fn open_read_only(path: &Path) -> Result<Connection, MetadataError> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(error)?;
    connection.execute_batch("PRAGMA query_only=ON; PRAGMA cache_size=-65536; PRAGMA mmap_size=268435456;").map_err(error)?;
    Ok(connection)
}

fn open_library_id(value: &str, suffix: char) -> Option<i64> {
    let digits = value
        .strip_prefix(if suffix == 'M' {
            "/books/OL"
        } else if suffix == 'W' {
            "/works/OL"
        } else {
            return None;
        })?
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

fn deserialize_key_references<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Vec<KeyReference>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    let values = match value {
        serde_json::Value::Array(values) => values,
        value @ serde_json::Value::Object(_) => vec![value],
        _ => Vec::new(),
    };
    Ok(values.into_iter().filter_map(|value| value.get("key").and_then(serde_json::Value::as_str).map(|key| KeyReference { key: key.to_owned() })).collect())
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
