//! Device-local proof that an import's bytes were successfully inspected.
use include_sqlite_sql::include_sql;
use rusqlite::Connection;
use sync_common::ContentHash;

pub fn has_import_inspection(conn: &Connection, hash: &ContentHash, format: book_model::BookFormat) -> rusqlite::Result<bool> {
    conn.has_import_inspection(hash.as_str(), format.canonical_extension(), inspection_version(format), |row| row.get(0))
}

pub fn record_import_inspection(conn: &Connection, hash: &ContentHash, format: book_model::BookFormat) -> rusqlite::Result<()> {
    conn.record_import_inspection(hash.as_str(), format.canonical_extension(), inspection_version(format))?;
    Ok(())
}

// Invalidate cached inspections when the shared evidence extractor changes.
fn inspection_version(format: book_model::BookFormat) -> i64 {
    match format {
        book_model::BookFormat::Epub | book_model::BookFormat::Pdf => i64::from(book_inspection::evidence::INSPECTION_VERSION),
        _ => 1,
    }
}

include_sql!("src/import_inspection/sql/import_inspection.sql");
