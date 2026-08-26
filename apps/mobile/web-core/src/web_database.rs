use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::io::Cursor;
use std::path::Path;

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sqlite_wasm_vfs::sahpool::{OpfsSAHPoolCfgBuilder, install};
use wasm_bindgen::prelude::*;

const DATABASE_FILE: &str = "bokheim-web-rust-v5.sqlite3";
const CATALOG_CACHE_FILE: &str = "bokheim-catalog-cache-v5.sqlite3";
const VFS_NAME: &str = "bokheim-rust-opfs-sahpool-v1";
const VFS_DIRECTORY: &str = ".bokheim-rust-opfs-sahpool-v1";

struct BrowserDatabases {
    global: Connection,
    libraries: HashMap<String, Connection>,
    catalog_cache: catalog_core::CatalogCache,
    catalog_sources: Vec<catalog_contract::CatalogSource>,
    http: reqwest::Client,
    taxonomy: taxonomy_client::TaxonomyClient,
    persistent: bool,
}

thread_local! {
    static DATABASES: RefCell<Option<BrowserDatabases>> = const { RefCell::new(None) };
}

const RUST_REGISTRY_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS rust_library_registry (
    library_id TEXT PRIMARY KEY NOT NULL,
    library_name TEXT NOT NULL CHECK(length(trim(library_name)) BETWEEN 1 AND 255),
    created_at INTEGER NOT NULL,
    remote INTEGER NOT NULL DEFAULT 0 CHECK(remote IN (0,1)),
    owner_user_id TEXT
) STRICT;
CREATE TABLE IF NOT EXISTS rust_account_profile (
    singleton INTEGER PRIMARY KEY NOT NULL CHECK(singleton=1),
    user_id TEXT NOT NULL,
    email TEXT NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS rust_preferences (
    key TEXT PRIMARY KEY NOT NULL,
    value_json TEXT NOT NULL CHECK(json_valid(value_json))
) STRICT;
CREATE TABLE IF NOT EXISTS rust_backend_metadata (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS rust_taxonomy_concept (
    concept_id TEXT PRIMARY KEY NOT NULL,
    concept_json TEXT NOT NULL CHECK(json_valid(concept_json))
) STRICT;
CREATE TABLE IF NOT EXISTS rust_taxonomy_resolution (
    system_id TEXT NOT NULL CHECK(system_id IN ('bisac','ddc','lcc')),
    code TEXT NOT NULL,
    server_release_id TEXT NOT NULL,
    matched_concept_ids_json TEXT NOT NULL CHECK(json_valid(matched_concept_ids_json)),
    PRIMARY KEY(system_id,code)
) STRICT;
"#;

const BROWSER_LIBRARY_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS browser_publication_asset (
    content_hash TEXT PRIMARY KEY NOT NULL,
    publication_bytes BLOB NOT NULL,
    cover_bytes BLOB,
    toc_json TEXT NOT NULL DEFAULT '[]' CHECK(json_valid(toc_json))
) STRICT;
CREATE TABLE IF NOT EXISTS browser_asset_transfer (
    content_hash TEXT NOT NULL,
    kind TEXT NOT NULL CHECK(kind IN ('book','thumbnail')),
    direction TEXT NOT NULL CHECK(direction IN ('upload','download')),
    state TEXT NOT NULL CHECK(state IN ('running','completed','failed')),
    attempts INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY(content_hash,kind,direction)
) STRICT;
"#;

#[derive(Clone, Serialize)]
struct BrowserLibraryEntry {
    library_id: String,
    library_name: String,
    library_path: String,
}

#[derive(Deserialize, Serialize)]
struct ImportedPublication {
    content_hash: String,
}

#[derive(Serialize)]
struct PublicationAsset {
    file_name: String,
    media_type: &'static str,
    #[serde(with = "serde_bytes")]
    publication_bytes: Vec<u8>,
}

fn publication_media_type(file_name: &str) -> &'static str {
    match Path::new(file_name).extension().and_then(|value| value.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("pdf") => "application/pdf",
        _ => "application/epub+zip",
    }
}

struct LocalPublicationAsset {
    content_hash: sync_common::ContentHash,
    publication_bytes: Vec<u8>,
    cover_bytes: Vec<u8>,
    thumbnail_uploaded: bool,
    thumbnail_downloaded: bool,
}

#[derive(Serialize)]
struct BrowserStorageUsage {
    library_id: String,
    library_name: String,
    used_bytes: u64,
}

#[derive(Serialize)]
struct BrowserTransferStatus {
    id: String,
    kind: String,
    content_hash: String,
    book_content_hash: Option<String>,
    path: Option<String>,
    state: String,
    origin: &'static str,
    attempts: u32,
    completed_items: u32,
    total_items: u32,
    last_error: Option<String>,
    next_retry_at: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Serialize)]
struct BrowserLibraryTransfers {
    library_id: String,
    library_name: String,
    scanner_running: bool,
    transfers: Vec<BrowserTransferStatus>,
    history: Vec<BrowserTransferStatus>,
}

fn js_error(error: impl ToString) -> JsValue {
    JsValue::from_str(&error.to_string())
}

fn api_url(path: &str) -> String {
    web_sys::window().and_then(|window| window.location().origin().ok()).filter(|origin| !origin.is_empty() && origin != "null").map_or_else(|| format!("http://localhost{path}"), |origin| format!("{origin}{path}"))
}

fn unix_millis() -> Result<i64, String> {
    Ok(js_sys::Date::now() as i64)
}

fn optional_js_millis(value: Option<f64>) -> Result<i64, String> {
    match value {
        None => unix_millis(),
        Some(value) if value.is_finite() && value >= 0.0 && value <= i64::MAX as f64 => Ok(value.trunc() as i64),
        Some(_) => Err("timestamp must be a non-negative finite number of milliseconds".to_owned()),
    }
}

fn parse_library_id(library_id: &str) -> Result<String, String> {
    uuid::Uuid::parse_str(library_id).map(|id| id.to_string()).map_err(|error| format!("invalid library id: {error}"))
}

fn library_database_file(library_id: &str) -> String {
    format!("bokheim-library-v4-{library_id}.sqlite3")
}

fn open_library_connection(library_id: &str, persistent: bool) -> Result<Connection, rusqlite::Error> {
    let connection = if persistent { Connection::open(library_database_file(library_id))? } else { Connection::open_in_memory()? };
    library_sqlite::init_db(&connection)?;
    connection.execute_batch(BROWSER_LIBRARY_SCHEMA)?;
    Ok(connection)
}

fn with_library<T>(library_id: &str, operation: impl FnOnce(&Connection) -> Result<T, String>) -> Result<T, JsValue> {
    let library_id = parse_library_id(library_id).map_err(js_error)?;
    DATABASES.with(|slot| {
        let mut databases = slot.borrow_mut();
        let databases = databases.as_mut().ok_or_else(|| js_error("The Rust browser database is not open"))?;
        let registered = databases.global.query_row("SELECT EXISTS(SELECT 1 FROM rust_library_registry WHERE library_id=?1)", [&library_id], |row| row.get::<_, bool>(0)).map_err(js_error)?;
        if !registered {
            return Err(js_error("The library does not exist"));
        }
        if !databases.libraries.contains_key(&library_id) {
            let connection = open_library_connection(&library_id, databases.persistent).map_err(js_error)?;
            databases.libraries.insert(library_id.clone(), connection);
        }
        operation(databases.libraries.get(&library_id).expect("the library connection was inserted")).map_err(js_error)
    })
}

fn with_global<T>(operation: impl FnOnce(&Connection) -> Result<T, String>) -> Result<T, JsValue> {
    DATABASES.with(|slot| {
        let borrowed = slot.borrow();
        let connection = &borrowed.as_ref().ok_or_else(|| js_error("The Rust browser database is not open"))?.global;
        operation(connection).map_err(js_error)
    })
}

fn persisted_taxonomy_concepts(connection: &Connection) -> Result<Vec<taxonomy_contract::TaxonomyConcept>, String> {
    let mut statement = connection.prepare("SELECT concept_json FROM rust_taxonomy_concept ORDER BY concept_id").map_err(|error| error.to_string())?;
    statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| error.to_string())?
        .map(|row| {
            let json = row.map_err(|error| error.to_string())?;
            serde_json::from_str(&json).map_err(|error| error.to_string())
        })
        .collect()
}

fn install_persisted_taxonomy(connection: &Connection) -> Result<(), String> {
    let concepts = persisted_taxonomy_concepts(connection)?;
    library_model::install_unified_taxonomy_overlay(taxonomy_client::concept_definitions(&concepts))
}

fn with_library_mut<T>(library_id: &str, operation: impl FnOnce(&mut Connection) -> Result<T, String>) -> Result<T, JsValue> {
    let library_id = parse_library_id(library_id).map_err(js_error)?;
    DATABASES.with(|slot| {
        let mut databases = slot.borrow_mut();
        let databases = databases.as_mut().ok_or_else(|| js_error("The Rust browser database is not open"))?;
        let registered = databases.global.query_row("SELECT EXISTS(SELECT 1 FROM rust_library_registry WHERE library_id=?1)", [&library_id], |row| row.get::<_, bool>(0)).map_err(js_error)?;
        if !registered {
            return Err(js_error("The library does not exist"));
        }
        if !databases.libraries.contains_key(&library_id) {
            let connection = open_library_connection(&library_id, databases.persistent).map_err(js_error)?;
            databases.libraries.insert(library_id.clone(), connection);
        }
        operation(databases.libraries.get_mut(&library_id).expect("the library connection was inserted")).map_err(js_error)
    })
}

fn library_entry(library_id: String, library_name: String) -> BrowserLibraryEntry {
    BrowserLibraryEntry { library_path: format!("opfs://bokheim/{library_id}"), library_id, library_name }
}

fn registered_library_entries() -> Result<Vec<BrowserLibraryEntry>, JsValue> {
    DATABASES.with(|slot| {
        let borrowed = slot.borrow();
        let global = &borrowed.as_ref().ok_or_else(|| js_error("The Rust browser database is not open"))?.global;
        let mut statement = global.prepare("SELECT library_id,library_name FROM rust_library_registry ORDER BY library_name COLLATE NOCASE,library_id").map_err(js_error)?;
        statement.query_map([], |row| Ok(library_entry(row.get(0)?, row.get(1)?))).map_err(js_error)?.collect::<Result<Vec<_>, _>>().map_err(js_error)
    })
}

/// Opens a fresh Rust-owned browser database. The VFS and filename deliberately
/// differ from the former JavaScript SQLite database: browser data is not
/// migrated implicitly.
#[wasm_bindgen]
pub async fn bokheim_database_open() -> Result<String, JsValue> {
    console_error_panic_hook::set_once();
    let config = OpfsSAHPoolCfgBuilder::new().vfs_name(VFS_NAME).directory(VFS_DIRECTORY).clear_on_init(false).initial_capacity(8).build();

    let (connection, persistence, persistent) = match install::<rusqlite::ffi::WasmOsCallback>(&config, true).await {
        Ok(_) => (Connection::open(DATABASE_FILE).map_err(js_error)?, "opfs", true),
        Err(_) => (Connection::open_in_memory().map_err(js_error)?, "memory", false),
    };
    connection.busy_timeout(std::time::Duration::from_secs(5)).map_err(js_error)?;
    connection.execute_batch(RUST_REGISTRY_SCHEMA).map_err(js_error)?;
    install_persisted_taxonomy(&connection).map_err(js_error)?;
    let catalog_cache = catalog_core::CatalogCache::open_vfs(if persistent { CATALOG_CACHE_FILE } else { ":memory:" }).map_err(js_error)?;
    let http = reqwest::Client::new();
    let taxonomy = taxonomy_client::TaxonomyClient::new("https://taxonomy.bokheim.se/").map_err(js_error)?;
    DATABASES.with(|slot| {
        *slot.borrow_mut() = Some(BrowserDatabases { global: connection, libraries: HashMap::new(), catalog_cache, catalog_sources: Vec::new(), http, taxonomy, persistent });
    });
    Ok(persistence.to_owned())
}

#[wasm_bindgen]
pub fn bokheim_database_sqlite_version() -> Result<String, JsValue> {
    DATABASES.with(|slot| {
        let borrowed = slot.borrow();
        borrowed.as_ref().ok_or_else(|| js_error("The Rust browser database is not open"))?.global.query_row("SELECT sqlite_version()", [], |row| row.get(0)).map_err(js_error)
    })
}

fn read_preferences<T>(connection: &Connection, key: &str) -> Result<T, String>
where
    T: serde::de::DeserializeOwned + Default,
{
    let json = connection.query_row("SELECT value_json FROM rust_preferences WHERE key=?1", [key], |row| row.get::<_, String>(0)).optional().map_err(|error| error.to_string())?;
    json.map_or_else(|| Ok(T::default()), |json| serde_json::from_str(&json).map_err(|error| error.to_string()))
}

