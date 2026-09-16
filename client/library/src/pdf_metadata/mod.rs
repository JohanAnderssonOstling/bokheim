//! Device database ingestion output, selected by exact transport checksum.
use pdf_view_common::PdfReaderMetadata;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use sync_common::ContentHash;

pub fn upsert(conn: &Transaction<'_>, hash: &ContentHash, checksum: &ContentHash, metadata: &PdfReaderMetadata) -> rusqlite::Result<()> {
    if !metadata.valid_for(checksum.as_str()) {
        return Err(rusqlite::Error::InvalidParameterName("PDF ingestion checksum or metadata is invalid".into()));
    }
    let bytes = serde_json::to_vec(metadata).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    conn.execute(include_str!("sql/pdf_metadata/upsert_insert.sql"), params![hash.as_str(), checksum.as_str(), bytes])?;
    Ok(())
}

pub fn get(conn: &Connection, hash: &ContentHash, checksum: &ContentHash) -> rusqlite::Result<Option<PdfReaderMetadata>> {
    let bytes: Option<Vec<u8>> = conn.query_row(include_str!("sql/pdf_metadata/get_select.sql"), params![hash.as_str(), checksum.as_str()], |r| r.get(0)).optional()?;
    Ok(bytes.and_then(|b| serde_json::from_slice::<PdfReaderMetadata>(&b).ok()).filter(|m| m.valid_for(checksum.as_str())))
}
