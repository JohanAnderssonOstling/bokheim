//! The curation database: every book in the catalog and where it came from.
//! Plain tables with JSON records, so viewers can read it directly. A book's
//! file is books/<hash>.epub and its cover covers/<hash>-{card,detail}.jpg.
//! A source-linked entry can have covers/<entry_key>-{card,detail}.jpg without
//! a retained book file.
use book_model::{AgentId, BookRecord, BookTocEntry, Contributor, AUTHOR_MARC_RELATOR_CODE};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::time::Duration;

mod authors;
pub use authors::AuthorIds;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS book(
  hash TEXT PRIMARY KEY,                 -- BLAKE3 of the file, the library's content hash
  title TEXT NOT NULL,                   -- copied from the record whenever it is written
  subtitle TEXT,
  author TEXT NOT NULL,                  -- authors as the library shows them, comma-separated
  record TEXT NOT NULL,                  -- the enriched BookRecord, JSON; curator subjects are edited into it
  toc TEXT                              -- navigation JSON; NULL until extracted, [] when the EPUB has none
) STRICT;
CREATE TABLE IF NOT EXISTS external_book(
  entry_key TEXT PRIMARY KEY,            -- opaque metadata identity, not a file content hash
  source TEXT NOT NULL,
  entry_id TEXT NOT NULL,
  entry_url TEXT NOT NULL,
  license TEXT NOT NULL,                 -- original source license identifier, for curation
  title TEXT NOT NULL,
  subtitle TEXT,
  author TEXT NOT NULL,
  record TEXT NOT NULL,
  file_url TEXT,                        -- acquisition link, never mirrored
  toc TEXT NOT NULL DEFAULT '[]',
  format INTEGER NOT NULL DEFAULT 1 CHECK(format IN (1,2,4,8,16)),
  UNIQUE(source,entry_id)
) STRICT;
CREATE TABLE IF NOT EXISTS book_source(
  hash TEXT NOT NULL REFERENCES book(hash) ON DELETE CASCADE,
  source TEXT NOT NULL,                  -- the host the book came from, or its inbox folder
  entry_id TEXT NOT NULL,                -- the OPDS entry id, or the inbox file name
  entry_url TEXT,                        -- the feed and file a downloaded book came from
  file_url TEXT,
  PRIMARY KEY(hash, source)
) STRICT;
CREATE TABLE IF NOT EXISTS source_entry(
  source TEXT NOT NULL,
  entry_id TEXT NOT NULL,
  entry_url TEXT NOT NULL,
  book_hash TEXT REFERENCES book(hash) ON DELETE SET NULL,
  external_entry_key TEXT REFERENCES external_book(entry_key) ON DELETE SET NULL,
  metadata TEXT NOT NULL,
  PRIMARY KEY(source,entry_id)
) STRICT, WITHOUT ROWID;
";

pub fn open(dir: &Path) -> Result<Connection, String> {
    std::fs::create_dir_all(dir.join("books")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dir.join("covers")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dir.join("inbox")).map_err(|e| e.to_string())?;
    let db = Connection::open(dir.join("curation.sqlite3")).map_err(|e| e.to_string())?;
    db.busy_timeout(Duration::from_secs(30)).map_err(|e| e.to_string())?;
    db.execute_batch("PRAGMA foreign_keys=ON;").map_err(|e| e.to_string())?;
    db.execute_batch(SCHEMA).map_err(|e| e.to_string())?;
    let has_toc: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('book') WHERE name='toc')", [], |row| row.get(0)).map_err(|e| e.to_string())?;
    if !has_toc {
        db.execute_batch("ALTER TABLE book ADD COLUMN toc TEXT;").map_err(|e| e.to_string())?;
    }
    let has_format: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('external_book') WHERE name='format')", [], |row| row.get(0)).map_err(|e| e.to_string())?;
    if !has_format {
        let tx = db.unchecked_transaction().map_err(|e| e.to_string())?;
        tx.execute_batch("ALTER TABLE external_book ADD COLUMN format INTEGER NOT NULL DEFAULT 1 CHECK(format IN (1,2,4,8,16));").map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
    }
    // Legacy workers can leave EPUB defaults even after the column exists.
    // Repair those from the rendition's retained audio evidence before publishing.
    db.execute(
        "UPDATE external_book SET format=16 WHERE format=1 AND EXISTS(
        SELECT 1 FROM source_entry WHERE external_entry_key=external_book.entry_key
        AND (json_extract(metadata,'$.audiobook_download_url') IS NOT NULL OR json_extract(metadata,'$.audiobook_delivery')='external_download')
    )",
        [],
    )
    .map_err(|e| e.to_string())?;
    authors::initialize(&db)?;
    Ok(db)
}