fn store_preferences<T: Serialize>(connection: &Connection, key: &str, preferences: &T) -> Result<(), String> {
    let json = serde_json::to_string(preferences).map_err(|error| error.to_string())?;
    connection.execute("INSERT INTO rust_preferences(key,value_json) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json", rusqlite::params![key, json]).map_err(|error| error.to_string())?;
    Ok(())
}

fn browsing_preferences() -> Result<app_contract::preferences::BrowsingPreferences, JsValue> {
    with_global(|connection| read_preferences(connection, "browsing"))
}

#[wasm_bindgen]
pub fn bokheim_preferences_reader_get() -> Result<JsValue, JsValue> {
    let preferences: app_contract::preferences::ReaderPreferences = with_global(|connection| read_preferences(connection, "reader"))?;
    serde_wasm_bindgen::to_value(&preferences).map_err(js_error)
}

pub fn bokheim_preferences_reader_store(preferences: JsValue) -> Result<(), JsValue> {
    let preferences: app_contract::preferences::ReaderPreferences = serde_wasm_bindgen::from_value(preferences).map_err(js_error)?;
    with_global(|connection| store_preferences(connection, "reader", &preferences))
}

#[wasm_bindgen]
pub fn bokheim_preferences_reader_update(patch: JsValue) -> Result<JsValue, JsValue> {
    let patch: app_contract::preferences::ReaderPreferencePatch = serde_wasm_bindgen::from_value(patch).map_err(js_error)?;
    let preferences = with_global(|connection| {
        let mut preferences: app_contract::preferences::ReaderPreferences = read_preferences(connection, "reader")?;
        patch.apply(&mut preferences).map_err(|error| error.to_string())?;
        store_preferences(connection, "reader", &preferences)?;
        Ok(preferences)
    })?;
    serde_wasm_bindgen::to_value(&preferences).map_err(js_error)
}

#[wasm_bindgen]
pub fn bokheim_preferences_browsing_get() -> Result<JsValue, JsValue> {
    serde_wasm_bindgen::to_value(&browsing_preferences()?).map_err(js_error)
}

#[wasm_bindgen]
pub fn bokheim_preferences_browsing_update(patch: JsValue) -> Result<JsValue, JsValue> {
    let patch: app_contract::preferences::BrowsingPreferencePatch = serde_wasm_bindgen::from_value(patch).map_err(js_error)?;
    let preferences = with_global(|connection| {
        let mut preferences: app_contract::preferences::BrowsingPreferences = read_preferences(connection, "browsing")?;
        patch.apply(&mut preferences).map_err(|error| error.to_string())?;
        store_preferences(connection, "browsing", &preferences)?;
        Ok(preferences)
    })?;
    serde_wasm_bindgen::to_value(&preferences).map_err(js_error)
}

pub fn bokheim_browser_session_get() -> Result<JsValue, JsValue> {
    let session: app_contract::backend::BrowserSession = with_global(|connection| read_preferences(connection, "browser_session"))?;
    serde_wasm_bindgen::to_value(&session).map_err(js_error)
}

pub fn bokheim_browser_session_store(session: JsValue) -> Result<(), JsValue> {
    let session: app_contract::backend::BrowserSession = serde_wasm_bindgen::from_value(session).map_err(js_error)?;
    with_global(|connection| store_preferences(connection, "browser_session", &session))
}

const CATALOG_ORIGIN: &str = "https://catalogs.bokheim.se";
const CATALOG_API: &str = "https://catalogs.bokheim.se/api/web";
const SUBJECT_COLLECTION_REVALIDATE_AFTER_SECONDS: u64 = 5 * 60;
const MAX_CATALOG_RESPONSE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_CATALOG_COVER_BYTES: u64 = 8 * 1024 * 1024;
const MAX_CATALOG_ACQUISITION_BYTES: u64 = 1_000_000_000;

#[derive(Serialize)]
struct BrowserCatalogPageView {
    page: catalog_contract::CatalogPage,
    owned_publications: HashSet<app_contract::catalog::CatalogPublicationKey>,
}

#[derive(Serialize)]
struct BrowserCatalogSubjectView {
    response: catalog_contract::CatalogSubjectBrowseResponse,
    owned_publications: HashSet<app_contract::catalog::CatalogPublicationKey>,
}

#[derive(Serialize)]
struct BrowserCatalogAcquisition {
    file_name: String,
    media_type: String,
    #[serde(with = "serde_bytes")]
    bytes: Vec<u8>,
}

fn to_js_value<T: Serialize>(value: &T) -> Result<JsValue, JsValue> {
    let serializer = serde_wasm_bindgen::Serializer::new().serialize_missing_as_null(true);
    value.serialize(&serializer).map_err(js_error)
}

fn catalog_components() -> Result<(catalog_core::CatalogCache, reqwest::Client), JsValue> {
    DATABASES.with(|slot| {
        let borrowed = slot.borrow();
        let databases = borrowed.as_ref().ok_or_else(|| js_error("The Rust browser database is not open"))?;
        Ok((databases.catalog_cache.clone(), databases.http.clone()))
    })
}

fn owned_catalog_publications(library_id: &str) -> Result<HashSet<app_contract::catalog::CatalogPublicationKey>, JsValue> {
    with_library(library_id, |connection| library_sqlite::catalog::all_owned_catalog_publications(connection).map(|values| values.into_iter().collect()).map_err(|error| error.to_string()))
}

async fn checked_catalog_response(response: reqwest::Response, byte_limit: u64) -> Result<reqwest::Response, JsValue> {
    if !response.status().is_success() {
        let status = response.status();
        let detail = response.text().await.unwrap_or_default().chars().take(4_096).collect::<String>();
        return Err(js_error(if detail.trim().is_empty() { format!("Catalogue server returned HTTP {status}") } else { detail }));
    }
    if response.content_length().is_some_and(|length| length > byte_limit) {
        return Err(js_error("Catalogue server response is too large"));
    }
    Ok(response)
}

async fn catalog_get_json<T: serde::de::DeserializeOwned>(http: &reqwest::Client, path: &str) -> Result<T, JsValue> {
    let response = checked_catalog_response(http.get(format!("{CATALOG_API}{path}")).header(reqwest::header::ACCEPT, "application/json").send().await.map_err(js_error)?, MAX_CATALOG_RESPONSE_BYTES).await?;
    let bytes = response.bytes().await.map_err(js_error)?;
    if bytes.len() as u64 > MAX_CATALOG_RESPONSE_BYTES {
        return Err(js_error("Catalogue server response is too large"));
    }
    serde_json::from_slice(&bytes).map_err(js_error)
}

async fn catalog_post_json<T: Serialize + ?Sized, R: serde::de::DeserializeOwned>(http: &reqwest::Client, path: &str, request: &T) -> Result<R, JsValue> {
    let response = checked_catalog_response(http.post(format!("{CATALOG_API}{path}")).header(reqwest::header::ACCEPT, "application/json").json(request).send().await.map_err(js_error)?, MAX_CATALOG_RESPONSE_BYTES).await?;
    let bytes = response.bytes().await.map_err(js_error)?;
    if bytes.len() as u64 > MAX_CATALOG_RESPONSE_BYTES {
        return Err(js_error("Catalogue server response is too large"));
    }
    serde_json::from_slice(&bytes).map_err(js_error)
}

fn catalog_languages() -> Result<Vec<String>, JsValue> {
    Ok(browsing_preferences()?.included_catalog_languages().to_vec())
}

async fn synchronize_subject_collection(cache: &catalog_core::CatalogCache, http: &reqwest::Client, locator: Option<String>) -> Result<(), JsValue> {
    for _ in 0..3 {
        let mut offset = 0_u32;
        let mut snapshot: Option<(String, u64, u32)> = None;
        loop {
            let request = catalog_contract::CatalogSubjectBrowseRequest { locator: locator.clone(), range: catalog_contract::CatalogResultRange::new(offset, 100), ..Default::default() };
            let (epoch, revision, known_entry_ids) = cache.known_subject_collection_chunk(&request).map_err(js_error)?;
            let refresh = catalog_contract::CatalogSubjectBrowseRefreshRequest { request: request.clone(), epoch, revision, known_entry_ids, identifiers_only: false, collection_sync: true };
            let delta: catalog_contract::CatalogSubjectBrowseDeltaResponse = catalog_post_json(http, "/subject-collection", &refresh).await?;
            let received = (delta.epoch.clone(), delta.revision, delta.range.total_items);
            if snapshot.as_ref().is_some_and(|snapshot| snapshot != &received) {
                break;
            }
            snapshot = Some(received);
            let returned = delta.range.returned_items;
            if cache.stage_subject_collection_chunk(&request, delta).map_err(js_error)? {
                return Ok(());
            }
            if returned == 0 {
                return Err(js_error("Catalogue subject collection synchronization made no progress"));
            }
            offset = offset.checked_add(returned).ok_or_else(|| js_error("Catalogue subject collection offset overflow"))?;
        }
    }
    Err(js_error("Catalogue subject collection kept changing during synchronization"))
}

async fn ensure_subject_collection(cache: &catalog_core::CatalogCache, http: &reqwest::Client, locator: Option<String>) -> Result<(), JsValue> {
    let state = cache.cached_subject_collection_state(locator.as_deref()).map_err(js_error)?;
    let Some(state) = state else {
        return synchronize_subject_collection(cache, http, locator).await;
    };
    let now = (js_sys::Date::now() / 1_000.0) as u64;
    if now.saturating_sub(state.checked_at) < SUBJECT_COLLECTION_REVALIDATE_AFTER_SECONDS {
        return Ok(());
    }
    let manifest: catalog_contract::CatalogSubjectCollectionManifestResponse = catalog_post_json(http, "/subject-collection/manifest", &catalog_contract::CatalogSubjectCollectionManifestRequest { locator: locator.clone() }).await?;
    if manifest.epoch != state.epoch || manifest.revision != state.revision {
        synchronize_subject_collection(cache, http, locator).await
    } else {
        cache.touch_subject_collection(locator.as_deref(), &manifest.epoch, manifest.revision).map_err(js_error)?;
        Ok(())
    }
}

#[wasm_bindgen]
pub fn bokheim_catalog_sources() -> Result<JsValue, JsValue> {
    DATABASES.with(|slot| {
        let borrowed = slot.borrow();
        let sources = &borrowed.as_ref().ok_or_else(|| js_error("The Rust browser database is not open"))?.catalog_sources;
        to_js_value(sources)
    })
}

#[wasm_bindgen]
pub async fn bokheim_catalog_refresh_sources() -> Result<JsValue, JsValue> {
    let (_, http) = catalog_components()?;
    let sources: Vec<catalog_contract::CatalogSource> = catalog_get_json(&http, "/catalogs").await?;
    DATABASES.with(|slot| {
        slot.borrow_mut().as_mut().ok_or_else(|| js_error("The Rust browser database is not open"))?.catalog_sources = sources.clone();
        Ok::<(), JsValue>(())
    })?;
    to_js_value(&sources)
}

#[wasm_bindgen]
pub async fn bokheim_catalog_page(library_id: String, source_id: String, locator: Option<String>) -> Result<JsValue, JsValue> {
    let (_, http) = catalog_components()?;
    let source_id = catalog_contract::CatalogSourceId::parse(&source_id).map_err(js_error)?;
    let request = catalog_contract::CatalogPageRequest { locator, supported_languages: catalog_languages()?, epoch: None, known_publications: Vec::new() };
    let page: catalog_contract::CatalogPage = catalog_post_json(&http, &format!("/sources/{}/page", source_id.as_str()), &request).await?;
    to_js_value(&BrowserCatalogPageView { page, owned_publications: owned_catalog_publications(&library_id)? })
}

#[wasm_bindgen]
pub async fn bokheim_catalog_search(library_id: String, source_id: String, query: JsValue) -> Result<JsValue, JsValue> {
    let (_, http) = catalog_components()?;
    let source_id = catalog_contract::CatalogSourceId::parse(&source_id).map_err(js_error)?;
    let query: catalog_contract::CatalogSearchQuery = serde_wasm_bindgen::from_value(query).map_err(js_error)?;
    let terms = [Some(query.query.as_str()), query.title.as_deref(), query.author.as_deref()].into_iter().flatten().map(str::trim).filter(|value| !value.is_empty()).collect::<Vec<_>>().join(" ");
    if terms.is_empty() {
        return Err(js_error("Catalogue search cannot be empty"));
    }
    let request = catalog_contract::CatalogSearchRequest { query: terms, supported_languages: catalog_languages()?, epoch: None, known_publications: Vec::new() };
    let page: catalog_contract::CatalogPage = catalog_post_json(&http, &format!("/sources/{}/search", source_id.as_str()), &request).await?;
    to_js_value(&BrowserCatalogPageView { page, owned_publications: owned_catalog_publications(&library_id)? })
}

