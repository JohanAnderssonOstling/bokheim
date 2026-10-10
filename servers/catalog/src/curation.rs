//! The curation database: every book in the catalog and where it came from.
//! Plain tables with JSON records, so viewers can read it directly. A book's
//! file is books/<hash>.epub and its cover covers/<hash>-{card,detail}.jpg.
use book_model::BookRecord;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS book(
  hash TEXT PRIMARY KEY,                 -- BLAKE3 of the file, the library's content hash
  title TEXT NOT NULL,                   -- copied from the record whenever it is written
  subtitle TEXT,
  author TEXT NOT NULL,                  -- authors as the library shows them, comma-separated
  record TEXT NOT NULL                   -- the enriched BookRecord, JSON; curator subjects are edited into it
) STRICT;
CREATE TABLE IF NOT EXISTS book_source(
  hash TEXT NOT NULL REFERENCES book(hash) ON DELETE CASCADE,
  source TEXT NOT NULL,                  -- the host the book came from, or its inbox folder
  entry_id TEXT NOT NULL,                -- the OPDS entry id, or the inbox file name
  entry_url TEXT,                        -- the feed and file a downloaded book came from
  file_url TEXT,
  PRIMARY KEY(hash, source)
) STRICT;
";

pub fn open(dir: &Path) -> Result<Connection, String> {
    std::fs::create_dir_all(dir.join("books")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dir.join("covers")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dir.join("inbox")).map_err(|e| e.to_string())?;
    let db = Connection::open(dir.join("curation.sqlite3")).map_err(|e| e.to_string())?;
    db.execute_batch("PRAGMA foreign_keys=ON;").map_err(|e| e.to_string())?;
    db.execute_batch(SCHEMA).map_err(|e| e.to_string())?;
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

pub fn set_record(db: &Connection, hash: &str, record: &BookRecord) -> Result<(), String> {
    let json = serde_json::to_string(record).map_err(|e| e.to_string())?;
    db.execute(
        "INSERT INTO book(hash,title,subtitle,author,record) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(hash) DO UPDATE SET title=excluded.title,subtitle=excluded.subtitle,author=excluded.author,record=excluded.record",
        params![hash, record.title, record.subtitle, author(record), json],
    )
    .map_err(|e| e.to_string())?;
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
    let rows = statement
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<String>>(2)?)))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    rows.into_iter()
        .map(|(hash, record, sources)| {
            let record = serde_json::from_str(&record).map_err(|e| e.to_string())?;
            Ok((hash, record, sources.map(|sources| sources.split(' ').map(str::to_owned).collect()).unwrap_or_default()))
        })
        .collect()
}
