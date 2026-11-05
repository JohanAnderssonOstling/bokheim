//! SQLite ownership and the persisted browse projection of a catalog snapshot.
use super::popularity::{read_popularity, read_popularity_for};
use super::{CatalogBook, FORMAT_VERSION, read_sources, read_sources_for, release};
use crate::query_methods::uuid_blob_column;
use crate::{DatabaseError, attach_taxonomy, taxonomy_update};
use book_model::{AUTHOR_MARC_RELATOR_CODE, BookDetails, BookFormat, BookMetadata, BookRecord, BookSubject, Contributor, Identifier, LanguageTag, Scheme, Scope, normalize_search_text};
use content_address::BookId;
use description_storage::{Source as DescriptionSource, read as read_description};
use library_model::CardKey;
use rusqlite::{
    Connection, Error as SqlError, OptionalExtension, Row,
    backup::{Backup, StepResult},
    functions::FunctionFlags,
    params, params_from_iter,
    types::{Type, Value},
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use subject_projection::{CATALOG_CONCEPT_SOURCE, UNIFIED_SYSTEM_ID};
use url::Url;

struct ActiveCatalog {
    connection: Connection,
    path: Option<PathBuf>,
    generation: u64,
}
static ACTIVE: OnceLock<Mutex<ActiveCatalog>> = OnceLock::new();
const PROJECTION_VERSION: i64 = 19;
const SCHEMA: &str = include_str!("schema.sql");
const COMPACT_SCHEMA: &str = include_str!("compact.sql");
// An inactive catalog exposes empty browse tables without allocating source records.
const EMPTY_AUTHORS: &str = "CREATE TABLE catalog_browse_author(author_id BLOB PRIMARY KEY CHECK(length(author_id)=16),name TEXT NOT NULL) STRICT, WITHOUT ROWID;
CREATE TABLE catalog_browse_author_book(book_row_id INTEGER NOT NULL,author_id BLOB NOT NULL,PRIMARY KEY(book_row_id,author_id)) STRICT, WITHOUT ROWID;";

fn read_metadata(db: &Connection, source_id: i64, year: Option<i32>) -> Result<BookMetadata, DatabaseError> {
    let mut metadata = BookMetadata { year, ..Default::default() };
    let mut identifiers = db.prepare_cached("SELECT value,scheme,scheme_label,scope FROM book_identifier WHERE book_row_id=?1 ORDER BY scheme,scheme_label,value,scope")?;
    for identifier in identifiers.query_map([&source_id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?, row.get::<_, i64>(3)?)))? {
        let (value, scheme, label, scope) = identifier?;
        let scheme = match scheme {
            0 => Scheme::Unspecified,
            1 => Scheme::Isbn,
            2 => Scheme::Asin,
            3 => Scheme::Doi,
            4 => Scheme::Issn,
            5 => Scheme::Oclc,
            6 => Scheme::Lccn,
            7 => Scheme::Uuid,
            8 => Scheme::Calibre,
            9 => Scheme::Google,
            10 => Scheme::Goodreads,
            11 => Scheme::Kobo,
            12 => Scheme::Other(label),
            _ => return Err(DatabaseError::message("catalog identifier has an unsupported scheme")),
        };
        let scope = match scope {
            0 => Scope::Book,
            1 => Scope::Edition,
            2 => Scope::Work,
            _ => return Err(DatabaseError::message("catalog identifier has an unsupported scope")),
        };
        metadata.identifiers.push(Identifier::new(value, scheme, scope).map_err(DatabaseError::operation)?);
    }
    metadata.languages = db
        .prepare_cached("SELECT language FROM book_language WHERE book_row_id=?1 ORDER BY language")?
        .query_map([&source_id], |row| row.get::<_, String>(0))?
        .map(|language| LanguageTag::parse(language?).map_err(DatabaseError::operation))
        .collect::<Result<Vec<_>, DatabaseError>>()?;
    if release(db)?.1 >= 9 {
        let mut subjects = db.prepare_cached(
            "SELECT assignment.concept_id,concept.preferred_label FROM book_concept assignment
             LEFT JOIN curated.concept ON concept.concept_id=assignment.concept_id
             WHERE assignment.book_row_id=?1 ORDER BY assignment.concept_id",
        )?;
        metadata.subjects = subjects
            .query_map([source_id], |row| {
                let concept = row.get::<_, i64>(0)?;
                let code = format!("concept:{concept}");
                let label = row.get::<_, Option<String>>(1)?.unwrap_or_else(|| code.clone());
                BookSubject::new(None, label, CATALOG_CONCEPT_SOURCE, Some(UNIFIED_SYSTEM_ID.into()), Some(code)).map_err(|error| SqlError::FromSqlConversionFailure(0, Type::Integer, Box::new(error)))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(metadata);
    }
    metadata.subjects = db
        .prepare_cached("SELECT subject.id,subject.name,sort_name,source.name,authority.name,code FROM book_subject JOIN subject USING(subject_id) JOIN subject_source source ON source.id=subject.source_id LEFT JOIN subject_authority authority ON authority.id=subject.authority_id WHERE book_row_id=?1 ORDER BY subject_id")?
        .query_map([&source_id], |row| {
            BookSubject::with_details(row.get(0)?, row.get::<_, String>(1)?, row.get(2)?, row.get::<_, String>(3)?, row.get(4)?, row.get(5)?).map_err(|error| SqlError::FromSqlConversionFailure(1, Type::Text, Box::new(error)))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(metadata)
}

fn decode_record(db: &Connection, source_id: i64, title: &str, subtitle: Option<&str>, description: String, year: Option<i32>, contributors: Vec<Contributor>) -> Result<BookRecord, DatabaseError> {
    let book = read_metadata(db, source_id, year)?;
    Ok(BookRecord { metadata: BookDetails { title: title.to_owned(), subtitle: subtitle.map(str::to_owned), contributors, book }, description })
}

fn author_from_row(row: &Row<'_>) -> rusqlite::Result<Contributor> {
    let id = uuid_blob_column(row, 0)?;
    Contributor::with_id(id, row.get::<_, String>(1)?, AUTHOR_MARC_RELATOR_CODE).map_err(|error| SqlError::FromSqlConversionFailure(1, Type::Text, Box::new(error)))
}

fn book_authors(db: &Connection, source_id: i64) -> Result<Vec<Contributor>, DatabaseError> {
    Ok(db
        .prepare_cached("SELECT author.author_id,name FROM book_author credit JOIN author ON author.row_id=credit.author_row_id WHERE credit.book_row_id=?1 ORDER BY name,author.author_id")?
        .query_map([source_id], author_from_row)?
        .collect::<Result<Vec<_>, _>>()?)
}

pub(crate) fn read<T>(query: impl FnOnce(&Connection) -> Result<T, DatabaseError>) -> Result<T, DatabaseError> {
    read_snapshot(|connection, _| query(connection))
}

/// Hold the catalog lock for selection, metadata and card hydration together.
pub(crate) fn read_snapshot<T>(query: impl FnOnce(&Connection, u64) -> Result<T, DatabaseError>) -> Result<T, DatabaseError> {
    match ACTIVE.get() {
        Some(active) => {
            let database = active.lock().map_err(|_| DatabaseError::message("catalog database lock is poisoned"))?;
            let snapshot = database.connection.unchecked_transaction()?;
            let result = query(&snapshot, database.generation)?;
            snapshot.commit()?;
            Ok(result)
        }
        None => {
            let empty = Connection::open_in_memory()?;
            empty.execute_batch(SCHEMA)?;
            empty.execute_batch(EMPTY_AUTHORS)?;
            empty.execute_batch("CREATE VIEW catalog_hosted AS SELECT NULL AS identity,NULL AS title,NULL AS subtitle,NULL AS description,NULL AS year WHERE 0; CREATE VIEW catalog_external AS SELECT NULL AS identity,NULL AS title,NULL AS subtitle,NULL AS description,NULL AS year,NULL AS external_url,NULL AS download_url,NULL AS format WHERE 0")?;
            configure_title_matching(&empty)?;
            query(&empty, 0)
        }
    }
}

fn configure_title_matching(connection: &Connection) -> Result<(), DatabaseError> {
    connection.create_scalar_function("catalog_title_key", 1, FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC, |context| Ok(context.get::<String>(0)?.trim().to_ascii_lowercase()))?;
    connection.create_scalar_function("normalize_search_text", 1, FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC, |context| Ok(normalize_search_text(&context.get::<String>(0)?)))?;
    Ok(())
}

fn activate_connection(connection: Connection, path: Option<PathBuf>) -> Result<(), DatabaseError> {
    configure_title_matching(&connection)?;
    connection.set_prepared_statement_cache_capacity(128);
    connection.pragma_update(None, "query_only", true)?;
    connection.pragma_update(None, "cache_size", -2048)?;
    ACTIVE.set(Mutex::new(ActiveCatalog { connection, path, generation: 0 })).map_err(|_| DatabaseError::message("the catalog can only be activated once per process"))
}

/// Reads and indexes one catalog database; records and navigation have one
/// on-disk copy. Browse tables are rebuilt only for a changed schema or taxonomy.
pub fn activate_path(path: &Path) -> Result<Option<u64>, DatabaseError> {
    if ACTIVE.get().is_some() {
        return Err(DatabaseError::message("the catalog can only be activated once per process"));
    }
    let Some((release, connection)) = prepare_path(path)? else { return Ok(None) };
    activate_connection(connection, Some(path.to_owned()))?;
    Ok(Some(release))
}

fn prepare_path(path: &Path) -> Result<Option<(u64, Connection)>, DatabaseError> {
    let Some(mut connection) = open_existing_catalog(path)? else { return Ok(None) };
    let Ok((release_id, format)) = release(&connection) else { return Ok(None) };
    if format != FORMAT_VERSION {
        return Ok(None);
    }
    connection.pragma_update(None, "cache_size", -2048)?;
    attach_taxonomy(&connection)?;
    let revision = taxonomy_update::revision(&connection)?;
    let cached = connection
        .query_row("SELECT version,release_id,format_version,taxonomy_revision FROM catalog_browse_revision WHERE singleton=1", [], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)? as u64, row.get::<_, u32>(2)?, row.get::<_, String>(3)?))
        })
        .optional();
    let overridden = std::env::var_os("BOKHEIM_CATALOG_FILE").is_some_and(|local| Path::new(&local) == path);
    if overridden || !matches!(cached, Ok(Some((version, source_release, source_format, ref taxonomy))) if version == PROJECTION_VERSION && source_release == release_id && source_format == format && taxonomy == &revision) {
        build_projection(&mut connection, &revision)?;
    }
    Ok(Some((release_id, connection)))
}