#[wasm_bindgen]
pub async fn bokheim_catalog_subjects(library_id: String, locator: Option<String>, source_ids: JsValue, query: String, range: JsValue) -> Result<JsValue, JsValue> {
    let (cache, http) = catalog_components()?;
    let source_ids = serde_wasm_bindgen::from_value::<Vec<String>>(source_ids).map_err(js_error)?.into_iter().map(|source_id| catalog_contract::CatalogSourceId::parse(&source_id)).collect::<Result<Vec<_>, _>>().map_err(js_error)?;
    let range: catalog_contract::CatalogResultRange = serde_wasm_bindgen::from_value(range).map_err(js_error)?;
    ensure_subject_collection(&cache, &http, locator.clone()).await?;
    let request = catalog_contract::CatalogSubjectBrowseRequest { locator, supported_languages: catalog_languages()?, source_ids, query, range, ..Default::default() };
    let response = cache.cached_subject_browse_response(&request).map_err(js_error)?.ok_or_else(|| js_error("Catalogue subject collection is not cached"))?.response;
    to_js_value(&BrowserCatalogSubjectView { response, owned_publications: owned_catalog_publications(&library_id)? })
}

#[wasm_bindgen]
pub async fn bokheim_catalog_cover(source_id: String, image_url: String) -> Result<Vec<u8>, JsValue> {
    let (_, http) = catalog_components()?;
    let source_id = catalog_contract::CatalogSourceId::parse(&source_id).map_err(js_error)?;
    let mut url = reqwest::Url::parse(&format!("{CATALOG_ORIGIN}/api/opds/sources/{}/cover", source_id.as_str())).map_err(js_error)?;
    url.query_pairs_mut().append_pair("image_url", &image_url);
    let response = checked_catalog_response(http.get(url).send().await.map_err(js_error)?, MAX_CATALOG_COVER_BYTES).await?;
    let media_type = response.headers().get(reqwest::header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).and_then(|value| value.split(';').next()).unwrap_or("");
    if media_type != "image/jpeg" {
        return Err(js_error("Catalogue cover was not a JPEG image"));
    }
    let bytes = response.bytes().await.map_err(js_error)?;
    if bytes.is_empty() || bytes.len() as u64 > MAX_CATALOG_COVER_BYTES {
        return Err(js_error("Catalogue cover is empty or too large"));
    }
    Ok(bytes.to_vec())
}

#[wasm_bindgen]
pub async fn bokheim_catalog_acquire(request: JsValue) -> Result<JsValue, JsValue> {
    let (_, http) = catalog_components()?;
    let request: catalog_contract::CatalogImportRequest = serde_wasm_bindgen::from_value(request).map_err(js_error)?;
    if !matches!(request.book_format, catalog_contract::CatalogBookFormat::Epub | catalog_contract::CatalogBookFormat::Pdf) {
        return Err(js_error("The web library currently accepts EPUB and PDF catalogue downloads only"));
    }
    let expected_media_type = request.book_format.media_type();
    let response = checked_catalog_response(
        http.post(format!("{CATALOG_API}/sources/{}/acquire", request.source_id.as_str())).header(reqwest::header::ACCEPT, expected_media_type).json(&request).send().await.map_err(js_error)?,
        MAX_CATALOG_ACQUISITION_BYTES,
    )
    .await?;
    let media_type = response.headers().get(reqwest::header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).and_then(|value| value.split(';').next()).unwrap_or("").to_owned();
    if media_type != expected_media_type {
        return Err(js_error("Catalogue acquisition has an unexpected media type"));
    }
    let bytes = response.bytes().await.map_err(js_error)?;
    if bytes.is_empty() || bytes.len() as u64 > MAX_CATALOG_ACQUISITION_BYTES {
        return Err(js_error("Catalogue acquisition is empty or too large"));
    }
    to_js_value(&BrowserCatalogAcquisition { file_name: request.suggested_file_name(), media_type, bytes: bytes.to_vec() })
}

fn taxonomy_client() -> Result<taxonomy_client::TaxonomyClient, JsValue> {
    DATABASES.with(|slot| Ok(slot.borrow().as_ref().ok_or_else(|| js_error("The Rust browser database is not open"))?.taxonomy.clone()))
}

