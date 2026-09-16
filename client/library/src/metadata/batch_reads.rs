//! Snapshot reads after lifecycle UPSERTs, before independent metadata writes.
use super::*;

pub(crate) fn update_independent_metadata(tx: &Transaction, incoming: &[(ContentHash, &SyncBookMetadata)]) -> Result<(), Box<dyn std::error::Error>> {
    for page in incoming.chunks(100) {
        let keys = serde_json::to_string(&page.iter().map(|(hash, _)| hash.as_str()).collect::<Vec<_>>())?;
        let previous = tx
            .prepare_cached(include_str!("sql/publication_batch_select.sql"))?
            .query_map([keys], |row| {
                let raw: Option<Vec<u8>> = row.get(0)?;
                Ok(raw.and_then(|raw| crate::external_metadata::decode_publication_metadata(&raw).ok()))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let matches = crate::authors::contributors_match_batch(tx, page)?;
        for (((hash, metadata), previous), matches) in page.iter().zip(previous).zip(matches) {
            update_book_metadata_with_snapshot(tx, *hash, metadata, Some((previous, matches)))?;
        }
    }
    Ok(())
}
