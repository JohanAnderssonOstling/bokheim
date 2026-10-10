//! Builds the snapshot clients attach, and the files they download.
use crate::curation;
use rusqlite::{params, Connection};
use std::collections::BTreeMap;
use std::path::Path;

pub const FORMAT_VERSION: u32 = 1;

const SCHEMA: &str = "
CREATE TABLE snapshot(singleton INTEGER PRIMARY KEY CHECK(singleton=1), format_version INTEGER NOT NULL, release_id INTEGER NOT NULL) STRICT;
CREATE TABLE source(id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE) STRICT;
CREATE TABLE book(
  hash TEXT PRIMARY KEY,                 -- books/<hash>.epub, covers/<hash>-{card,detail}.jpg
  title TEXT NOT NULL,                   -- copied from the record for the card
  subtitle TEXT,
  author TEXT NOT NULL,                  -- authors as the library shows them, comma-separated
  record TEXT NOT NULL,                  -- the enriched BookRecord, JSON; recorded as the book's enrichment answer
  file_bytes INTEGER NOT NULL
) STRICT, WITHOUT ROWID;
CREATE TABLE book_source(hash TEXT NOT NULL REFERENCES book(hash), source_id INTEGER NOT NULL REFERENCES source(id), PRIMARY KEY(hash, source_id)) STRICT, WITHOUT ROWID;
";

pub fn publish(dir: &Path, db: &Connection, out: &Path, release_id: u64) -> Result<(), String> {
    if release_id == 0 {
        return Err("RELEASE_ID must be a positive integer".into());
    }
    let path = out.join(format!("releases/{release_id}/catalog.sqlite3"));
    if path.exists() {
        return Err(format!("{} already exists; releases are immutable", path.display()));
    }
    std::fs::create_dir_all(out.join(format!("releases/{release_id}"))).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(out.join("books")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(out.join("covers")).map_err(|e| e.to_string())?;
    let books = curation::books(db)?;
    let snapshot = Connection::open(&path).map_err(|e| e.to_string())?;
    snapshot.execute_batch(SCHEMA).map_err(|e| e.to_string())?;
    snapshot.execute("INSERT INTO snapshot VALUES(1,?1,?2)", params![FORMAT_VERSION, release_id as i64]).map_err(|e| e.to_string())?;
    let mut sources = BTreeMap::new();
    let mut total_file_bytes = 0;
    for (hash, record, book_sources) in &books {
        for file in [format!("books/{hash}.epub"), format!("covers/{hash}-card.jpg"), format!("covers/{hash}-detail.jpg")] {
            let (from, to) = (dir.join(&file), out.join(&file));
            if from.exists() && !to.exists() {
                std::fs::copy(&from, &to).map_err(|e| format!("{}: {e}", from.display()))?;
            }
        }
        let file_bytes = std::fs::metadata(dir.join(format!("books/{hash}.epub"))).map_err(|e| e.to_string())?.len();
        let json = serde_json::to_string(record).map_err(|e| e.to_string())?;
        snapshot
            .execute("INSERT INTO book(hash,title,subtitle,author,record,file_bytes) VALUES(?1,?2,?3,?4,?5,?6)", params![hash, record.title, record.subtitle, curation::author(record), json, file_bytes as i64])
            .map_err(|e| e.to_string())?;
        for name in book_sources {
            let next = sources.len() as i64 + 1;
            let id = *sources.entry(name.clone()).or_insert(next);
            snapshot.execute("INSERT OR IGNORE INTO source(id,name) VALUES(?1,?2)", params![id, name]).map_err(|e| e.to_string())?;
            snapshot.execute("INSERT INTO book_source(hash,source_id) VALUES(?1,?2)", params![hash, id]).map_err(|e| e.to_string())?;
        }
        total_file_bytes += file_bytes;
    }
    snapshot.execute_batch("VACUUM;").map_err(|e| e.to_string())?;
    drop(snapshot);
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    let size = bytes.len() as u64;
    eprintln!("{}: {} books, {} bytes ({} per book); {} bytes of books", path.display(), books.len(), size, size / books.len().max(1) as u64, total_file_bytes);
    // The manifest's catalog entry, for the release that publishes OUT_DIR at
    // the catalog origin.
    let origin = std::env::var("BOKHEIM_CATALOG_ORIGIN").unwrap_or_else(|_| "https://catalog.bokheim.se".into());
    let sha256 = ring::digest::digest(&ring::digest::SHA256, &bytes).as_ref().iter().map(|byte| format!("{byte:02x}")).collect();
    let artifact = update_protocol::Artifact { url: format!("{origin}/releases/{release_id}/catalog.sqlite3"), bytes: size, sha256 };
    let release = update_protocol::CatalogRelease { release_id, format_version: FORMAT_VERSION, artifact };
    println!("{}", serde_json::to_string_pretty(&release).map_err(|e| e.to_string())?);
    Ok(())
}