/// The one book whose hash starts with `prefix`.
pub fn resolve(db: &Connection, prefix: &str) -> Result<String, String> {
    let mut statement = db.prepare("SELECT hash FROM book WHERE hash LIKE ?1 || '%'").map_err(|e| e.to_string())?;
    let hashes = statement.query_map([prefix], |row| row.get::<_, String>(0)).map_err(|e| e.to_string())?.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    match hashes.as_slice() {
        [hash] => Ok(hash.clone()),
        [] => Err(format!("no book matches {prefix}")),
        _ => Err(format!("{prefix} matches {} books", hashes.len())),
    }
}

pub fn record(db: &Connection, hash: &str) -> Result<Option<BookRecord>, String> {
    let record: Option<String> = db.query_row("SELECT record FROM book WHERE hash=?1", [hash], |row| row.get(0)).optional().map_err(|e| e.to_string())?;
    record.map(|record| serde_json::from_str(&record).map_err(|e| e.to_string())).transpose()
}

/// Writes a book's record, its authors under the identities the catalog
/// already gives them.
pub fn set_record(db: &Connection, hash: &str, record: &BookRecord) -> Result<(), String> {
    let mut record = record.clone();
    AuthorIds::load(db)?.assign(&mut record)?;
    write_book(db, hash, &record)
}

/// Writes a book's record as the metadata server answered it: its authors
/// keep the server's identities, which every other book of theirs takes too.
pub fn set_enriched_record(db: &Connection, hash: &str, record: &BookRecord) -> Result<(), String> {
    AuthorIds::load(db)?.adopt(db, record)?;
    write_book(db, hash, record)
}