fn reconcile_taxonomy_release(connection: &Connection, release_id: &str) -> Result<bool, String> {
    let previous = connection.query_row("SELECT value FROM rust_backend_metadata WHERE key='taxonomy_server_release_id'", [], |row| row.get::<_, String>(0)).optional().map_err(|error| error.to_string())?;
    let obsolete = previous.as_deref().is_some_and(|previous| previous != release_id);
    let transaction = connection.unchecked_transaction().map_err(|error| error.to_string())?;
    if obsolete {
        transaction.execute("DELETE FROM rust_taxonomy_resolution", []).map_err(|error| error.to_string())?;
        transaction.execute("DELETE FROM rust_taxonomy_concept", []).map_err(|error| error.to_string())?;
    }
    transaction.execute("INSERT INTO rust_backend_metadata(key,value) VALUES('taxonomy_server_release_id',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [release_id]).map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(obsolete)
}

fn normalize_taxonomy_codes(codes: Vec<taxonomy_contract::TaxonomyCode>) -> Result<Vec<taxonomy_contract::TaxonomyCode>, JsValue> {
    let codes = codes
        .into_iter()
        .map(|mut code| {
            code.code = code.code.trim().to_ascii_uppercase();
            if code.code.is_empty() || code.code.len() > taxonomy_contract::MAX_CODE_BYTES {
                return Err(js_error(format!("Taxonomy code must contain between 1 and {} bytes", taxonomy_contract::MAX_CODE_BYTES)));
            }
            Ok(code)
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    Ok(codes.into_iter().collect())
}

async fn hydrate_taxonomy_codes(codes: Vec<taxonomy_contract::TaxonomyCode>) -> Result<(), JsValue> {
    let codes = normalize_taxonomy_codes(codes)?;
    let client = taxonomy_client()?;
    let advertised_release = client.current_release_id().await.ok();
    if let Some(release_id) = advertised_release.as_deref() {
        with_global(|connection| {
            if reconcile_taxonomy_release(connection, release_id)? {
                install_persisted_taxonomy(connection)?;
            }
            Ok(())
        })?;
    }

    let missing = with_global(|connection| {
        let mut missing = Vec::new();
        let mut persisted = connection.prepare("SELECT EXISTS(SELECT 1 FROM rust_taxonomy_resolution WHERE system_id=?1 AND code=?2)").map_err(|error| error.to_string())?;
        for code in codes {
            if subject_projection::unified_has_exact_source_code(code.system.as_str(), &code.code) {
                continue;
            }
            let known = persisted.query_row(rusqlite::params![code.system.as_str(), code.code], |row| row.get::<_, bool>(0)).map_err(|error| error.to_string())?;
            if !known {
                missing.push(code);
            }
        }
        Ok(missing)
    })?;
    let Some(response) = client.resolve_codes(missing).await.map_err(js_error)? else { return Ok(()) };
    with_global(|connection| {
        reconcile_taxonomy_release(connection, &response.release_id)?;
        let transaction = connection.unchecked_transaction().map_err(|error| error.to_string())?;
        for concept in &response.concepts {
            transaction
                .execute(
                    "INSERT INTO rust_taxonomy_concept(concept_id,concept_json) VALUES(?1,?2) ON CONFLICT(concept_id) DO UPDATE SET concept_json=excluded.concept_json",
                    rusqlite::params![concept.concept_id, serde_json::to_string(concept).map_err(|error| error.to_string())?],
                )
                .map_err(|error| error.to_string())?;
        }
        for result in &response.results {
            transaction
                .execute(
                    "INSERT INTO rust_taxonomy_resolution(system_id,code,server_release_id,matched_concept_ids_json) VALUES(?1,?2,?3,?4)
                     ON CONFLICT(system_id,code) DO UPDATE SET server_release_id=excluded.server_release_id,matched_concept_ids_json=excluded.matched_concept_ids_json",
                    rusqlite::params![result.system.as_str(), result.code, response.release_id, serde_json::to_string(&result.matched_concept_ids).map_err(|error| error.to_string())?],
                )
                .map_err(|error| error.to_string())?;
        }
        transaction.commit().map_err(|error| error.to_string())?;
        install_persisted_taxonomy(connection)
    })
}

#[wasm_bindgen]
pub async fn bokheim_taxonomy_hydrate(codes: JsValue) -> Result<(), JsValue> {
    let codes = serde_wasm_bindgen::from_value(codes).map_err(js_error)?;
    hydrate_taxonomy_codes(codes).await
}

#[derive(Deserialize)]
struct LoosePublicationSubject {
    #[serde(default)]
    id: Option<String>,
    name: String,
    #[serde(default = "default_subject_source")]
    source: String,
    #[serde(default)]
    authority: Option<String>,
    #[serde(default)]
    code: Option<String>,
}

fn default_subject_source() -> String {
    "dc:subject".to_owned()
}

fn projected_taxonomy_codes(publication: &serde_json::Map<String, serde_json::Value>) -> Vec<taxonomy_contract::TaxonomyCode> {
    let subjects = publication.get("subjects").and_then(serde_json::Value::as_array).into_iter().flatten().filter_map(|value| serde_json::from_value::<LoosePublicationSubject>(value.clone()).ok());
    let subjects = subjects.filter_map(|subject| publication_model::PublicationSubject::new(subject.id, subject.name, subject.source, subject.authority, subject.code).ok()).collect::<Vec<_>>();
    subject_projection::project_subject_codes(&subjects)
        .into_iter()
        .filter_map(|classification| {
            let system = match classification.system_id() {
                subject_projection::BISAC_SYSTEM_ID => taxonomy_contract::TaxonomySystem::Bisac,
                subject_projection::DDC_SYSTEM_ID => taxonomy_contract::TaxonomySystem::Ddc,
                subject_projection::LCC_SYSTEM_ID => taxonomy_contract::TaxonomySystem::Lcc,
                _ => return None,
            };
            Some(taxonomy_contract::TaxonomyCode { system, code: classification.code().to_owned() })
        })
        .collect()
}

fn mutation_publication(value: &mut serde_json::Value) -> Option<&mut serde_json::Map<String, serde_json::Value>> {
    let root = value.as_object_mut()?;
    if root.contains_key("publication") {
        return root.get_mut("publication")?.as_object_mut();
    }
    root.get_mut("present")?.as_object_mut()?.get_mut("metadata")?.as_object_mut()?.get_mut("publication")?.as_object_mut()
}

#[wasm_bindgen]
pub async fn bokheim_taxonomy_hydrate_mutations(mutations: JsValue) -> Result<JsValue, JsValue> {
    let mut mutations: Vec<serde_json::Value> = serde_wasm_bindgen::from_value(mutations).map_err(js_error)?;
    let codes = mutations
        .iter_mut()
        .filter(|mutation| matches!(mutation.get("kind").and_then(serde_json::Value::as_str), Some("metadata" | "book_lifecycle")))
        .filter_map(|mutation| mutation.get_mut("value"))
        .filter_map(mutation_publication)
        .flat_map(|publication| projected_taxonomy_codes(publication))
        .collect();
    hydrate_taxonomy_codes(codes).await?;
    for publication in
        mutations.iter_mut().filter(|mutation| matches!(mutation.get("kind").and_then(serde_json::Value::as_str), Some("metadata" | "book_lifecycle"))).filter_map(|mutation| mutation.get_mut("value")).filter_map(mutation_publication)
    {
        let mut paths = publication.get("subject_paths").and_then(serde_json::Value::as_array).into_iter().flatten().filter_map(serde_json::Value::as_str).map(str::to_owned).collect::<BTreeSet<_>>();
        for code in projected_taxonomy_codes(publication) {
            paths.extend(subject_projection::unified_subject_paths(code.system.as_str(), code.code.trim()));
        }
        publication.insert("subject_paths".to_owned(), serde_json::Value::Array(paths.into_iter().map(serde_json::Value::String).collect()));
    }
    to_js_value(&mutations)
}

#[wasm_bindgen]
pub fn bokheim_library_list() -> Result<JsValue, JsValue> {
    DATABASES.with(|slot| {
        let mut borrowed = slot.borrow_mut();
        let databases = borrowed.as_mut().ok_or_else(|| js_error("The Rust browser database is not open"))?;
        let count = databases.global.query_row("SELECT count(*) FROM rust_library_registry", [], |row| row.get::<_, i64>(0)).map_err(js_error)?;
        if count == 0 {
            databases
                .global
                .execute("INSERT INTO rust_library_registry(library_id,library_name,created_at) VALUES(?1,'My Library',?2)", rusqlite::params![uuid::Uuid::new_v4().to_string(), unix_millis().map_err(js_error)?])
                .map_err(js_error)?;
        }
        Ok::<(), JsValue>(())
    })?;
    let entries = registered_library_entries()?;
    serde_wasm_bindgen::to_value(&entries).map_err(js_error)
}

fn validate_library_name(name: &str) -> Result<String, JsValue> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 255 || name.chars().any(char::is_control) {
        return Err(js_error("Library names must contain between 1 and 255 printable characters"));
    }
    Ok(name.to_owned())
}

#[wasm_bindgen]
pub fn bokheim_library_create(name: &str) -> Result<JsValue, JsValue> {
    let name = validate_library_name(name)?;
    let library_id = uuid::Uuid::new_v4().to_string();
    DATABASES.with(|slot| {
        let borrowed = slot.borrow();
        let global = &borrowed.as_ref().ok_or_else(|| js_error("The Rust browser database is not open"))?.global;
        global.execute("INSERT INTO rust_library_registry(library_id,library_name,created_at) VALUES(?1,?2,?3)", rusqlite::params![library_id, name, unix_millis().map_err(js_error)?]).map_err(js_error)?;
        serde_wasm_bindgen::to_value(&library_entry(library_id, name)).map_err(js_error)
    })
}

#[wasm_bindgen]
pub fn bokheim_library_attach(library_id: &str, name: &str, owner_user_id: Option<String>) -> Result<JsValue, JsValue> {
    let library_id = parse_library_id(library_id).map_err(js_error)?;
    let name = validate_library_name(name)?;
    DATABASES.with(|slot| {
        let borrowed = slot.borrow();
        let global = &borrowed.as_ref().ok_or_else(|| js_error("The Rust browser database is not open"))?.global;
        global
            .execute(
                "INSERT INTO rust_library_registry(library_id,library_name,created_at,remote,owner_user_id) VALUES(?1,?2,?3,1,?4) ON CONFLICT(library_id) DO UPDATE SET library_name=excluded.library_name,remote=1,owner_user_id=COALESCE(excluded.owner_user_id,rust_library_registry.owner_user_id)",
                rusqlite::params![library_id, name, unix_millis().map_err(js_error)?, owner_user_id],
            )
            .map_err(js_error)?;
        serde_wasm_bindgen::to_value(&library_entry(library_id, name)).map_err(js_error)
    })
}

#[wasm_bindgen]
pub fn bokheim_library_claim(library_id: &str, owner_user_id: &str) -> Result<(), JsValue> {
    let library_id = parse_library_id(library_id).map_err(js_error)?;
    if owner_user_id.trim().is_empty() {
        return Err(js_error("The account identity is empty"));
    }
    DATABASES.with(|slot| {
        let borrowed = slot.borrow();
        let global = &borrowed.as_ref().ok_or_else(|| js_error("The Rust browser database is not open"))?.global;
        let existing: Option<String> = global.query_row("SELECT owner_user_id FROM rust_library_registry WHERE library_id=?1", [&library_id], |row| row.get(0)).map_err(js_error)?;
        if existing.as_deref().is_some_and(|owner| owner != owner_user_id) {
            return Err(js_error("The library belongs to a different account"));
        }
        global.execute("UPDATE rust_library_registry SET owner_user_id=?2 WHERE library_id=?1", rusqlite::params![library_id, owner_user_id]).map_err(js_error)?;
        Ok::<(), JsValue>(())
    })
}

fn remote_library_ids() -> Result<Vec<String>, JsValue> {
    DATABASES.with(|slot| {
        let borrowed = slot.borrow();
        let global = &borrowed.as_ref().ok_or_else(|| js_error("The Rust browser database is not open"))?.global;
        let mut statement = global.prepare("SELECT library_id FROM rust_library_registry WHERE remote=1 ORDER BY library_id").map_err(js_error)?;
        statement.query_map([], |row| row.get::<_, String>(0)).map_err(js_error)?.collect::<Result<Vec<_>, _>>().map_err(js_error)
    })
}

#[wasm_bindgen]
pub fn bokheim_library_remote_ids() -> Result<JsValue, JsValue> {
    serde_wasm_bindgen::to_value(&remote_library_ids()?).map_err(js_error)
}

#[wasm_bindgen]
pub fn bokheim_library_book_count(library_id: &str) -> Result<usize, JsValue> {
    with_library(library_id, |connection| {
        let count = connection.query_row("SELECT count(*) FROM book WHERE deleted_at IS NULL AND purged_at IS NULL", [], |row| row.get::<_, i64>(0)).map_err(|error| error.to_string())?;
        usize::try_from(count).map_err(|_| "The library book count is outside its supported range".to_owned())
    })
}

#[wasm_bindgen]
pub fn bokheim_library_sync_details(library_id: &str) -> Result<JsValue, JsValue> {
    with_library(library_id, |connection| {
        let details = library_sqlite::get_library_sync_details(connection).map_err(|error| error.to_string())?;
        serde_wasm_bindgen::to_value(&serde_json::json!({
            "pending": usize::from(details.has_unsynced_changes),
            "lastSyncedAt": details.last_synced_at,
        }))
        .map_err(|error| error.to_string())
    })
}

#[wasm_bindgen]
pub fn bokheim_library_remove(library_id: &str) -> Result<(), JsValue> {
    let library_id = parse_library_id(library_id).map_err(js_error)?;
    DATABASES.with(|slot| {
        let mut borrowed = slot.borrow_mut();
        let databases = borrowed.as_mut().ok_or_else(|| js_error("The Rust browser database is not open"))?;
        let removed = databases.global.execute("DELETE FROM rust_library_registry WHERE library_id=?1", [&library_id]).map_err(js_error)?;
        if removed == 0 {
            return Err(js_error("The library does not exist"));
        }
        databases.libraries.remove(&library_id);
        Ok::<(), JsValue>(())
    })
}

fn parse_file_type(value: &str) -> Result<app_contract::library::LibraryFileTypeFilter, String> {
    match value {
        "all" => Ok(app_contract::library::LibraryFileTypeFilter::All),
        "book" => Ok(app_contract::library::LibraryFileTypeFilter::Book),
        "pdf" => Ok(app_contract::library::LibraryFileTypeFilter::Pdf),
        "audiobook" => Ok(app_contract::library::LibraryFileTypeFilter::Audiobook),
        _ => Err("invalid library file type filter".to_owned()),
    }
}

#[wasm_bindgen]
pub fn bokheim_library_home(library_id: &str, limit: i32) -> Result<JsValue, JsValue> {
    with_library(library_id, |connection| {
        let home = library_sqlite::browse::library_home(connection, limit, false).map_err(|error| error.to_string())?;
        serde_wasm_bindgen::to_value(&home).map_err(|error| error.to_string())
    })
}

#[wasm_bindgen]
pub fn bokheim_library_folder(library_id: &str, directory_id: &str, file_type: &str) -> Result<JsValue, JsValue> {
    let file_type = parse_file_type(file_type).map_err(js_error)?;
    let directory_id = uuid::Uuid::parse_str(directory_id).map_err(js_error)?;
    with_library(library_id, |connection| {
        let contents = library_sqlite::browse::library_folder(connection, &directory_id, false, file_type).map_err(|error| error.to_string())?;
        serde_wasm_bindgen::to_value(&contents).map_err(|error| error.to_string())
    })
}

#[wasm_bindgen]
pub fn bokheim_library_authors(library_id: &str, author_id: Option<String>, sort: &str) -> Result<JsValue, JsValue> {
    let author_id = author_id.map(|value| uuid::Uuid::parse_str(&value)).transpose().map_err(js_error)?;
    let sort = match sort {
        "name" => app_contract::library::AuthorSort::Name,
        "book_count" => app_contract::library::AuthorSort::BookCount,
        _ => return Err(js_error("invalid author sort")),
    };
    with_library(library_id, |connection| {
        let view = library_sqlite::browse::library_authors(connection, author_id, sort, false).map_err(|error| error.to_string())?;
        serde_wasm_bindgen::to_value(&view).map_err(|error| error.to_string())
    })
}

#[wasm_bindgen]
pub fn bokheim_library_search(library_id: &str, query: &str) -> Result<JsValue, JsValue> {
    serde_wasm_bindgen::to_value(&library_search_result(library_id, query)?).map_err(js_error)
}

#[wasm_bindgen]
pub fn bokheim_library_search_folder(library_id: &str, directory_id: &str, query: &str, file_type: &str) -> Result<JsValue, JsValue> {
    serde_wasm_bindgen::to_value(&library_search_folder_result(library_id, directory_id, query, file_type)?).map_err(js_error)
}

fn library_search_result(library_id: &str, query: &str) -> Result<app_contract::library::LibrarySearchResult, JsValue> {
    with_library(library_id, |connection| library_sqlite::browse::library_search(connection, query, false).map_err(|error| error.to_string()))
}

fn library_search_folder_result(library_id: &str, directory_id: &str, query: &str, file_type: &str) -> Result<app_contract::library::LibrarySearchResult, JsValue> {
    let directory_id = uuid::Uuid::parse_str(directory_id).map_err(js_error)?;
    let file_type = parse_file_type(file_type).map_err(js_error)?;
    with_library(library_id, |connection| library_sqlite::browse::library_search_folder(connection, &directory_id, query, file_type, false).map_err(|error| error.to_string()))
}

fn limit_library_search_result(mut result: app_contract::library::LibrarySearchResult, limit: usize) -> Result<JsValue, JsValue> {
    result.title_matches.truncate(limit);
    result.author_matches.truncate(limit.saturating_sub(result.title_matches.len()));
    serde_wasm_bindgen::to_value(&result).map_err(js_error)
}

#[wasm_bindgen]
pub fn bokheim_library_subjects(library_id: &str, subject_path: Option<String>, file_type: &str) -> Result<JsValue, JsValue> {
    let file_type = parse_file_type(file_type).map_err(js_error)?;
    with_library(library_id, |connection| {
        let view = library_sqlite::browse::library_subjects(connection, subject_path.as_deref(), file_type, false).map_err(|error| error.to_string())?;
        serde_wasm_bindgen::to_value(&view).map_err(|error| error.to_string())
    })
}

#[wasm_bindgen]
pub fn bokheim_library_search_subject(library_id: &str, subject_path: Option<String>, query: &str, limit: usize, file_type: &str) -> Result<JsValue, JsValue> {
    let file_type = parse_file_type(file_type).map_err(js_error)?;
    with_library(library_id, |connection| {
        let mut result = library_sqlite::browse::library_search_subject(connection, subject_path.as_deref(), query, file_type, false).map_err(|error| error.to_string())?;
        result.title_matches.truncate(limit);
        result.author_matches.truncate(limit.saturating_sub(result.title_matches.len()));
        serde_wasm_bindgen::to_value(&result).map_err(|error| error.to_string())
    })
}

#[wasm_bindgen]
pub fn bokheim_library_owned_catalog_publications(library_id: &str) -> Result<JsValue, JsValue> {
    with_library(library_id, |connection| {
        let publications = library_sqlite::catalog::all_owned_catalog_publications(connection).map_err(|error| error.to_string())?;
        let serializer = serde_wasm_bindgen::Serializer::new().serialize_missing_as_null(true);
        publications.serialize(&serializer).map_err(|error| error.to_string())
    })
}

#[wasm_bindgen]
pub fn bokheim_library_audiobook_chapters(library_id: &str, content_hash: &str) -> Result<JsValue, JsValue> {
    let content_hash = sync_common::ContentHash::new(content_hash);
    with_library(library_id, |connection| {
        let chapters = library_sqlite::audiobook_chapters(connection, &content_hash).map_err(|error| error.to_string())?;
        serde_wasm_bindgen::to_value(&chapters).map_err(|error| error.to_string())
    })
}

#[wasm_bindgen]
pub fn bokheim_library_annotations(library_id: &str, content_hash: &str) -> Result<JsValue, JsValue> {
    let content_hash = sync_common::ContentHash::new(content_hash);
    with_library(library_id, |connection| {
        let annotations = library_sqlite::marks::load_annotations(connection, content_hash).map_err(|error| error.to_string())?;
        serde_wasm_bindgen::to_value(&annotations).map_err(|error| error.to_string())
    })
}

#[wasm_bindgen]
pub fn bokheim_library_store_annotation(library_id: &str, annotation: JsValue) -> Result<(), JsValue> {
    let annotation: app_contract::reader_state::ReaderAnnotation = serde_wasm_bindgen::from_value(annotation).map_err(js_error)?;
    with_library(library_id, |connection| library_sqlite::marks::store_annotation(connection, &annotation).map_err(|error| error.to_string()))
}

#[wasm_bindgen]
pub fn bokheim_library_delete_annotation(library_id: &str, annotation_id: &str, modified_at: Option<f64>) -> Result<(), JsValue> {
    let modified_at = optional_js_millis(modified_at).map_err(js_error)?;
    with_library(library_id, |connection| library_sqlite::marks::remove_annotation(connection, annotation_id, modified_at).map_err(|error| error.to_string()))
}

#[wasm_bindgen]
pub fn bokheim_library_bookmarks(library_id: &str, content_hash: &str) -> Result<JsValue, JsValue> {
    let content_hash = sync_common::ContentHash::new(content_hash);
    with_library(library_id, |connection| {
        let bookmarks = library_sqlite::marks::load_bookmarks(connection, content_hash).map_err(|error| error.to_string())?;
        serde_wasm_bindgen::to_value(&bookmarks).map_err(|error| error.to_string())
    })
}

#[wasm_bindgen]
pub fn bokheim_library_create_bookmark(library_id: &str, bookmark: JsValue) -> Result<(), JsValue> {
    let bookmark: app_contract::reader_state::ReaderBookmark = serde_wasm_bindgen::from_value(bookmark).map_err(js_error)?;
    with_library(library_id, |connection| library_sqlite::marks::insert_bookmark(connection, &bookmark).map_err(|error| error.to_string()))
}

#[wasm_bindgen]
pub fn bokheim_library_delete_bookmark(library_id: &str, bookmark_id: &str, deleted_at: Option<f64>) -> Result<(), JsValue> {
    let deleted_at = optional_js_millis(deleted_at).map_err(js_error)?;
    with_library(library_id, |connection| library_sqlite::marks::remove_bookmark(connection, bookmark_id, deleted_at).map_err(|error| error.to_string()))
}

#[wasm_bindgen]
pub fn bokheim_library_child_directories(library_id: &str, parent_id: &str) -> Result<JsValue, JsValue> {
    let parent_id = uuid::Uuid::parse_str(parent_id).map_err(js_error)?.to_string();
    with_library(library_id, |connection| {
        let mut directories = library_sqlite::browse::get_child_directories(connection, &parent_id).map_err(|error| error.to_string())?;
        app_contract::library::FolderSort::Name.sort(&mut directories);
        serde_wasm_bindgen::to_value(&directories).map_err(|error| error.to_string())
    })
}

#[wasm_bindgen]
pub fn bokheim_library_directory_path(library_id: &str, directory_id: &str) -> Result<JsValue, JsValue> {
    let directory_id = uuid::Uuid::parse_str(directory_id).map_err(js_error)?.to_string();
    with_library(library_id, |connection| {
        let directories =
            library_sqlite::assets::get_dir_path_components(connection, &directory_id).map_err(|error| error.to_string())?.into_iter().map(|component| app_contract::library::Dir::new(component.id, component.name, 0, 0)).collect::<Vec<_>>();
        serde_wasm_bindgen::to_value(&directories).map_err(|error| error.to_string())
    })
}

#[wasm_bindgen]
pub fn bokheim_library_folder_destinations(library_id: &str) -> Result<JsValue, JsValue> {
    with_library(library_id, |connection| {
        let destinations = library_sqlite::folder_destinations(connection).map_err(|error| error.to_string())?;
        serde_wasm_bindgen::to_value(&destinations).map_err(|error| error.to_string())
    })
}

fn validate_file_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 255 || name.contains(['/', '\\']) || name.chars().any(char::is_control) {
        return Err("File and directory names must contain between 1 and 255 printable path-safe characters".to_owned());
    }
    Ok(name.to_owned())
}

#[wasm_bindgen]
pub fn bokheim_library_create_directory(library_id: &str, parent_id: &str, name: &str) -> Result<JsValue, JsValue> {
    let parent_id = uuid::Uuid::parse_str(parent_id).map_err(js_error)?;
    let name = validate_file_name(name).map_err(js_error)?;
    with_library_mut(library_id, |connection| {
        let transaction = connection.transaction().map_err(|error| error.to_string())?;
        let directory = library_sqlite::create_directory_record(&transaction, &parent_id, &name).map_err(|error| error.to_string())?;
        transaction.commit().map_err(|error| error.to_string())?;
        serde_wasm_bindgen::to_value(&directory).map_err(|error| error.to_string())
    })
}

#[wasm_bindgen]
pub fn bokheim_library_rename_directory(library_id: &str, directory_id: &str, name: &str) -> Result<String, JsValue> {
    let directory_id = uuid::Uuid::parse_str(directory_id).map_err(js_error)?;
    let name = validate_file_name(name).map_err(js_error)?;
    with_library_mut(library_id, |connection| {
        let transaction = connection.transaction().map_err(|error| error.to_string())?;
        let chosen = library_sqlite::move_directory_record(&transaction, &directory_id, None, Some(&name)).map_err(|error| error.to_string())?;
        transaction.commit().map_err(|error| error.to_string())?;
        Ok(chosen)
    })
}

#[wasm_bindgen]
pub fn bokheim_library_move_directory(library_id: &str, directory_id: &str, destination_id: &str) -> Result<String, JsValue> {
    let directory_id = uuid::Uuid::parse_str(directory_id).map_err(js_error)?;
    let destination_id = uuid::Uuid::parse_str(destination_id).map_err(js_error)?;
    with_library_mut(library_id, |connection| {
        let transaction = connection.transaction().map_err(|error| error.to_string())?;
        let chosen = library_sqlite::move_directory_record(&transaction, &directory_id, Some(&destination_id), None).map_err(|error| error.to_string())?;
        transaction.commit().map_err(|error| error.to_string())?;
        Ok(chosen)
    })
}

#[wasm_bindgen]
pub fn bokheim_library_transfer_book(library_id: &str, content_hash: &str, source_id: &str, destination_id: &str, remove_source: bool) -> Result<String, JsValue> {
    let source_id = uuid::Uuid::parse_str(source_id).map_err(js_error)?;
    let destination_id = uuid::Uuid::parse_str(destination_id).map_err(js_error)?;
    let content_hash = sync_common::ContentHash::new(content_hash);
    with_library_mut(library_id, |connection| {
        let transaction = connection.transaction().map_err(|error| error.to_string())?;
        let chosen = library_sqlite::transfer_book_placement_record(&transaction, &content_hash, &source_id, &destination_id, remove_source).map_err(|error| error.to_string())?;
        transaction.commit().map_err(|error| error.to_string())?;
        Ok(chosen)
    })
}

#[wasm_bindgen]
pub fn bokheim_library_remove_book_placement(library_id: &str, content_hash: &str, source_id: &str) -> Result<bool, JsValue> {
    let source_id = uuid::Uuid::parse_str(source_id).map_err(js_error)?;
    let content_hash = sync_common::ContentHash::new(content_hash);
    with_library_mut(library_id, |connection| {
        let transaction = connection.transaction().map_err(|error| error.to_string())?;
        let orphaned = library_sqlite::remove_book_placement_record(&transaction, &content_hash, &source_id).map_err(|error| error.to_string())?;
        transaction.commit().map_err(|error| error.to_string())?;
        Ok(orphaned)
    })
}

#[wasm_bindgen]
pub fn bokheim_library_trash(library_id: &str) -> Result<JsValue, JsValue> {
    with_library(library_id, |connection| {
        let trash = library_sqlite::library_trash(connection).map_err(|error| error.to_string())?;
        serde_wasm_bindgen::to_value(&trash).map_err(|error| error.to_string())
    })
}

#[wasm_bindgen]
pub fn bokheim_library_trash_book(library_id: &str, content_hash: &str) -> Result<(), JsValue> {
    let content_hash = sync_common::ContentHash::new(content_hash);
    with_library(library_id, |connection| library_sqlite::trash_book_record(connection, &content_hash).map_err(|error| error.to_string()))
}

#[wasm_bindgen]
pub fn bokheim_library_restore_book(library_id: &str, content_hash: &str) -> Result<(), JsValue> {
    let content_hash = sync_common::ContentHash::new(content_hash);
    with_library(library_id, |connection| library_sqlite::restore_book_record(connection, &content_hash).map_err(|error| error.to_string()))
}

#[wasm_bindgen]
pub fn bokheim_library_trash_directory(library_id: &str, directory_id: &str) -> Result<(), JsValue> {
    let directory_id = uuid::Uuid::parse_str(directory_id).map_err(js_error)?;
    with_library_mut(library_id, |connection| {
        let transaction = connection.transaction().map_err(|error| error.to_string())?;
        library_sqlite::trash_directory_record(&transaction, &directory_id).map_err(|error| error.to_string())?;
        transaction.commit().map_err(|error| error.to_string())
    })
}

#[wasm_bindgen]
pub fn bokheim_library_restore_directory(library_id: &str, directory_id: &str) -> Result<String, JsValue> {
    let directory_id = uuid::Uuid::parse_str(directory_id).map_err(js_error)?;
    with_library_mut(library_id, |connection| {
        let transaction = connection.transaction().map_err(|error| error.to_string())?;
        let chosen = library_sqlite::restore_directory_record(&transaction, &directory_id).map_err(|error| error.to_string())?;
        transaction.commit().map_err(|error| error.to_string())?;
        Ok(chosen)
    })
}

#[wasm_bindgen]
pub fn bokheim_library_purge_directory(library_id: &str, directory_id: &str) -> Result<(), JsValue> {
    let directory_id = uuid::Uuid::parse_str(directory_id).map_err(js_error)?;
    with_library_mut(library_id, |connection| {
        let transaction = connection.transaction().map_err(|error| error.to_string())?;
        library_sqlite::purge_directory_record(&transaction, &directory_id).map_err(|error| error.to_string())?;
        transaction.commit().map_err(|error| error.to_string())
    })
}

#[wasm_bindgen]
pub fn bokheim_library_purge_book(library_id: &str, content_hash: &str) -> Result<(), JsValue> {
    let content_hash = sync_common::ContentHash::new(content_hash);
    with_library_mut(library_id, |connection| {
        let transaction = connection.transaction().map_err(|error| error.to_string())?;
        library_sqlite::purge_book_record(&transaction, &content_hash).map_err(|error| error.to_string())?;
        transaction.execute("DELETE FROM browser_publication_asset WHERE content_hash=?1", [content_hash.as_str()]).map_err(|error| error.to_string())?;
        transaction.commit().map_err(|error| error.to_string())
    })
}

#[wasm_bindgen]
pub fn bokheim_library_empty_trash(library_id: &str) -> Result<(), JsValue> {
    with_library_mut(library_id, |connection| {
        let transaction = connection.transaction().map_err(|error| error.to_string())?;
        let content_hashes = library_sqlite::empty_trash_records(&transaction).map_err(|error| error.to_string())?;
        for content_hash in &content_hashes {
            transaction.execute("DELETE FROM browser_publication_asset WHERE content_hash=?1", [content_hash.as_str()]).map_err(|error| error.to_string())?;
        }
        transaction.commit().map_err(|error| error.to_string())
    })
}

#[wasm_bindgen]
pub fn bokheim_library_reading_position(library_id: &str, content_hash: &str) -> Result<Option<String>, JsValue> {
    with_library(library_id, |connection| {
        library_sqlite::reading::get_stored_reading_position(connection, sync_common::ContentHash::new(content_hash)).map(|position| position.map(|position| position.as_str())).map_err(|error| error.to_string())
    })
}

#[wasm_bindgen]
pub fn bokheim_library_pdf_reading_position(library_id: &str, content_hash: &str) -> Result<JsValue, JsValue> {
    let value = bokheim_library_reading_position(library_id, content_hash)?;
    let position = value.as_deref().and_then(|value| library_model::ReadingPosition::parse(value).ok()).and_then(|position| match position {
        library_model::ReadingPosition::Pdf(page) => Some((page, None)),
        library_model::ReadingPosition::PdfAtPoint { page, full_page_position } => Some((page, Some(full_page_position))),
        _ => None,
    });
    serde_wasm_bindgen::to_value(&position).map_err(js_error)
}

#[wasm_bindgen]
pub fn bokheim_library_update_reading_position(library_id: &str, content_hash: &str, position: &str, progress: Option<f32>) -> Result<(), JsValue> {
    let progress = progress.map(library_model::ReadProgress::new).transpose().map_err(js_error)?;
    with_library(library_id, |connection| library_sqlite::update_reading_position(connection, sync_common::ContentHash::new(content_hash), position, progress).map_err(|error| error.to_string()))
}

#[wasm_bindgen]
pub fn bokheim_library_import(library_id: &str, directory_id: &str, file_name: &str, bytes: &[u8], rendered_cover: &[u8]) -> Result<JsValue, JsValue> {
    if bytes.is_empty() {
        return Err(js_error("The selected publication is empty"));
    }
    let directory_id = uuid::Uuid::parse_str(directory_id).map_err(js_error)?;
    let file_name = validate_file_name(file_name).map_err(js_error)?;
    let source_name = Path::new(&file_name);
    let extension = source_name.extension().and_then(|value| value.to_str()).unwrap_or_default();
    let format = library_model::BookFormat::from_extension(extension).ok_or_else(|| js_error("The web library currently accepts EPUB and PDF files"))?;
    let content_hash = sync_common::ContentHash::new(&blake3::hash(bytes).to_hex().to_string());
    let (metadata, toc, cover) = match format {
        library_model::BookFormat::Epub => {
            let inspected = book_metadata::inspect_epub_reader(source_name, Cursor::new(bytes.to_vec())).map_err(js_error)?;
            let provider = epub_provider::EpubProvider::try_from_reader(Cursor::new(bytes.to_vec())).map_err(js_error)?;
            let cover = provider
                .cover_image_path()
                .map_err(js_error)?
                .and_then(|path| provider.read_bytes(&path).ok())
                .filter(|cover| cover.len() <= 20 * 1024 * 1024)
                .and_then(|cover| thumbnail::generate_thumbnail_from_image_bytes(&cover).ok())
                .unwrap_or_default();
            (inspected.metadata, inspected.toc, cover)
        }
        library_model::BookFormat::Pdf => {
            if !bytes.starts_with(b"%PDF-") {
                return Err(js_error("The selected file is not a valid PDF document"));
            }
            let title = source_name.file_stem().and_then(|value| value.to_str()).filter(|value| !value.trim().is_empty()).unwrap_or("Untitled").to_owned();
            (library_model::BookMetadataState { title, contributors: Vec::new(), description: String::new(), publication: Default::default() }, Vec::new(), Vec::new())
        }
        _ => return Err(js_error("The web library currently accepts EPUB and PDF files")),
    };
    with_library_mut(library_id, |connection| {
        if !library_sqlite::directory_exists(connection, &directory_id.to_string()).map_err(|error| error.to_string())? {
            return Err("The destination directory does not exist".to_owned());
        }
        let transaction = connection.transaction().map_err(|error| error.to_string())?;
        library_sqlite::insert_scanned_book_metadata(&transaction, content_hash.clone(), &metadata, format).map_err(|error| error.to_string())?;
        library_sqlite::insert_local_book_dir(&transaction, &directory_id, content_hash.clone(), &file_name, 0, unix_millis()?).map_err(|error| error.to_string())?;
        let cover = if rendered_cover.is_empty() { cover.as_slice() } else { rendered_cover };
        transaction
            .execute(
                "INSERT INTO browser_publication_asset(content_hash,publication_bytes,cover_bytes,toc_json) VALUES(?1,?2,?3,?4) ON CONFLICT(content_hash) DO UPDATE SET publication_bytes=excluded.publication_bytes,cover_bytes=excluded.cover_bytes,toc_json=excluded.toc_json",
                rusqlite::params![content_hash.as_str(), bytes, (!cover.is_empty()).then_some(cover), serde_json::to_string(&toc).map_err(|error| error.to_string())?],
            )
            .map_err(|error| error.to_string())?;
        transaction.commit().map_err(|error| error.to_string())?;
        serde_wasm_bindgen::to_value(&ImportedPublication { content_hash: content_hash.to_string() }).map_err(|error| error.to_string())
    })
}

#[wasm_bindgen]
pub fn bokheim_library_book_detail(library_id: &str, content_hash: &str) -> Result<JsValue, JsValue> {
    with_library(library_id, |connection| {
        let mut detail = library_sqlite::browse::book_detail_summary(connection, sync_common::ContentHash::new(content_hash)).map_err(|error| error.to_string())?;
        let toc = connection.query_row("SELECT toc_json FROM browser_publication_asset WHERE content_hash=?1", [content_hash], |row| row.get::<_, String>(0)).ok().and_then(|json| serde_json::from_str(&json).ok()).unwrap_or_default();
        detail.toc = toc;
        serde_wasm_bindgen::to_value(&detail).map_err(|error| error.to_string())
    })
}

#[wasm_bindgen]
pub fn bokheim_library_record_catalog_publication(library_id: &str, content_hash: &str, request: JsValue) -> Result<(), JsValue> {
    let request: catalog_contract::CatalogImportRequest = serde_wasm_bindgen::from_value(request).map_err(js_error)?;
    with_library_mut(library_id, |connection| {
        let transaction = connection.transaction().map_err(|error| error.to_string())?;
        library_sqlite::catalog::record_catalog_publication(&transaction, &request.source_id, &request.publication, &sync_common::ContentHash::new(content_hash)).map_err(|error| error.to_string())?;
        transaction.commit().map_err(|error| error.to_string())
    })
}

#[wasm_bindgen]
pub fn bokheim_library_import_catalog(library_id: &str, directory_id: &str, file_name: &str, bytes: &[u8], rendered_cover: &[u8], request: JsValue) -> Result<String, JsValue> {
    let imported: ImportedPublication = serde_wasm_bindgen::from_value(bokheim_library_import(library_id, directory_id, file_name, bytes, rendered_cover)?).map_err(js_error)?;
    bokheim_library_record_catalog_publication(library_id, &imported.content_hash, request)?;
    Ok(imported.content_hash)
}

#[wasm_bindgen]
pub fn bokheim_library_publication_asset(library_id: &str, content_hash: &str) -> Result<JsValue, JsValue> {
    with_library(library_id, |connection| {
        let asset = connection
            .query_row(
                "SELECT placement.file_name,asset.publication_bytes FROM browser_publication_asset asset JOIN book ON book.content_hash=asset.content_hash JOIN book_dir placement ON placement.book_row_id=book.row_id AND placement.deleted_at IS NULL WHERE asset.content_hash=?1 ORDER BY placement.dir_id LIMIT 1",
                [content_hash],
                |row| {
                    let file_name: String = row.get(0)?;
                    Ok(PublicationAsset { media_type: publication_media_type(&file_name), file_name, publication_bytes: row.get(1)? })
                },
            )
            .map_err(|error| if matches!(error, rusqlite::Error::QueryReturnedNoRows) { "The publication asset is not stored locally".to_owned() } else { error.to_string() })?;
        serde_wasm_bindgen::to_value(&asset).map_err(|error| error.to_string())
    })
}

#[wasm_bindgen]
pub fn bokheim_library_thumbnail(library_id: &str, content_hash: &str) -> Result<Vec<u8>, JsValue> {
    with_library(library_id, |connection| {
        connection
            .query_row("SELECT COALESCE(cover_bytes,X'') FROM browser_publication_asset WHERE content_hash=?1", [content_hash], |row| row.get(0))
            .map_err(|error| if matches!(error, rusqlite::Error::QueryReturnedNoRows) { "The book does not exist".to_owned() } else { error.to_string() })
    })
}

#[wasm_bindgen]
pub fn bokheim_library_file_name(library_id: &str, content_hash: &str) -> Result<String, JsValue> {
    with_library(library_id, |connection| {
        connection
            .query_row(
                "SELECT placement.file_name FROM book JOIN book_dir placement ON placement.book_row_id=book.row_id AND placement.deleted_at IS NULL WHERE book.content_hash=?1 ORDER BY placement.dir_id LIMIT 1",
                [content_hash],
                |row| row.get(0),
            )
            .map_err(|error| if matches!(error, rusqlite::Error::QueryReturnedNoRows) { "The book does not exist".to_owned() } else { error.to_string() })
    })
}

async fn post_authenticated_wire(path: &str, body: &[u8]) -> Result<Vec<u8>, JsValue> {
    authenticated_wire(reqwest::Method::POST, path, Some(body)).await
}

async fn authenticated_wire(method: reqwest::Method, path: &str, body: Option<&[u8]>) -> Result<Vec<u8>, JsValue> {
    async fn send(method: reqwest::Method, path: &str, body: Option<&[u8]>) -> Result<reqwest::Response, reqwest::Error> {
        let mut request = reqwest::Client::new().request(method, api_url(path)).header(reqwest::header::ACCEPT, wire::MEDIA_TYPE).fetch_credentials_same_origin();
        if let Some(body) = body {
            request = request.header(reqwest::header::CONTENT_TYPE, wire::MEDIA_TYPE).body(body.to_vec());
        }
        request.send().await
    }

    let mut response = send(method.clone(), path, body).await.map_err(js_error)?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        let _ = response.bytes().await;
        let refresh = reqwest::Client::new().post(api_url("/api/auth/web/refresh")).header(reqwest::header::ACCEPT, wire::MEDIA_TYPE).fetch_credentials_same_origin().send().await.map_err(js_error)?;
        if !refresh.status().is_success() {
            return Err(js_error("Sign in to synchronize this library"));
        }
        response = send(method, path, body).await.map_err(js_error)?;
    }
    if !response.status().is_success() {
        let status = response.status();
        let detail = response.text().await.unwrap_or_default();
        return Err(js_error(if detail.trim().is_empty() { format!("Synchronization server returned {status}") } else { detail }));
    }
    response.bytes().await.map(|bytes| bytes.to_vec()).map_err(js_error)
}