/// Catalog reads may first need rollback recovery. Detach a published hard link
/// before opening it writable, then let SQLite recover before probing its schema.
fn open_existing_catalog(path: &Path) -> Result<Option<Connection>, DatabaseError> {
    #[cfg(unix)]
    {
        if !path.try_exists().map_err(DatabaseError::operation)? {
            return Ok(None);
        }
        make_private(path)?;
    }
    crate::sqlite::open_existing_writable(path).map_err(DatabaseError::message)
}

/// Published snapshots can be hard-linked into the app-data directory. Detach
/// that link before adding local tables so published release bytes stay intact.
#[cfg(unix)]
fn make_private(path: &Path) -> Result<(), DatabaseError> {
    use std::os::unix::fs::MetadataExt;
    if std::fs::metadata(path).map_err(DatabaseError::operation)?.nlink() > 1 {
        let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
        std::fs::copy(path, &temporary).map_err(DatabaseError::operation)?;
        std::fs::rename(&temporary, path).map_err(DatabaseError::operation)?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn make_private(_path: &Path) -> Result<(), DatabaseError> {
    Ok(())
}

#[derive(Default)]
struct StableIds {
    books: HashMap<CardKey, i64>,
}

fn previous_ids(db: &Connection) -> Result<StableIds, DatabaseError> {
    let has_projection: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='catalog_browse_book')", [], |row| row.get(0))?;
    if !has_projection {
        return Ok(StableIds::default());
    }
    let mut ids = StableIds::default();
    let mut rows = db.prepare("SELECT key,row_id FROM catalog_browse_book")?;
    for row in rows.query_map([], |row| Ok((CardKey::from(row.get::<_, BookId>(0)?), row.get::<_, i64>(1)?)))? {
        let (key, id) = row?;
        ids.books.insert(key, id);
    }
    Ok(ids)
}

pub(super) fn store_snapshot(source: &Connection, path: &Path) -> Result<(), DatabaseError> {
    let started = web_time::Instant::now();
    // Copy only the stable browse IDs while holding the catalog lock. Source
    // import and taxonomy projection must not block the library actor that
    // answers book-open requests.
    let active_ids = if let Some(active) = ACTIVE.get() {
        let active = active.lock().map_err(|_| DatabaseError::message("catalog database lock is poisoned"))?;
        if active.path.as_deref() == Some(path) { Some((previous_ids(&active.connection)?, active.generation)) } else { None }
    } else {
        None
    };
    let (ids, generation) = match active_ids {
        Some((ids, generation)) => (ids, Some(generation)),
        None => {
            let ids = match open_existing_catalog(path)? {
                Some(connection) => previous_ids(&connection)?,
                None => StableIds::default(),
            };
            (ids, None)
        }
    };
    let mut prepared = Connection::open_in_memory()?;
    prepared.pragma_update(None, "foreign_keys", true)?;
    attach_taxonomy(&prepared)?;
    install_snapshot(&mut prepared, source, ids)?;
    log::debug!(target: "book_startup", "catalog_update phase=prepare_complete elapsed_ms={}", started.elapsed().as_millis());

    if let Some(active) = ACTIVE.get() {
        let mut active = active.lock().map_err(|_| DatabaseError::message("catalog database lock is poisoned"))?;
        if active.path.as_deref() == Some(path) {
            if generation != Some(active.generation) {
                return Err(DatabaseError::message("catalog changed while its update was being prepared"));
            }
            let connection = &mut active.connection;
            connection.pragma_update(None, "query_only", false)?;
            let result = publish_snapshot(&prepared, connection);
            let restore = connection.pragma_update(None, "query_only", true);
            restore?;
            if result.is_ok() {
                active.generation = active.generation.wrapping_add(1);
            }
            return result;
        }
    }
    let mut destination = match open_existing_catalog(path)? {
        Some(connection) => connection,
        None => Connection::open(path)?,
    };
    destination.pragma_update(None, "cache_size", -2048)?;
    publish_snapshot(&prepared, &mut destination)
}

/// SQLite publishes the prepared pages as one transaction into the existing
/// file, so library connections that attached it also see the new snapshot.
/// Replacing pages avoids dropping old parent tables with live foreign-key
/// relationships, which can otherwise perform quadratic constraint work.
fn publish_snapshot(source: &Connection, destination: &mut Connection) -> Result<(), DatabaseError> {
    let started = web_time::Instant::now();
    destination.flush_prepared_statement_cache();
    let backup = Backup::new(source, destination)?;
    match backup.step(-1)? {
        StepResult::Done => {
            log::debug!(target: "book_startup", "catalog_update phase=publish_complete elapsed_ms={}", started.elapsed().as_millis());
            Ok(())
        }
        _ => Err(DatabaseError::message("catalog is busy; its update was not published")),
    }
}

fn quote_identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('\"', "\"\""))
}