fn write_book(db: &Connection, hash: &str, record: &BookRecord) -> Result<(), String> {
    let json = serde_json::to_string(record).map_err(|e| e.to_string())?;
    db.execute(
        "INSERT INTO book(hash,title,subtitle,author,record) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(hash) DO UPDATE SET title=excluded.title,subtitle=excluded.subtitle,author=excluded.author,record=excluded.record",
        params![hash, record.title, record.subtitle, author(record), json],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Gives each author credit on `record` the identity `identity` chooses from
/// its name and current identity. Two spellings of one author on one book
/// become one credit. Returns whether anything changed.
fn reidentify(record: &mut BookRecord, mut identity: impl FnMut(&str, AgentId) -> AgentId) -> Result<bool, String> {
    let mut credited = Vec::new();
    let mut contributors = Vec::with_capacity(record.contributors.len());
    let mut changed = false;
    for credit in &record.contributors {
        if credit.role_code() != AUTHOR_MARC_RELATOR_CODE {
            contributors.push(credit.clone());
            continue;
        }
        let id = identity(credit.name(), credit.agent_id());
        if credited.contains(&id) {
            changed = true;
            continue;
        }
        credited.push(id);
        if id == credit.agent_id() {
            contributors.push(credit.clone());
        } else {
            changed = true;
            contributors.push(Contributor::with_id(id, credit.name(), credit.role_code()).map_err(|e| e.to_string())?);
        }
    }
    record.contributors = contributors;
    Ok(changed)
}

pub fn set_toc(db: &Connection, hash: &str, toc: &[BookTocEntry]) -> Result<(), String> {
    let json = serde_json::to_string(toc).map_err(|e| e.to_string())?;
    let updated = db.execute("UPDATE book SET toc=?2 WHERE hash=?1", params![hash, json]).map_err(|e| e.to_string())?;
    if updated != 1 {
        return Err(format!("no catalog book matches {hash}"));
    }
    Ok(())
}

pub fn toc(db: &Connection, hash: &str) -> Result<Vec<BookTocEntry>, String> {
    let json: Option<String> = db.query_row("SELECT toc FROM book WHERE hash=?1", [hash], |row| row.get(0)).map_err(|e| e.to_string())?;
    let json = json.ok_or_else(|| format!("{hash}: table of contents has not been extracted"))?;
    serde_json::from_str(&json).map_err(|e| format!("{hash}: invalid table of contents: {e}"))
}

/// Backfills navigation from already curated files without replacing enriched
/// metadata or fetching anything. An empty list is a completed extraction.
pub fn ensure_toc(dir: &Path, db: &Connection) -> Result<(), String> {
    let mut statement = db.prepare("SELECT hash FROM book WHERE toc IS NULL ORDER BY hash").map_err(|e| e.to_string())?;
    let hashes = statement.query_map([], |row| row.get::<_, String>(0)).map_err(|e| e.to_string())?.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    drop(statement);
    let mut extracted = Vec::new();
    for hash in hashes {
        let path = dir.join(format!("books/{hash}.epub"));
        let mut file = std::fs::File::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let actual = content_address::hash_reader(&mut file).map_err(|e| e.to_string())?;
        if actual.to_hex() != hash {
            return Err(format!("{} differs from the catalog's hash", path.display()));
        }
        let inspected = book_inspection::inspect_epub_metadata_with_toc(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        extracted.push((hash, inspected.toc));
    }
    let transaction = db.unchecked_transaction().map_err(|e| e.to_string())?;
    for (hash, toc) in &extracted {
        set_toc(&transaction, hash, toc)?;
    }
    transaction.commit().map_err(|e| e.to_string())?;
    if !extracted.is_empty() {
        eprintln!("Extracted table of contents for {} catalog books", extracted.len());
    }
    Ok(())
}

/// The authors as the library shows them on a card.
pub fn author(record: &BookRecord) -> String {
    record.authors().map(|author| author.name()).collect::<Vec<_>>().join(", ")
}

pub fn add_source(db: &Connection, hash: &str, source: &str, entry_id: &str, entry_url: Option<&str>, file_url: Option<&str>) -> Result<(), String> {
    db.execute(
        "INSERT INTO book_source(hash,source,entry_id,entry_url,file_url) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(hash,source) DO UPDATE SET entry_id=excluded.entry_id,entry_url=excluded.entry_url,file_url=excluded.file_url",
        params![hash, source, entry_id, entry_url, file_url],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn remove(db: &Connection, hash: &str) -> Result<(), String> {
    db.execute("DELETE FROM book WHERE hash=?1", [hash]).map_err(|e| e.to_string())?;
    Ok(())
}

/// Every book with its sources, in the order they were added.
pub fn books(db: &Connection) -> Result<Vec<(String, BookRecord, Vec<String>)>, String> {
    let mut statement = db.prepare("SELECT book.hash,book.record,group_concat(book_source.source,' ') FROM book LEFT JOIN book_source USING(hash) GROUP BY book.hash ORDER BY book.rowid").map_err(|e| e.to_string())?;
    let rows = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<String>>(2)?))).map_err(|e| e.to_string())?.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    rows.into_iter()
        .map(|(hash, record, sources)| {
            let record = serde_json::from_str(&record).map_err(|e| e.to_string())?;
            Ok((hash, record, sources.map(|sources| sources.split(' ').map(str::to_owned).collect()).unwrap_or_default()))
        })
        .collect()
}