async fn authenticated_asset_request(method: reqwest::Method, path: &str, content_type: Option<&str>, body: Option<&[u8]>) -> Result<reqwest::Response, JsValue> {
    async fn send(method: reqwest::Method, path: &str, content_type: Option<&str>, body: Option<&[u8]>) -> Result<reqwest::Response, reqwest::Error> {
        let mut request = reqwest::Client::new().request(method, api_url(path)).fetch_credentials_same_origin();
        if let Some(content_type) = content_type {
            request = request.header(reqwest::header::CONTENT_TYPE, content_type);
        }
        if let Some(body) = body {
            request = request.body(body.to_vec());
        }
        request.send().await
    }

    let mut response = send(method.clone(), path, content_type, body).await.map_err(js_error)?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        let _ = response.bytes().await;
        let refresh = reqwest::Client::new().post(api_url("/api/auth/web/refresh")).header(reqwest::header::ACCEPT, wire::MEDIA_TYPE).fetch_credentials_same_origin().send().await.map_err(js_error)?;
        if !refresh.status().is_success() {
            return Err(js_error("Sign in to synchronize this library"));
        }
        response = send(method, path, content_type, body).await.map_err(js_error)?;
    }
    Ok(response)
}

fn set_asset_transfer(library_id: &str, content_hash: &sync_common::ContentHash, kind: &str, direction: &str, state: &str, last_error: Option<&str>) -> Result<(), JsValue> {
    with_library(library_id, |connection| {
        connection
            .execute(
                "INSERT INTO browser_asset_transfer(content_hash,kind,direction,state,attempts,last_error,updated_at) VALUES(?1,?2,?3,?4,CASE WHEN ?4='running' THEN 1 ELSE 0 END,?5,?6)
                 ON CONFLICT(content_hash,kind,direction) DO UPDATE SET state=excluded.state,attempts=browser_asset_transfer.attempts+CASE WHEN excluded.state='running' THEN 1 ELSE 0 END,last_error=excluded.last_error,updated_at=excluded.updated_at",
                rusqlite::params![content_hash.as_str(), kind, direction, state, last_error, unix_millis()?],
            )
            .map_err(|error| error.to_string())?;
        Ok(())
    })
}