/// Replacing source rows and derived rows is one SQLite transaction. Readers
/// cannot observe a new snapshot with missing or stale browse tables.
fn install_snapshot(destination: &mut Connection, source: &Connection, ids: StableIds) -> Result<(), DatabaseError> {
    let page_size: u32 = source.pragma_query_value(None, "page_size", |row| row.get(0))?;
    destination.pragma_update(None, "page_size", page_size)?;
    let revision = taxonomy_update::revision(destination)?;
    let schema = source
        .prepare("SELECT type,name,sql FROM sqlite_schema WHERE sql IS NOT NULL AND name NOT LIKE 'sqlite_%' ORDER BY CASE type WHEN 'table' THEN 0 WHEN 'index' THEN 1 WHEN 'view' THEN 2 ELSE 3 END")?
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    let tx = destination.transaction()?;
    // Source and projection tables are replaced together. Parent rows may
    // disappear temporarily; validate the restored relationships at commit.
    tx.pragma_update(None, "defer_foreign_keys", true)?;
    let old = tx
        .prepare("SELECT type,name FROM sqlite_schema WHERE sql IS NOT NULL AND name NOT LIKE 'sqlite_%' ORDER BY CASE type WHEN 'view' THEN 0 WHEN 'trigger' THEN 1 WHEN 'index' THEN 2 ELSE 3 END")?
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    for (kind, name) in old {
        tx.execute_batch(&format!("DROP {} IF EXISTS {}", kind.to_uppercase(), quote_identifier(&name)))?;
    }
    for (kind, name, sql) in &schema {
        if kind != "table" {
            continue;
        }
        if name == "description" {
            tx.execute_batch("CREATE TABLE description(row_id INTEGER PRIMARY KEY,text ANY NOT NULL CHECK(typeof(text) IN('text','blob'))) STRICT")?;
        } else {
            tx.execute_batch(sql)?;
        }
        let columns = source.prepare("SELECT name FROM pragma_table_info(?1)")?.query_map([name], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
        let columns_sql = columns.iter().map(|name| quote_identifier(name)).collect::<Vec<_>>().join(",");
        let parameters = (1..=columns.len()).map(|index| format!("?{index}")).collect::<Vec<_>>().join(",");
        let mut select = source.prepare(&format!("SELECT {columns_sql} FROM {}", quote_identifier(name)))?;
        let mut insert = tx.prepare(&format!("INSERT INTO {}({columns_sql}) VALUES({parameters})", quote_identifier(name)))?;
        let mut rows = select.query([])?;
        while let Some(row) = rows.next()? {
            let mut values = (0..columns.len()).map(|index| row.get::<_, Value>(index)).collect::<Result<Vec<_>, _>>()?;
            if name == "description" {
                if let Some(index) = columns.iter().position(|column| column == "text") {
                    values[index] = description_storage::encode((&values[index]).into())?;
                }
            }
            insert.execute(params_from_iter(values))?;
        }
    }
    for (kind, _, sql) in &schema {
        if kind != "table" {
            tx.execute_batch(sql)?;
        }
    }
    build_projection_in(&tx, &revision, ids)?;
    tx.commit()?;
    destination.execute_batch("VACUUM")?;
    Ok(())
}

/// Activation entry point for callers supplying a small SQLite image.
/// Application startup uses activate_path, including in the browser's OPFS VFS.
pub fn activate(image: Vec<u8>) -> Result<(), DatabaseError> {
    let mut source = Connection::open_in_memory()?;
    source.deserialize_read_exact("main", image.as_slice(), image.len(), false)?;
    drop(image);
    if release(&source)?.1 != FORMAT_VERSION {
        return Err(DatabaseError::message("unsupported catalog snapshot format"));
    }
    let ids = previous_ids(&source)?;
    let mut connection = Connection::open_in_memory()?;
    connection.pragma_update(None, "foreign_keys", true)?;
    attach_taxonomy(&connection)?;
    install_snapshot(&mut connection, &source, ids)?;
    activate_connection(connection, None)
}

fn source_view(db: &Connection) -> Result<(), DatabaseError> {
    db.execute_batch(
        "DROP VIEW IF EXISTS temp.catalog_source; DROP VIEW IF EXISTS catalog_hosted; DROP VIEW IF EXISTS catalog_external;
        CREATE VIEW catalog_hosted AS SELECT key AS identity,title,subtitle,description.text AS description,year
            FROM book JOIN description ON description.row_id=book.description_id WHERE external=0;
        CREATE VIEW catalog_external AS SELECT key AS identity,title,subtitle,description.text AS description,year,entry_url AS external_url,download_url,format
            FROM book JOIN description ON description.row_id=book.description_id WHERE external=1;
        CREATE TEMP VIEW catalog_source AS SELECT identity,title,subtitle,description,year,0 AS external,NULL AS external_url,NULL AS download_url,1 AS format FROM catalog_hosted
            UNION ALL SELECT identity,title,subtitle,description,year,1,external_url,download_url,format FROM catalog_external;",
    )?;
    Ok(())
}

pub(super) fn build_projection(db: &mut Connection, taxonomy_revision: &str) -> Result<(), DatabaseError> {
    let ids = previous_ids(db)?;
    let tx = db.transaction()?;
    build_projection_in(&tx, taxonomy_revision, ids)?;
    tx.commit()?;
    Ok(())
}

fn published_concepts(db: &Connection) -> Result<HashMap<CardKey, Vec<i64>>, DatabaseError> {
    let mut concepts = HashMap::<CardKey, Vec<i64>>::new();
    let mut statement = db.prepare("SELECT b.key,c.concept_id FROM book_concept c JOIN book b ON b.row_id=c.book_row_id ORDER BY b.row_id,c.concept_id")?;
    for row in statement.query_map([], |row| Ok((CardKey::from(row.get::<_, BookId>(0)?), row.get(1)?)))? {
        let (key, concept) = row?;
        concepts.entry(key).or_default().push(concept);
    }
    Ok(concepts)
}

#[cfg(test)]
mod published_concept_tests {
    use super::*;

    #[test]
    fn compact_assignments_retain_identities_across_taxonomy_revisions() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE snapshot(singleton INTEGER PRIMARY KEY,format_version INTEGER,release_id INTEGER); INSERT INTO snapshot VALUES(1,11,1);
             CREATE TABLE book(row_id INTEGER PRIMARY KEY,key BLOB); CREATE TABLE book_concept(book_row_id INTEGER,concept_id INTEGER);
             INSERT INTO book VALUES(1,zeroblob(32)),(2,randomblob(32)); INSERT INTO book_concept VALUES(1,3915);",
        )
        .unwrap();
        assert_eq!(published_concepts(&db).unwrap(), HashMap::from([(CardKey([0; 32]), vec![3915])]));
    }
}

