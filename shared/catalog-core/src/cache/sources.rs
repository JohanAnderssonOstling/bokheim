use super::{advance_catalog_transport_revision, params, storage_i64, stored_u64, CatalogCache, CatalogCacheError, CatalogSourceId, CatalogSourceState, Connection, OptionalExtension, Transaction, TransactionBehavior, Uuid};

impl CatalogCache {
    pub fn source_state(&self, source_id: &CatalogSourceId) -> Result<Option<CatalogSourceState>, CatalogCacheError> {
        let connection = self.pool.get()?;
        connection
            .query_row("SELECT epoch, provider_epoch, active_generation FROM catalog_source WHERE source_id=?1", [source_id.as_str()], |row| {
                Ok(CatalogSourceState { epoch: row.get(0)?, provider_epoch: row.get(1)?, active_generation: stored_u64(row.get(2)?, "active generation")? })
            })
            .optional()
            .map_err(Into::into)
    }

    /// Ensures a source exists. A URL change invalidates every canonical row
    /// for that source and creates a new epoch.
    pub fn ensure_source(&self, source_id: &CatalogSourceId, source_url: &str) -> Result<CatalogSourceState, CatalogCacheError> {
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_source_in(&transaction, source_id, source_url)?;
        let state = source_state_in(&transaction, source_id)?.expect("source inserted");
        transaction.commit()?;
        Ok(state)
    }

    pub fn set_provider_epoch(&self, source_id: &CatalogSourceId, source_url: &str, provider_epoch: &str) -> Result<bool, CatalogCacheError> {
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_source_in(&transaction, source_id, source_url)?;
        let previous = transaction.query_row("SELECT provider_epoch FROM catalog_source WHERE source_id=?1", [source_id.as_str()], |row| row.get::<_, Option<String>>(0))?;
        let changed = previous.as_deref().is_some_and(|previous| previous != provider_epoch);
        if changed {
            clear_source_rows(&transaction, source_id)?;
        }
        transaction.execute("UPDATE catalog_source SET provider_epoch=?2 WHERE source_id=?1", params![source_id.as_str(), provider_epoch])?;
        transaction.commit()?;
        Ok(changed)
    }

    pub fn remove_source(&self, source_id: &CatalogSourceId) -> Result<(), CatalogCacheError> {
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute("DELETE FROM catalog_source WHERE source_id=?1", [source_id.as_str()])?;
        advance_catalog_transport_revision(&transaction)?;
        transaction.commit()?;
        Ok(())
    }
}

pub fn ensure_source_in(transaction: &Transaction<'_>, source_id: &CatalogSourceId, source_url: &str) -> Result<(), CatalogCacheError> {
    let existing = transaction.query_row("SELECT source_url FROM catalog_source WHERE source_id=?1", [source_id.as_str()], |row| row.get::<_, String>(0)).optional()?;
    match existing {
        Some(existing) if existing == source_url => {}
        Some(_) => {
            transaction.execute("DELETE FROM catalog_source WHERE source_id=?1", [source_id.as_str()])?;
            insert_source(transaction, source_id, source_url)?;
            advance_catalog_transport_revision(transaction)?;
        }
        None => insert_source(transaction, source_id, source_url)?,
    }
    Ok(())
}

pub(crate) fn insert_source(transaction: &Transaction<'_>, source_id: &CatalogSourceId, source_url: &str) -> Result<(), CatalogCacheError> {
    transaction.execute("INSERT INTO catalog_source(source_id, source_url, epoch, provider_epoch, active_generation, next_sequence) VALUES(?1, ?2, ?3, NULL, 1, 0)", params![source_id.as_str(), source_url, Uuid::new_v4().to_string()])?;
    Ok(())
}

pub fn clear_source_rows(transaction: &Transaction<'_>, source_id: &CatalogSourceId) -> Result<(), CatalogCacheError> {
    transaction.execute("DELETE FROM catalog_page WHERE source_id=?1", [source_id.as_str()])?;
    transaction.execute("DELETE FROM catalog_publication WHERE source_id=?1", [source_id.as_str()])?;
    transaction.execute(
        "UPDATE catalog_source SET epoch=?2, active_generation=1, next_sequence=0, crawl_status='never', last_crawl_error=NULL,
             active_page_count=0, active_publication_count=0, active_epub_count=0
         WHERE source_id=?1",
        params![source_id.as_str(), Uuid::new_v4().to_string()],
    )?;
    advance_catalog_transport_revision(transaction)?;
    Ok(())
}

pub fn refresh_active_counts(transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64) -> Result<(), CatalogCacheError> {
    let generation = storage_i64(generation, "generation")?;
    transaction.execute(
        "UPDATE catalog_source
         SET active_page_count=(SELECT COUNT(*) FROM catalog_page WHERE source_id=?1 AND generation=?2),
             active_publication_count=(SELECT COUNT(*) FROM catalog_publication WHERE source_id=?1 AND generation=?2),
             active_epub_count=(SELECT COUNT(*) FROM catalog_publication WHERE source_id=?1 AND generation=?2 AND book_format='epub' AND importable=1)
         WHERE source_id=?1",
        params![source_id.as_str(), generation],
    )?;
    Ok(())
}

pub fn source_state_in(transaction: &Transaction<'_>, source_id: &CatalogSourceId) -> Result<Option<CatalogSourceState>, CatalogCacheError> {
    transaction
        .query_row("SELECT epoch, provider_epoch, active_generation FROM catalog_source WHERE source_id=?1", [source_id.as_str()], |row| {
            Ok(CatalogSourceState { epoch: row.get(0)?, provider_epoch: row.get(1)?, active_generation: stored_u64(row.get(2)?, "active generation")? })
        })
        .optional()
        .map_err(Into::into)
}

pub fn active_generation_in(transaction: &Transaction<'_>, source_id: &CatalogSourceId) -> Result<u64, CatalogCacheError> {
    transaction.query_row("SELECT active_generation FROM catalog_source WHERE source_id=?1", [source_id.as_str()], |row| stored_u64(row.get(0)?, "active generation")).map_err(Into::into)
}

pub fn active_generation_optional(connection: &Connection, source_id: &CatalogSourceId) -> Result<Option<u64>, CatalogCacheError> {
    connection.query_row("SELECT active_generation FROM catalog_source WHERE source_id=?1", [source_id.as_str()], |row| stored_u64(row.get(0)?, "active generation")).optional().map_err(Into::into)
}

pub fn next_sequence_in(transaction: &Transaction<'_>, source_id: &CatalogSourceId) -> Result<u64, CatalogCacheError> {
    transaction.query_row("SELECT next_sequence FROM catalog_source WHERE source_id=?1", [source_id.as_str()], |row| stored_u64(row.get(0)?, "next sequence")).map_err(Into::into)
}
