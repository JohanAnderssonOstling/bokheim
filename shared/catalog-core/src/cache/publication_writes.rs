use super::*;

/// Stores a publication payload with a sequence assigned by an authoritative
/// catalogue server. This is a storage primitive; response resolution belongs
/// to the client that received the payload.
pub fn upsert_publication_with_sequence(transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64, publication: &CatalogPublication, sequence: u64) -> Result<(), CatalogCacheError> {
    let payload = wire::encode(publication)?;
    let payload_hash = blake3::hash(&payload).to_hex().to_string();
    let inventory = publication_inventory(publication);
    let extraction = publication_extraction(publication);
    let (edition_id, work_id) = publication_key(publication);
    let sequence = storage_i64(sequence, "publication sequence")?;
    let entry_id = publication_entry_id(transaction, source_id, publication, &payload_hash)?;
    transaction.execute(
        "INSERT INTO catalog_publication(source_id, generation, edition_id, work_id, sequence, payload_hash, normalized_title, provider_popularity, book_format, importable, has_upstream_cover, inventory_indexed, acquisition_url, extraction_format, cover_url, payload, entry_id)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 1, ?12, ?13, ?14, ?15, ?16)
         ON CONFLICT(source_id, generation, edition_id, work_id) DO UPDATE SET entry_id=excluded.entry_id, sequence=excluded.sequence, payload_hash=excluded.payload_hash, normalized_title=excluded.normalized_title,
             provider_popularity=excluded.provider_popularity, book_format=excluded.book_format, importable=excluded.importable, has_upstream_cover=excluded.has_upstream_cover, inventory_indexed=1,
             acquisition_url=excluded.acquisition_url, extraction_format=excluded.extraction_format, cover_url=excluded.cover_url, payload=excluded.payload",
        params![
            source_id.as_str(),
            storage_i64(generation, "generation")?,
            edition_id,
            work_id,
            sequence,
            payload_hash,
            normalize_search_text(&publication.title),
            0,
            inventory.book_format,
            inventory.importable,
            inventory.has_upstream_cover,
            extraction.as_ref().map(|item| item.acquisition_url),
            extraction.as_ref().map(|item| book_format_storage(item.book_format)),
            publication.image_url,
            payload,
            entry_id
        ],
    )?;
    transaction.execute("UPDATE catalog_source SET next_sequence=MAX(next_sequence, ?2) WHERE source_id=?1", params![source_id.as_str(), sequence])?;
    replace_publication_metadata(transaction, source_id, generation, publication)?;
    Ok(())
}