fn build_projection_in(db: &Connection, taxonomy_revision: &str, ids: StableIds) -> Result<(), DatabaseError> {
    if release(db)?.1 != FORMAT_VERSION {
        return Err(DatabaseError::message("unsupported catalog snapshot format"));
    }
    db.execute_batch(metadata_contract::catalog::SOURCE_PERCENTILES_SQL)?;
    source_view(db)?;
    let mut sources = read_sources(db)?;
    let mut popularity = read_popularity(db)?;
    let mut concepts = published_concepts(db)?;
    let keys = db.prepare("SELECT identity FROM catalog_source ORDER BY identity")?.query_map([], |row| row.get::<_, BookId>(0))?.collect::<Result<Vec<_>, _>>()?;
    for table in ["catalog_browse_author_book", "catalog_browse_author", "catalog_browse_language", "catalog_browse_concept", "catalog_browse_book", "catalog_browse_search", "catalog_browse_author_search", "catalog_browse_revision"] {
        if let Some(kind) = db.query_row("SELECT type FROM sqlite_schema WHERE name=?1", [table], |row| row.get::<_, String>(0)).optional()? {
            db.execute_batch(&format!("DROP {} {table}", kind.to_uppercase()))?;
        }
    }
    db.execute_batch(COMPACT_SCHEMA)?;
    let mut next_id = ids.books.values().copied().max().map_or(0, |id| id + 1);
    for identity in keys {
        let key = CardKey::from(identity);
        let index = ids.books.get(&key).copied().unwrap_or_else(|| {
            let id = next_id;
            next_id += 1;
            id
        });
        let (title, subtitle, external, external_url, download_url, format, year, source_id) = db.query_row(
            "SELECT source.title,source.subtitle,source.external,source.external_url,source.download_url,source.format,source.year,book.row_id FROM catalog_source source JOIN book ON book.key=source.identity WHERE source.identity=?1",
            [&key.0[..]],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, bool>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, Option<i32>>(6)?,
                    row.get::<_, i64>(7)?,
                ))
            },
        )?;
        let format = BookFormat::from_code(format).ok_or_else(|| DatabaseError::message("catalog entry has an unsupported file format"))?;
        if external {
            let url = external_url.as_deref().ok_or_else(|| DatabaseError::message("external catalog entry has no URL"))?;
            for url in std::iter::once(url).chain(download_url.as_deref()) {
                let url = Url::parse(url).map_err(DatabaseError::operation)?;
                if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() {
                    return Err(DatabaseError::message("external catalog URL must use HTTP or HTTPS without credentials"));
                }
            }
        }
        let contributors = book_authors(db, source_id)?;
        let author = contributors.iter().map(|credit| credit.name()).collect::<Vec<_>>().join(", ");
        // Search and navigation need metadata, never description text.
        let record = decode_record(db, source_id, &title, subtitle.as_deref(), String::new(), year, contributors)?;
        let mut book = CatalogBook {
            hash: (!external).then_some(identity),
            key,
            external_url,
            download_url,
            file_format: format,
            audiobook_duration_ms: None,
            sources: sources.remove(&key.0).unwrap_or_default(),
            title,
            subtitle,
            author,
            search_title: String::new(),
            search_author: String::new(),
            languages: Vec::new(),
            concepts: Vec::new(),
            record,
            toc: Vec::new(),
            popularity: popularity.remove(&key.0),
        };
        book.search_title = crate::browse::text::search_title(Some(&book.title), book.subtitle.as_deref());
        book.search_author = normalize_search_text(&book.author);
        book.concepts = concepts.remove(&key).unwrap_or_default();
        book.concepts.sort_unstable();
        book.concepts.dedup();
        db.prepare_cached("INSERT INTO catalog_browse_search(row_id,book_row_id,search_title,search_author,source_priority,popularity_score) VALUES(?1,?2,?3,?4,?5,?6)")?.execute(params![
            index,
            source_id,
            book.search_title,
            book.search_author,
            book.source_priority(),
            book.popularity.as_ref().and_then(|value| value.score())
        ])?;
        for concept in &book.concepts {
            db.execute("INSERT INTO catalog_browse_concept VALUES(?1,?2)", params![index, concept])?;
        }
    }
    let (release, format) = release(db)?;
    db.execute("INSERT INTO catalog_browse_revision VALUES(1,?1,?2,?3,?4)", params![PROJECTION_VERSION, i64::try_from(release).map_err(DatabaseError::operation)?, format, taxonomy_revision])?;
    db.execute_batch("DROP VIEW catalog_source")?;
    Ok(())
}