fn clear_asset_transfer(library_id: &str, content_hash: &sync_common::ContentHash, kind: &str, direction: &str) -> Result<(), JsValue> {
    with_library(library_id, |connection| {
        connection.execute("DELETE FROM browser_asset_transfer WHERE content_hash=?1 AND kind=?2 AND direction=?3", rusqlite::params![content_hash.as_str(), kind, direction]).map_err(|error| error.to_string())?;
        Ok(())
    })
}

fn local_publication_assets(library_id: &str) -> Result<Vec<LocalPublicationAsset>, JsValue> {
    with_library(library_id, |connection| {
        let mut statement = connection
            .prepare(
                "SELECT asset.content_hash,asset.publication_bytes,COALESCE(asset.cover_bytes,X''),
                        EXISTS(SELECT 1 FROM browser_asset_transfer transfer WHERE transfer.content_hash=asset.content_hash AND transfer.kind='thumbnail' AND transfer.direction='upload' AND transfer.state='completed'),
                        EXISTS(SELECT 1 FROM browser_asset_transfer transfer WHERE transfer.content_hash=asset.content_hash AND transfer.kind='thumbnail' AND transfer.direction='download' AND transfer.state='completed')
                 FROM browser_publication_asset asset JOIN book ON book.content_hash=asset.content_hash
                 WHERE book.deleted_at IS NULL ORDER BY asset.content_hash",
            )
            .map_err(|error| error.to_string())?;
        statement
            .query_map([], |row| {
                Ok(LocalPublicationAsset {
                    content_hash: sync_common::ContentHash::new(&row.get::<_, String>(0)?),
                    publication_bytes: row.get(1)?,
                    cover_bytes: row.get(2)?,
                    thumbnail_uploaded: row.get(3)?,
                    thumbnail_downloaded: row.get(4)?,
                })
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())
    })
}

#[wasm_bindgen]
pub async fn bokheim_library_sync_assets(library_id: &str) -> Result<(), JsValue> {
    if stored_account_profile()?.is_none() {
        return Err(js_error("Sign in to synchronize this library"));
    }
    let assets = local_publication_assets(library_id)?;
    if assets.is_empty() {
        return Ok(());
    }

    let manifest = sync_common::api::assets::BlobManifestRequest {
        blobs: assets.iter().map(|asset| sync_common::api::assets::BlobManifestEntry { content_hash: asset.content_hash.clone(), size_bytes: asset.publication_bytes.len() as u64 }).collect(),
    };
    let encoded = wire::encode(&manifest).map_err(js_error)?;
    let response = post_authenticated_wire(&format!("/api/libraries/{library_id}/blobs/negotiate"), &encoded).await?;
    let response: sync_common::api::assets::BlobManifestResponse = wire::decode(&response, wire::MAX_DECODED_RESPONSE_BYTES).map_err(js_error)?;
    sync_common::api::assets::validate_blob_manifest_response(&manifest, &response).map_err(js_error)?;
    if !response.rejected.is_empty() {
        return Err(js_error(format!("Server rejected publication transfer: {}", response.rejected.iter().map(|entry| format!("{} ({:?})", entry.content_hash, entry.reason)).collect::<Vec<_>>().join(", "))));
    }

    let uploads = response.upload.into_iter().collect::<std::collections::HashSet<_>>();
    for asset in &assets {
        if uploads.contains(&asset.content_hash) {
            set_asset_transfer(library_id, &asset.content_hash, "book", "upload", "running", None)?;
            let path = format!("/api/libraries/{library_id}/blobs/{}", asset.content_hash);
            let result = authenticated_asset_request(reqwest::Method::PUT, &path, Some("application/octet-stream"), Some(&asset.publication_bytes)).await;
            match result {
                Ok(response) if response.status().is_success() => set_asset_transfer(library_id, &asset.content_hash, "book", "upload", "completed", None)?,
                Ok(response) => {
                    let status = response.status();
                    let detail = response.text().await.unwrap_or_default();
                    let message = if detail.trim().is_empty() { format!("Publication upload returned {status}") } else { detail };
                    set_asset_transfer(library_id, &asset.content_hash, "book", "upload", "failed", Some(&message))?;
                    return Err(js_error(message));
                }
                Err(error) => {
                    let message = error.as_string().unwrap_or_else(|| "Publication upload failed".to_owned());
                    set_asset_transfer(library_id, &asset.content_hash, "book", "upload", "failed", Some(&message))?;
                    return Err(js_error(message));
                }
            }
        }

        let thumbnail_path = format!("/api/libraries/{library_id}/thumbnails/{}", asset.content_hash);
        if !asset.cover_bytes.is_empty() {
            if asset.thumbnail_uploaded || asset.thumbnail_downloaded {
                continue;
            }
            set_asset_transfer(library_id, &asset.content_hash, "thumbnail", "upload", "running", None)?;
            let response = match authenticated_asset_request(reqwest::Method::PUT, &thumbnail_path, Some("image/jpeg"), Some(&asset.cover_bytes)).await {
                Ok(response) => response,
                Err(error) => {
                    let message = error.as_string().unwrap_or_else(|| "Thumbnail upload failed".to_owned());
                    set_asset_transfer(library_id, &asset.content_hash, "thumbnail", "upload", "failed", Some(&message))?;
                    return Err(js_error(message));
                }
            };
            if !response.status().is_success() {
                let status = response.status();
                let message = response.text().await.unwrap_or_else(|_| format!("Thumbnail upload returned {status}"));
                set_asset_transfer(library_id, &asset.content_hash, "thumbnail", "upload", "failed", Some(&message))?;
                return Err(js_error(message));
            }
            set_asset_transfer(library_id, &asset.content_hash, "thumbnail", "upload", "completed", None)?;
        } else {
            set_asset_transfer(library_id, &asset.content_hash, "thumbnail", "download", "running", None)?;
            let response = match authenticated_asset_request(reqwest::Method::GET, &thumbnail_path, None, None).await {
                Ok(response) => response,
                Err(error) => {
                    let message = error.as_string().unwrap_or_else(|| "Thumbnail download failed".to_owned());
                    set_asset_transfer(library_id, &asset.content_hash, "thumbnail", "download", "failed", Some(&message))?;
                    return Err(js_error(message));
                }
            };
            if response.status() == reqwest::StatusCode::NOT_FOUND {
                let _ = response.bytes().await;
                clear_asset_transfer(library_id, &asset.content_hash, "thumbnail", "download")?;
                continue;
            }
            if !response.status().is_success() {
                let status = response.status();
                let message = format!("Thumbnail download returned {status}");
                set_asset_transfer(library_id, &asset.content_hash, "thumbnail", "download", "failed", Some(&message))?;
                return Err(js_error(message));
            }
            let bytes = match response.bytes().await {
                Ok(bytes) => bytes.to_vec(),
                Err(error) => {
                    let message = error.to_string();
                    set_asset_transfer(library_id, &asset.content_hash, "thumbnail", "download", "failed", Some(&message))?;
                    return Err(js_error(message));
                }
            };
            if bytes.is_empty() || bytes.len() as u64 > sync_common::MAX_THUMBNAIL_BYTES {
                let message = "The server returned an invalid thumbnail";
                set_asset_transfer(library_id, &asset.content_hash, "thumbnail", "download", "failed", Some(message))?;
                return Err(js_error(message));
            }
            if let Err(error) = with_library(library_id, |connection| {
                connection.execute("UPDATE browser_publication_asset SET cover_bytes=?2 WHERE content_hash=?1 AND cover_bytes IS NULL", rusqlite::params![asset.content_hash.as_str(), bytes]).map_err(|error| error.to_string())?;
                Ok(())
            }) {
                let message = error.as_string().unwrap_or_else(|| "Could not store the downloaded thumbnail".to_owned());
                set_asset_transfer(library_id, &asset.content_hash, "thumbnail", "download", "failed", Some(&message))?;
                return Err(js_error(message));
            }
            set_asset_transfer(library_id, &asset.content_hash, "thumbnail", "download", "completed", None)?;
        }
    }
    Ok(())
}

#[wasm_bindgen]
pub fn bokheim_library_local_storage_usage() -> Result<JsValue, JsValue> {
    let mut usage = Vec::new();
    for library in registered_library_entries()? {
        let used_bytes = with_library(&library.library_id, |connection| {
            connection
                .query_row("SELECT COALESCE(SUM(length(publication_bytes)+length(COALESCE(cover_bytes,X''))),0) FROM browser_publication_asset", [], |row| row.get::<_, i64>(0))
                .map(|value| value.max(0) as u64)
                .map_err(|error| error.to_string())
        })?;
        usage.push(BrowserStorageUsage { library_id: library.library_id, library_name: library.library_name, used_bytes });
    }
    serde_wasm_bindgen::to_value(&usage).map_err(js_error)
}

fn iso_millis(value: i64) -> String {
    js_sys::Date::new(&JsValue::from_f64(value as f64)).to_iso_string().as_string().unwrap_or_default()
}

#[wasm_bindgen]
pub fn bokheim_library_transfer_statuses() -> Result<JsValue, JsValue> {
    let mut libraries = Vec::new();
    for library in registered_library_entries()? {
        let statuses = with_library(&library.library_id, |connection| {
            let mut statement = connection.prepare("SELECT content_hash,kind,direction,state,attempts,last_error,updated_at FROM browser_asset_transfer ORDER BY updated_at DESC,content_hash").map_err(|error| error.to_string())?;
            statement
                .query_map([], |row| {
                    let content_hash: String = row.get(0)?;
                    let kind: String = row.get(1)?;
                    let direction: String = row.get(2)?;
                    let state: String = row.get(3)?;
                    let attempts = u32::try_from(row.get::<_, i64>(4)?).unwrap_or(u32::MAX);
                    let updated_at = iso_millis(row.get(6)?);
                    Ok(BrowserTransferStatus {
                        id: format!("web:{}:{kind}:{direction}:{content_hash}", library.library_id),
                        kind: format!("{direction}_{kind}"),
                        content_hash: content_hash.clone(),
                        book_content_hash: (kind == "thumbnail").then_some(content_hash),
                        path: None,
                        state: state.clone(),
                        origin: "background",
                        attempts,
                        completed_items: u32::from(state == "completed"),
                        total_items: 1,
                        last_error: row.get(5)?,
                        next_retry_at: None,
                        created_at: updated_at.clone(),
                        updated_at,
                    })
                })
                .map_err(|error| error.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| error.to_string())
        })?;
        let (transfers, history): (Vec<_>, Vec<_>) = statuses.into_iter().partition(|status| status.state == "running");
        libraries.push(BrowserLibraryTransfers { library_id: library.library_id, library_name: library.library_name, scanner_running: false, transfers, history });
    }
    serde_wasm_bindgen::to_value(&libraries).map_err(js_error)
}

#[derive(Serialize)]
struct BrowserLibraryListState {
    libraries: Vec<BrowserLibraryEntry>,
    sync_statuses: Vec<app_contract::account::LibrarySyncStatus>,
}

#[wasm_bindgen]
pub async fn bokheim_library_reconcile() -> Result<JsValue, JsValue> {
    let profile = stored_account_profile()?.ok_or_else(|| js_error("Sign in to synchronize libraries"))?;

    DATABASES.with(|slot| {
        let borrowed = slot.borrow();
        let global = &borrowed.as_ref().ok_or_else(|| js_error("The Rust browser database is not open"))?.global;
        let conflicting = global.query_row("SELECT EXISTS(SELECT 1 FROM rust_library_registry WHERE owner_user_id IS NOT NULL AND owner_user_id!=?1)", [&profile.user_id], |row| row.get::<_, bool>(0)).map_err(js_error)?;
        if conflicting {
            return Err(js_error("A local library belongs to a different account"));
        }
        global.execute("UPDATE rust_library_registry SET owner_user_id=?1 WHERE owner_user_id IS NULL", [&profile.user_id]).map_err(js_error)?;
        Ok(())
    })?;

    let deleted_bytes = authenticated_wire(reqwest::Method::GET, "/api/libraries/deleted", None).await?;
    let deleted: sync_common::api::libraries::DeletedLibrariesResponse = wire::decode(&deleted_bytes, wire::MAX_DECODED_RESPONSE_BYTES).map_err(js_error)?;
    DATABASES.with(|slot| {
        let mut borrowed = slot.borrow_mut();
        let databases = borrowed.as_mut().ok_or_else(|| js_error("The Rust browser database is not open"))?;
        for library_id in deleted.library_ids {
            let library_id = library_id.to_string();
            databases.global.execute("DELETE FROM rust_library_registry WHERE library_id=?1", [&library_id]).map_err(js_error)?;
            databases.libraries.remove(&library_id);
        }
        Ok::<(), JsValue>(())
    })?;

    let remote_bytes = authenticated_wire(reqwest::Method::GET, "/api/libraries", None).await?;
    let remote: Vec<sync_common::api::libraries::LibrarySummary> = wire::decode(&remote_bytes, wire::MAX_DECODED_RESPONSE_BYTES).map_err(js_error)?;
    DATABASES.with(|slot| {
        let borrowed = slot.borrow();
        let global = &borrowed.as_ref().ok_or_else(|| js_error("The Rust browser database is not open"))?.global;
        for library in &remote {
            global
                .execute(
                    "INSERT INTO rust_library_registry(library_id,library_name,created_at,remote,owner_user_id) VALUES(?1,?2,?3,1,?4) ON CONFLICT(library_id) DO UPDATE SET library_name=excluded.library_name,remote=1,owner_user_id=excluded.owner_user_id",
                    rusqlite::params![library.library_id.to_string(), library.library_name, unix_millis().map_err(js_error)?, profile.user_id],
                )
                .map_err(js_error)?;
        }
        Ok::<(), JsValue>(())
    })?;

    let local = registered_library_entries()?;
    let mut remote_ids = remote.iter().map(|library| library.library_id).collect::<std::collections::HashSet<_>>();
    for library in &local {
        let library_id = uuid::Uuid::parse_str(&library.library_id).map_err(js_error)?;
        if remote_ids.contains(&library_id) || bokheim_library_book_count(&library.library_id)? == 0 {
            continue;
        }
        let request = sync_common::api::libraries::UpsertLibraryRequest { library_id, library_name: library.library_name.clone() };
        let body = wire::encode(&request).map_err(js_error)?;
        let response = authenticated_wire(reqwest::Method::POST, "/api/libraries", Some(&body)).await?;
        let enrolled: sync_common::api::libraries::LibrarySummary = wire::decode(&response, wire::MAX_DECODED_RESPONSE_BYTES).map_err(js_error)?;
        remote_ids.insert(enrolled.library_id);
        DATABASES.with(|slot| {
            let borrowed = slot.borrow();
            let global = &borrowed.as_ref().ok_or_else(|| js_error("The Rust browser database is not open"))?.global;
            global.execute("UPDATE rust_library_registry SET library_name=?2,remote=1 WHERE library_id=?1", rusqlite::params![enrolled.library_id.to_string(), enrolled.library_name]).map_err(js_error)?;
            Ok::<(), JsValue>(())
        })?;
    }

    let libraries = registered_library_entries()?;
    let mut sync_statuses = Vec::with_capacity(libraries.len());
    for library in &libraries {
        let library_id = uuid::Uuid::parse_str(&library.library_id).map_err(js_error)?;
        let details = with_library(&library.library_id, |connection| library_sqlite::get_library_sync_details(connection).map_err(|error| error.to_string()))?;
        let state = if remote_ids.contains(&library_id) {
            app_contract::account::LibrarySyncState::Synced
        } else if bokheim_library_book_count(&library.library_id)? > 0 {
            app_contract::account::LibrarySyncState::RemoteMissing
        } else {
            app_contract::account::LibrarySyncState::LocalOnly
        };
        sync_statuses.push(app_contract::account::LibrarySyncStatus {
            library_id,
            library_name: library.library_name.clone(),
            state,
            server_url: Some(ACCOUNT_SERVER_URL.to_owned()),
            account_id: Some(profile.user_id.clone()),
            has_unsynced_changes: details.has_unsynced_changes,
            last_synced_at: details.last_synced_at.and_then(|value| u64::try_from(value).ok()),
        });
    }
    serde_wasm_bindgen::to_value(&BrowserLibraryListState { libraries, sync_statuses }).map_err(js_error)
}

#[derive(Default, Serialize)]
struct BrowserSyncResult {
    pushed: usize,
    pulled: usize,
}

#[wasm_bindgen]
pub async fn bokheim_library_sync(library_id: &str) -> Result<JsValue, JsValue> {
    let parsed_library_id = uuid::Uuid::parse_str(library_id).map_err(js_error)?;
    let mut result = BrowserSyncResult::default();
    for _ in 0..10_000 {
        let (request, cursor, requested_ids, sent_count) = with_library(library_id, |connection| {
            let replica_id = library_sqlite::get_or_create_replica_id(connection).map_err(|error| error.to_string())?;
            let cursor = library_sqlite::get_pull_cursor(connection).map_err(|error| error.to_string())?;
            let pending = library_sqlite::get_pending_mutations(connection).map_err(|error| error.to_string())?.into_iter().take(1_000).collect::<Vec<_>>();
            let requested_ids = pending.iter().map(|mutation| mutation.mutation_id).collect::<Vec<_>>();
            let mutations = pending.iter().map(library_replica::StateMutation::to_wire).collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())?;
            Ok((sync_common::SyncExchangeRequest { library_id: parsed_library_id, replica_id, mutations, cursor }, cursor, requested_ids, pending.len()))
        })?;
        let body = wire::encode(&request).map_err(js_error)?;
        let response_bytes = post_authenticated_wire("/api/sync/exchange", &body).await?;
        let response: sync_common::SyncExchangeResponse = wire::decode(&response_bytes, wire::MAX_DECODED_RESPONSE_BYTES).map_err(js_error)?;
        let validated_push = sync_common::validate_push_response(&requested_ids, &response.push).map_err(js_error)?;
        sync_common::validate_pull_batch(&response.pull, cursor).map_err(js_error)?;
        let has_more = response.pull.has_more;
        let pulled_count = response.pull.mutations.len();
        let next_cursor = response.pull.next_cursor;
        let completed = validated_push.completed_ids();

        with_library_mut(library_id, |connection| {
            let transaction = connection.transaction().map_err(|error| error.to_string())?;
            transaction.pragma_update(None, "defer_foreign_keys", "ON").map_err(|error| error.to_string())?;
            for change in &response.pull.mutations {
                library_sqlite::apply_remote_change_in_tx(&transaction, change).map_err(|error| error.to_string())?;
            }
            library_sqlite::acknowledge_mutations(&transaction, &completed).map_err(|error| error.to_string())?;
            library_sqlite::set_pull_cursor(&transaction, next_cursor).map_err(|error| error.to_string())?;
            library_sqlite::set_sync_metadata(&transaction, "last_successful_sync_at", &unix_millis()?.to_string()).map_err(|error| error.to_string())?;
            transaction.commit().map_err(|error| error.to_string())
        })?;

        result.pushed += validated_push.accepted.len();
        result.pulled += pulled_count;
        if let Some(sync_common::MutationIssue::Rejected(rejection)) = validated_push.issues.iter().find(|issue| matches!(issue, sync_common::MutationIssue::Rejected(_))) {
            return Err(js_error(format!("Server permanently rejected synchronization mutation {}: {:?}", rejection.mutation_id, rejection.reason)));
        }
        let deferred = validated_push.issues.iter().filter(|issue| matches!(issue, sync_common::MutationIssue::Retryable(_))).count();
        if deferred > 0 {
            return Err(js_error(format!("Server deferred {deferred} synchronization change(s); they remain queued")));
        }
        if !has_more && sent_count < 1_000 {
            return serde_wasm_bindgen::to_value(&result).map_err(js_error);
        }
    }
    Err(js_error("Synchronization pagination did not terminate"))
}

