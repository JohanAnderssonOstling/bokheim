use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn advance_catalog_transport_revision(connection: &Connection) -> Result<(), CatalogStoreError> {
    connection.execute("UPDATE catalog_transport_state SET revision=revision+1 WHERE singleton=1", [])?;
    Ok(())
}

pub(crate) fn unix_timestamp() -> Result<i64, CatalogStoreError> {
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|error| CatalogStoreError::new(error.to_string()))?.as_secs();
    storage_i64(timestamp, "Unix timestamp")
}

pub(crate) fn next_sequence_in(transaction: &Transaction<'_>, source_id: &CatalogSourceId) -> Result<u64, CatalogStoreError> {
    transaction.query_row("SELECT next_sequence FROM catalog_source WHERE source_id=?1", [source_id.as_str()], |row| stored_u64(row.get(0)?, "next sequence")).map_err(Into::into)
}

pub(crate) fn allocate_sequence(transaction: &Transaction<'_>, source_id: &CatalogSourceId) -> Result<i64, CatalogStoreError> {
    let next = next_sequence_in(transaction, source_id)?.checked_add(1).ok_or_else(|| CatalogStoreError::new("book sequence overflow"))?;
    transaction.execute("UPDATE catalog_source SET next_sequence=?2 WHERE source_id=?1", params![source_id.as_str(), storage_i64(next, "book sequence")?])?;
    storage_i64(next, "book sequence")
}

pub(crate) fn sequence_in(transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64, identity: &CatalogBookIdentity) -> Result<Option<u64>, CatalogStoreError> {
    transaction
        .query_row(
            "SELECT sequence FROM catalog_book WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4",
            params![source_id.as_str(), storage_i64(generation, "generation")?, identity.edition_id, identity.work_id.as_deref().unwrap_or("")],
            |row| stored_u64(row.get(0)?, "book sequence"),
        )
        .optional()
        .map_err(Into::into)
}

pub(crate) fn sequenced_book_in(connection: &Connection, source_id: &CatalogSourceId, generation: u64, identity: &CatalogBookIdentity) -> Result<Option<SequencedCatalogBook>, CatalogStoreError> {
    connection
        .query_row(
            "SELECT payload,sequence FROM catalog_book WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4",
            params![source_id.as_str(), storage_i64(generation, "generation")?, identity.edition_id, identity.work_id.as_deref().unwrap_or("")],
            |row| {
                let payload: Vec<u8> = row.get(0)?;
                let book = decode_cached(&payload).map_err(sql_decode_error)?;
                let sequence = stored_u64(row.get(1)?, "book sequence")?;
                Ok(SequencedCatalogBook { book, sequence })
            },
        )
        .optional()
        .map_err(Into::into)
}