pub fn contains(key: CardKey, external: Option<bool>) -> Result<bool, DatabaseError> {
    read(|db| Ok(db.query_row("SELECT EXISTS(SELECT 1 FROM catalog_browse_book WHERE key=?1 AND (?2 IS NULL OR external=?2))", params![&key.0[..], external], |row| row.get(0))?))
}

pub fn audiobook_url(key: CardKey) -> Result<Option<String>, DatabaseError> {
    read(|db| {
        Ok(db
            .query_row(
                "SELECT source.download_url FROM catalog_browse_book book JOIN catalog_external source ON source.identity=book.key WHERE book.key=?1 AND book.external=1 AND book.format=?2",
                params![&key.0[..], BookFormat::Mp3Folder as i64],
                |row| row.get(0),
            )
            .optional()?
            .flatten())
    })
}

/// Loads one catalog record. Chapters come from the added book's own files.
pub fn entry(key: CardKey) -> Result<Option<CatalogBook>, DatabaseError> {
    read(|db| load_entry(db, key))
}

/// The entry's row in the browse projection, which is what the catalog's
/// subject tree lists books by.
pub(crate) fn browse_row(key: CardKey) -> Result<Option<i64>, DatabaseError> {
    read(|db| Ok(db.query_row("SELECT row_id FROM catalog_browse_book WHERE key=?1", [&key.0[..]], |row| row.get(0)).optional()?))
}