async fn synchronize_all_remote_libraries() -> Result<(), JsValue> {
    for library_id in remote_library_ids()? {
        bokheim_library_sync(&library_id).await?;
        bokheim_library_sync_assets(&library_id).await?;
    }
    Ok(())
}

#[wasm_bindgen]
pub async fn bokheim_library_sync_all() -> Result<(), JsValue> {
    bokheim_library_reconcile().await?;
    synchronize_all_remote_libraries().await
}

const ACCOUNT_SERVER_URL: &str = "https://api.bokheim.se";

#[derive(Serialize)]
struct BrowserAccountSession<'a> {
    server_url: &'static str,
    user_id: &'a str,
    email: &'a str,
}

fn stored_account_profile() -> Result<Option<account_contract::MeResponse>, JsValue> {
    DATABASES.with(|slot| {
        let borrowed = slot.borrow();
        let global = &borrowed.as_ref().ok_or_else(|| js_error("The Rust browser database is not open"))?.global;
        match global.query_row("SELECT user_id,email FROM rust_account_profile WHERE singleton=1", [], |row| Ok(account_contract::MeResponse { user_id: row.get(0)?, email: row.get(1)? })) {
            Ok(profile) => Ok(Some(profile)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(js_error(error)),
        }
    })
}

fn store_account_profile(profile: &account_contract::MeResponse) -> Result<(), JsValue> {
    DATABASES.with(|slot| {
        let borrowed = slot.borrow();
        let global = &borrowed.as_ref().ok_or_else(|| js_error("The Rust browser database is not open"))?.global;
        global
            .execute("INSERT INTO rust_account_profile(singleton,user_id,email) VALUES(1,?1,?2) ON CONFLICT(singleton) DO UPDATE SET user_id=excluded.user_id,email=excluded.email", rusqlite::params![profile.user_id, profile.email])
            .map(|_| ())
            .map_err(js_error)
    })
}

fn clear_account_profile() -> Result<(), JsValue> {
    DATABASES.with(|slot| {
        let borrowed = slot.borrow();
        borrowed.as_ref().ok_or_else(|| js_error("The Rust browser database is not open"))?.global.execute("DELETE FROM rust_account_profile WHERE singleton=1", []).map(|_| ()).map_err(js_error)
    })
}

fn account_status(profile: Option<&account_contract::MeResponse>) -> app_contract::account::AccountStatus {
    profile.map(|profile| app_contract::account::AccountStatus::authenticated(ACCOUNT_SERVER_URL, &profile.email)).unwrap_or_else(app_contract::account::AccountStatus::signed_out)
}

#[wasm_bindgen]
pub fn bokheim_account_session() -> Result<JsValue, JsValue> {
    let profile = stored_account_profile()?;
    let session = profile.as_ref().map(|profile| BrowserAccountSession { server_url: ACCOUNT_SERVER_URL, user_id: &profile.user_id, email: &profile.email });
    serde_wasm_bindgen::to_value(&session).map_err(js_error)
}

async fn checked_account_response(response: reqwest::Response, context: &str) -> Result<reqwest::Response, JsValue> {
    if response.status().is_success() {
        return Ok(response);
    }
    let status = response.status();
    let detail = response.text().await.unwrap_or_default();
    Err(js_error(if detail.trim().is_empty() { format!("{context} returned {status}") } else { detail }))
}

async fn post_account_request<T: serde::Serialize>(path: &str, request: &T) -> Result<reqwest::Response, JsValue> {
    let body = wire::encode(request).map_err(js_error)?;
    let response =
        reqwest::Client::new().post(api_url(path)).header(reqwest::header::ACCEPT, wire::MEDIA_TYPE).header(reqwest::header::CONTENT_TYPE, wire::MEDIA_TYPE).body(body).fetch_credentials_same_origin().send().await.map_err(js_error)?;
    checked_account_response(response, "Account server").await
}

async fn decode_profile(response: reqwest::Response) -> Result<account_contract::MeResponse, JsValue> {
    let bytes = response.bytes().await.map_err(js_error)?;
    wire::decode(&bytes, wire::MAX_DECODED_RESPONSE_BYTES).map_err(js_error)
}

#[wasm_bindgen]
pub async fn bokheim_account_status() -> Result<JsValue, JsValue> {
    if let Some(profile) = stored_account_profile()? {
        return serde_wasm_bindgen::to_value(&account_status(Some(&profile))).map_err(js_error);
    }
    let request_profile = || async { reqwest::Client::new().get(api_url("/api/auth/web/me")).header(reqwest::header::ACCEPT, wire::MEDIA_TYPE).fetch_credentials_same_origin().send().await };
    let mut response = request_profile().await.map_err(js_error)?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        let _ = response.bytes().await;
        let refresh = reqwest::Client::new().post(api_url("/api/auth/web/refresh")).header(reqwest::header::ACCEPT, wire::MEDIA_TYPE).fetch_credentials_same_origin().send().await.map_err(js_error)?;
        if !refresh.status().is_success() {
            return serde_wasm_bindgen::to_value(&account_status(None)).map_err(js_error);
        }
        response = request_profile().await.map_err(js_error)?;
    }
    if !response.status().is_success() {
        return serde_wasm_bindgen::to_value(&account_status(None)).map_err(js_error);
    }
    let profile = decode_profile(response).await?;
    store_account_profile(&profile)?;
    serde_wasm_bindgen::to_value(&account_status(Some(&profile))).map_err(js_error)
}

