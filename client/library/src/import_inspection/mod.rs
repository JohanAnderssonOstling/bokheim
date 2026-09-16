//! Device-local proof that an import's bytes were successfully inspected.
use rusqlite::Connection;
use sync_common::ContentHash;

pub fn has_import_inspection(conn: &Connection, hash: &ContentHash, format: library_model::BookFormat) -> rusqlite::Result<bool> {
    conn.query_row(include_str!("sql/import_inspection/has_import_inspection_select.sql"), rusqlite::params![hash.as_str(), format.canonical_extension(), inspection_version(format)], |row| row.get(0))
}

pub fn record_import_inspection(conn: &Connection, hash: &ContentHash, format: library_model::BookFormat) -> rusqlite::Result<()> {
    conn.execute(include_str!("sql/import_inspection/record_import_inspection_insert.sql"), rusqlite::params![hash.as_str(), format.canonical_extension(), inspection_version(format)])?;
    Ok(())
}

// Invalidate cached inspections when the shared evidence extractor changes.
fn inspection_version(format: library_model::BookFormat) -> i64 {
    match format {
        library_model::BookFormat::Epub | library_model::BookFormat::Pdf => i64::from(publication_inspection::evidence::INSPECTION_VERSION),
        _ => 1,
    }
}
