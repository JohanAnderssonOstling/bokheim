//! The public-domain catalog. The host stores the snapshot the signed update
//! manifest names and activates it once per process; activation reads every
//! book and places it in the installed taxonomy, as a library book is placed.
//! Catalog pages browse these books in memory: a catalog holds thousands of
//! books, not a library's worth.
use crate::DatabaseError;
use content_address::ContentHash;
use rusqlite::{Connection, OpenFlags};
use std::path::Path;
use std::sync::OnceLock;

static BOOKS: OnceLock<Vec<CatalogBook>> = OnceLock::new();

/// One catalog book, with what browsing filters, sorts and shows, and the
/// record a library adding it records as its enrichment answer.
pub struct CatalogBook {
    pub hash: ContentHash,
    pub title: String,
    pub subtitle: Option<String>,
    pub author: String,
    pub search_title: String,
    pub search_author: String,
    /// Base language codes, as library books store them.
    pub languages: Vec<String>,
    /// The taxonomy concepts its subject codes project to.
    pub concepts: Vec<i64>,
    pub record: book_model::BookRecord,
}

/// A stored snapshot with the release and format it declares.
pub struct StoredCatalog {
    pub release: u64,
    pub format: u32,
    pub image: Vec<u8>,
}

/// The active catalog's books; empty until one is activated.
pub fn books() -> &'static [CatalogBook] {
    BOOKS.get().map_or(&[], Vec::as_slice)
}

pub fn activate(image: Vec<u8>) -> Result<(), DatabaseError> {
    let mut db = Connection::open_in_memory()?;
    db.deserialize_read_exact("main", image.as_slice(), image.len(), true)?;
    let mut statement = db.prepare("SELECT hash,title,subtitle,author,record FROM book ORDER BY hash")?;
    let rows = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<String>>(2)?, row.get::<_, String>(3)?, row.get::<_, String>(4)?)))?;
    let mut books = Vec::new();
    for row in rows {
        let (hash, title, subtitle, author, record) = row?;
        let hash = hash.parse::<ContentHash>().map_err(|_| DatabaseError::message("catalog book has an invalid hash"))?;
        let record: book_model::BookRecord = serde_json::from_str(&record).map_err(DatabaseError::operation)?;
        let mut concepts = crate::sync::apply::project_book(&record.book)?.concepts.into_iter().map(|(_, concept)| concept).collect::<Vec<_>>();
        concepts.sort_unstable();
        concepts.dedup();
        let languages = record.book.languages.iter().map(|language| crate::browse::options::base_language(language.as_str())).filter(|language| !language.is_empty()).collect();
        let search_title = crate::browse::text::search_title(Some(&title), subtitle.as_deref());
        let search_author = book_model::normalize_search_text(&author);
        books.push(CatalogBook { hash, title, subtitle, author, search_title, search_author, languages, concepts, record });
    }
    BOOKS.set(books).map_err(|_| DatabaseError::message("the catalog can only be activated once per process"))
}

/// The snapshot stored at `path`, or none when nothing is stored there.
pub fn load(path: &Path) -> Result<Option<StoredCatalog>, DatabaseError> {
    let Ok(db) = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY) else { return Ok(None) };
    let Ok((release, format)) = release(&db) else { return Ok(None) };
    let image = db.serialize("main")?.to_vec();
    Ok(Some(StoredCatalog { release, format, image }))
}

/// Replaces the snapshot at `path` in one transaction, after checking it is
/// the release and format the signed manifest names.
pub fn store(image: &[u8], path: &Path, expected: (u64, u32)) -> Result<(), DatabaseError> {
    let mut source = Connection::open_in_memory()?;
    source.deserialize_read_exact("main", image, image.len(), true)?;
    if release(&source)? != expected {
        return Err(DatabaseError::message("catalog release differs from the signed manifest"));
    }
    let mut destination = Connection::open(path)?;
    let backup = rusqlite::backup::Backup::new(&source, &mut destination)?;
    match backup.step(-1)? {
        rusqlite::backup::StepResult::Done => Ok(()),
        _ => Err(DatabaseError::message("the stored catalog is busy")),
    }
}

fn release(db: &Connection) -> rusqlite::Result<(u64, u32)> {
    db.query_row("SELECT release_id, format_version FROM snapshot WHERE singleton=1", [], |row| Ok((row.get::<_, i64>(0)? as u64, row.get(1)?)))
}