#[wasm_bindgen]
pub async fn bokheim_account_refresh() -> Result<JsValue, JsValue> {
    let response = reqwest::Client::new().post(api_url("/api/auth/web/refresh")).header(reqwest::header::ACCEPT, wire::MEDIA_TYPE).fetch_credentials_same_origin().send().await.map_err(js_error)?;
    let response = checked_account_response(response, "Session refresh").await?;
    let profile = decode_profile(response).await?;
    store_account_profile(&profile)?;
    serde_wasm_bindgen::to_value(&account_status(Some(&profile))).map_err(js_error)
}

#[wasm_bindgen]
pub async fn bokheim_account_login(email: String, password: String) -> Result<JsValue, JsValue> {
    let response = post_account_request("/api/auth/web/login", &account_contract::LoginRequest { email, password }).await?;
    let profile = decode_profile(response).await?;
    store_account_profile(&profile)?;
    serde_wasm_bindgen::to_value(&account_status(Some(&profile))).map_err(js_error)
}

#[wasm_bindgen]
pub async fn bokheim_account_register(email: String, password: String) -> Result<(), JsValue> {
    post_account_request("/api/auth/register", &account_contract::PublicRegistrationRequest { email, password }).await.map(|_| ())
}

#[wasm_bindgen]
pub async fn bokheim_account_resend_verification(email: String) -> Result<(), JsValue> {
    post_account_request("/api/auth/resend-verification", &account_contract::EmailRequest { email }).await.map(|_| ())
}

#[wasm_bindgen]
pub async fn bokheim_account_verify_email(email: String, pin: String) -> Result<JsValue, JsValue> {
    let response = post_account_request("/api/auth/web/verify-email", &account_contract::EmailVerificationRequest { email, pin }).await?;
    let profile = decode_profile(response).await?;
    store_account_profile(&profile)?;
    serde_wasm_bindgen::to_value(&account_status(Some(&profile))).map_err(js_error)
}

#[wasm_bindgen]
pub async fn bokheim_account_request_password_reset(email: String) -> Result<(), JsValue> {
    post_account_request("/api/auth/request-password-reset", &account_contract::EmailRequest { email }).await.map(|_| ())
}

#[wasm_bindgen]
pub async fn bokheim_account_reset_password(token: String, new_password: String) -> Result<(), JsValue> {
    post_account_request("/api/auth/reset-password", &account_contract::ResetPasswordRequest { token, new_password }).await.map(|_| ())
}

#[wasm_bindgen]
pub async fn bokheim_account_logout() -> Result<JsValue, JsValue> {
    let response = reqwest::Client::new().post(api_url("/api/auth/web/logout")).header(reqwest::header::ACCEPT, wire::MEDIA_TYPE).fetch_credentials_same_origin().send().await;
    clear_account_profile()?;
    if let Ok(response) = response {
        checked_account_response(response, "Account logout").await?;
    }
    serde_wasm_bindgen::to_value(&account_status(None)).map_err(js_error)
}

#[wasm_bindgen]
pub async fn bokheim_account_storage_usage() -> Result<JsValue, JsValue> {
    let bytes = authenticated_wire(reqwest::Method::GET, "/api/account/storage", None).await?;
    let usage: account_contract::StorageUsageResponse = wire::decode(&bytes, wire::MAX_DECODED_RESPONSE_BYTES).map_err(js_error)?;
    to_js_value(&usage)
}

#[wasm_bindgen]
pub async fn bokheim_account_library_storage_usage() -> Result<JsValue, JsValue> {
    let bytes = authenticated_wire(reqwest::Method::GET, "/api/account/storage/libraries", None).await?;
    let usage: account_contract::LibraryStorageUsageResponse = wire::decode(&bytes, wire::MAX_DECODED_RESPONSE_BYTES).map_err(js_error)?;
    to_js_value(&usage)
}