pub(super) fn load_entry(db: &Connection, key: CardKey) -> Result<Option<CatalogBook>, DatabaseError> {
    let found = db
        .query_row("SELECT external,format,search_title,search_author,duration_ms FROM catalog_browse_book WHERE key=?1", [&key.0[..]], |row| {
            let duration = row.get::<_, Option<i64>>(4)?.map(u64::try_from).transpose().map_err(|error| SqlError::FromSqlConversionFailure(4, Type::Integer, Box::new(error)))?;
            Ok((row.get::<_, bool>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?, duration))
        })
        .optional()?;
    let Some((external, format, search_title, search_author, audiobook_duration_ms)) = found else { return Ok(None) };
    let (view, urls) = if external { ("catalog_external", "source.external_url,source.download_url") } else { ("catalog_hosted", "NULL,NULL") };
    let (title, subtitle, description_id, year, external_url, download_url, source_id) =
        db.query_row(&format!("SELECT source.title,source.subtitle,book.description_id,source.year,{urls},book.row_id FROM {view} source JOIN book ON book.key=source.identity WHERE source.identity=?1"), [&key.0[..]], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?, row.get::<_, i64>(2)?, row.get::<_, Option<i32>>(3)?, row.get::<_, Option<String>>(4)?, row.get::<_, Option<String>>(5)?, row.get::<_, i64>(6)?))
        })?;
    let description = read_description(db, DescriptionSource::Catalog, description_id)?.ok_or(SqlError::QueryReturnedNoRows)?;
    let contributors = book_authors(db, source_id)?;
    let author = contributors.iter().map(|credit| credit.name()).collect::<Vec<_>>().join(", ");
    let record = decode_record(db, source_id, &title, subtitle.as_deref(), description, year, contributors)?;
    let concepts = db
        .prepare_cached("SELECT concept_id FROM catalog_browse_concept WHERE book_row_id=(SELECT row_id FROM catalog_browse_book WHERE key=?1) ORDER BY concept_id")?
        .query_map([&key.0[..]], |row| row.get::<_, i64>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let sources = read_sources_for(db, &key.0)?;
    let popularity = read_popularity_for(db, &key.0)?;
    let languages = record.book.languages.iter().map(|language| crate::browse::options::base_language(language.as_str())).filter(|language| !language.is_empty()).collect();
    Ok(Some(CatalogBook {
        hash: (!external).then(|| BookId::from_bytes(key.0)),
        key,
        external_url,
        download_url,
        file_format: BookFormat::from_code(format).ok_or_else(|| DatabaseError::message("catalog entry has an unsupported file format"))?,
        audiobook_duration_ms,
        sources,
        title,
        subtitle,
        author,
        search_title,
        search_author,
        languages,
        concepts,
        record,
        toc: Vec::new(),
        popularity,
    }))
}

/// Only candidate recordings of this title need full author/language records.
pub fn audiobook_candidates(title: &str) -> Result<Vec<CatalogBook>, DatabaseError> {
    read(|db| audiobook_candidates_in(db, title))
}

fn audiobook_candidates_in(db: &Connection, title: &str) -> Result<Vec<CatalogBook>, DatabaseError> {
    let keys = db
        .prepare(
            "SELECT book.key FROM catalog_browse_book book JOIN catalog_external source ON source.identity=book.key WHERE book.external=1 AND book.format=?1 AND catalog_title_key(source.title)=catalog_title_key(?2) ORDER BY book.row_id",
        )?
        .query_map(params![BookFormat::Mp3Folder as i64, title], |row| row.get::<_, BookId>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    keys.into_iter().map(|key| load_entry(db, CardKey::from(key))?.ok_or_else(|| DatabaseError::message("catalog entry is missing"))).collect()
}

#[cfg(test)]
mod record_tests {
    use super::*;
    use crate::catalog::{CatalogPopularity, CombinedPopularity};
    use uuid::Uuid;

    fn fixture() -> Connection {
        fixture_with_schema(include_str!("../../tests/sql/catalog_pagination.sql"))
    }

    fn fixture_with_schema(schema: &str) -> Connection {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(schema).unwrap();
        db.execute_batch(SCHEMA).unwrap();
        source_view(&db).unwrap();
        attach_taxonomy(&db).unwrap();
        db.execute_batch("INSERT INTO snapshot VALUES(1,11,1); INSERT INTO source VALUES(1,'example.org'); INSERT INTO description VALUES(1,'Shared description');").unwrap();
        for index in 1..=2 {
            let key = CardKey([index as u8; 32]);
            let external = index == 2;
            let format = if external { BookFormat::Mp3Folder } else { BookFormat::Epub };
            db.execute(
                "INSERT INTO book(row_id,key,external,title,subtitle,description_id,year,format,entry_url,download_url) VALUES(?1,?2,?3,?4,'Subtitle',1,1984,?5,'https://example.org/book','https://example.org/audio.zip')",
                params![index, &key.0[..], external, format!("Book {index}"), format as i64],
            )
            .unwrap();
            db.execute(
                "INSERT INTO catalog_browse_book(row_id,key,external,format,search_title,search_author,source_priority) VALUES(?1,?2,?3,?4,?5,'',0)",
                params![index + 100, &key.0[..], external, format as i64, format!("book {index}")],
            )
            .unwrap();
            for author in [2, 1] {
                let author_id = index * 10 + author;
                db.execute("INSERT INTO author VALUES(?1,?2,?3)", params![author_id, Uuid::from_u128(author_id as u128).as_bytes().as_slice(), format!("Writer {index}-{author}")]).unwrap();
                db.execute("INSERT INTO book_author VALUES(?1,?2)", params![index, author_id]).unwrap();
            }
            db.execute("INSERT INTO book_language VALUES(?1,'sv-SE'),(?1,'en-US')", [index]).unwrap();
            db.execute("INSERT INTO book_identifier VALUES(?1,12,'serial','value',2),(?1,1,'','9780306406157',0),(?1,12,'local','42',1)", [index]).unwrap();
            db.execute("INSERT INTO book_concept VALUES(?1,3915)", [index]).unwrap();
        }
        db
    }

    #[test]
    fn shared_line_catalog_records_remain_readable() {
        let db = fixture_with_schema(include_str!("../../tests/sql/catalog_shared_lines.sql"));
        let text = "雪\r\nShared line\n\nEmbedded\0NUL\n";
        db.execute("INSERT INTO description VALUES(2,?1)", [text]).unwrap();
        db.execute("UPDATE book SET description_id=2", []).unwrap();
        for index in 1..=2 {
            let book = load_entry(&db, CardKey([index; 32])).unwrap().unwrap();
            assert_eq!(book.record.description.as_bytes(), text.as_bytes());
            assert_eq!(book.record.book.subjects[0].code(), Some("concept:3915"));
        }
    }

    #[test]
    fn catalog_records_preserve_metadata_order_and_nullable_source_urls() {
        let db = fixture();
        for index in 1..=2 {
            let key = CardKey([index; 32]);
            let book = load_entry(&db, key).unwrap().unwrap();
            let external = index == 2;
            assert_eq!(book.hash, (!external).then(|| BookId::from_bytes(key.0)));
            assert_eq!(book.title, format!("Book {index}"));
            assert_eq!(book.subtitle.as_deref(), Some("Subtitle"));
            assert_eq!(book.record.description, "Shared description");
            assert_eq!(book.author, format!("Writer {index}-1, Writer {index}-2"));
            assert_eq!(book.record.contributors, [1, 2].map(|author| Contributor::with_id(Uuid::from_u128(u128::from(index) * 10 + author), format!("Writer {index}-{author}"), AUTHOR_MARC_RELATOR_CODE).unwrap()));
            assert_eq!(book.record.book.year, Some(1984));
            assert_eq!(
                book.record.book.identifiers,
                [
                    Identifier::new("9780306406157", Scheme::Isbn, Scope::Book).unwrap(),
                    Identifier::new("42", Scheme::Other("local".into()), Scope::Edition).unwrap(),
                    Identifier::new("value", Scheme::Other("serial".into()), Scope::Work).unwrap(),
                ]
            );
            assert_eq!(book.record.book.languages.iter().map(|language| language.as_str()).collect::<Vec<_>>(), ["en-US", "sv-SE"]);
            assert_eq!(book.languages, ["en", "sv"]);
            assert_eq!(book.record.book.subjects.len(), 1);
            assert_eq!(book.record.book.subjects[0].source(), CATALOG_CONCEPT_SOURCE);
            assert_eq!(book.record.book.subjects[0].code(), Some("concept:3915"));
            assert_eq!(book.external_url.as_deref(), external.then_some("https://example.org/book"));
            assert_eq!(book.download_url.as_deref(), external.then_some("https://example.org/audio.zip"));
            assert_eq!(book.format(), if external { BookFormat::Mp3Folder } else { BookFormat::Epub });
        }
        db.execute("UPDATE book SET entry_url=NULL,download_url=NULL WHERE external=1", []).unwrap();
        let book = load_entry(&db, CardKey([2; 32])).unwrap().unwrap();
        assert!(book.external_url.is_none());
        assert!(book.download_url.is_none());
        assert!(load_entry(&db, CardKey([9; 32])).unwrap().is_none());
    }

    #[test]
    fn catalog_source_credits_are_sorted_and_deduplicated_across_all_bindings() {
        let db = fixture();
        assert!(load_entry(&db, CardKey([1; 32])).unwrap().unwrap().sources.is_empty());
        db.execute_batch(
            "INSERT INTO source VALUES(2,'zebra.org'),(3,'alpha.org'),(4,'unbound.org');
            INSERT INTO book_source VALUES(1,1),(1,3),(2,1);
            UPDATE book SET source_id=2 WHERE row_id=2;
            INSERT INTO source_entry(source_id,entry_id,hosted_row_id,external_row_id) VALUES(1,1,1,2),(4,2,NULL,NULL);
            INSERT INTO source_entry_source VALUES(1,1,1),(1,1,3),(4,2,4);",
        )
        .unwrap();
        let all = read_sources(&db).unwrap();
        assert_eq!(all.len(), 2);
        for (index, expected) in [(1, vec!["alpha.org", "example.org"]), (2, vec!["alpha.org", "example.org", "zebra.org"])] {
            let key = CardKey([index; 32]);
            assert_eq!(load_entry(&db, key).unwrap().unwrap().sources, expected);
            assert_eq!(all[&key.0], expected);
        }
        // Invalid UTF-8 in a credited name must fail reads and recover after repair.
        db.execute_batch("UPDATE source SET name=CAST(X'80' AS TEXT) WHERE id=3").unwrap();
        assert!(read_sources(&db).is_err());
        assert!(load_entry(&db, CardKey([1; 32])).is_err());
        db.execute_batch("UPDATE source SET name='alpha.org' WHERE id=3").unwrap();
        assert_eq!(load_entry(&db, CardKey([1; 32])).unwrap().unwrap().sources, ["alpha.org", "example.org"]);
    }

    #[test]
    fn catalog_popularity_keeps_one_observation_for_each_bound_book() {
        let db = fixture();
        assert!(load_entry(&db, CardKey([1; 32])).unwrap().unwrap().popularity.is_none());
        db.execute_batch(
            "INSERT INTO source VALUES(2,'standardebooks.org'),(3,'litteraturbanken.se'),(4,'publicdomainlibrary.org');
            INSERT INTO source_entry(source_id,entry_id,hosted_row_id,external_row_id,ranking_source_id,score,popularity,epub_popularity,reading_percentile,epub_percentile) VALUES
            (1,1,1,NULL,1,50,999,NULL,NULL,NULL),
            (2,2,1,2,2,0.1,90,NULL,NULL,NULL),
            (2,3,1,2,2,0.25,7,NULL,NULL,NULL),
            (2,4,1,2,2,0.25,NULL,8,NULL,NULL),
            (3,5,1,2,3,99,999,999,99,99),
            (4,6,1,NULL,4,NULL,999,NULL,NULL,NULL);",
        )
        .unwrap();
        let expected = CatalogPopularity {
            popularity: Some(999),
            epub_popularity: Some(999),
            source: Some("litteraturbanken.se".into()),
            combined_popularity: Some(CombinedPopularity { score: Some(99.0), reading_percentile: Some(99.0), epub_percentile: Some(99.0) }),
            ..Default::default()
        };
        let all = read_popularity(&db).unwrap();
        assert_eq!(all.len(), 2);
        for index in [1, 2] {
            let key = CardKey([index; 32]);
            assert_eq!(load_entry(&db, key).unwrap().unwrap().popularity.as_ref(), Some(&expected));
            assert_eq!(all.get(&key.0), Some(&expected));
        }
        db.execute_batch("INSERT INTO source_entry(source_id,entry_id,external_row_id,ranking_source_id,score) VALUES(2,7,2,2,100)").unwrap();
        let external = CatalogPopularity { source_score: Some(100.0), source: Some("standardebooks.org".into()), ..Default::default() };
        let all = read_popularity(&db).unwrap();
        for (index, expected) in [(1, expected), (2, external)] {
            let key = CardKey([index; 32]);
            assert_eq!(load_entry(&db, key).unwrap().unwrap().popularity.as_ref(), Some(&expected));
            assert_eq!(all.get(&key.0), Some(&expected));
        }
        // Losing observations must still be validated; unrelated books stay readable.
        db.execute_batch("PRAGMA ignore_check_constraints=ON; UPDATE source_entry SET popularity=-1 WHERE source_id=4").unwrap();
        assert!(load_entry(&db, CardKey([1; 32])).is_err());
        assert!(read_popularity(&db).is_err());
        assert_eq!(load_entry(&db, CardKey([2; 32])).unwrap().unwrap().popularity.unwrap().score(), Some(100.0));
        db.execute_batch("UPDATE source_entry SET popularity=NULL WHERE source_id=4").unwrap();
        assert_eq!(load_entry(&db, CardKey([1; 32])).unwrap().unwrap().popularity.unwrap().score(), Some(99.0));
    }

    #[test]
    fn corrupt_catalog_metadata_is_rejected_and_cached_reads_can_recover() {
        for corruption in [
            "UPDATE book_identifier SET scheme=99 WHERE scheme=1",
            "UPDATE book_identifier SET scope=99 WHERE scheme=1",
            "UPDATE book_identifier SET value='' WHERE scheme=1",
            "UPDATE book_language SET language='invalid language!' WHERE language='en-US'",
        ] {
            let db = fixture();
            db.execute_batch("PRAGMA ignore_check_constraints=ON").unwrap();
            db.execute_batch(corruption).unwrap();
            assert!(load_entry(&db, CardKey([1; 32])).is_err(), "{corruption}");
            db.execute_batch("DELETE FROM book_identifier; DELETE FROM book_language").unwrap();
            let book = load_entry(&db, CardKey([1; 32])).unwrap().unwrap();
            assert!(book.record.book.identifiers.is_empty());
            assert!(book.record.book.languages.is_empty());
            assert_eq!(book.title, "Book 1");
        }
    }
}
