use super::*;

impl CatalogStore {
    pub(crate) fn source_file_references(&self, source: &CatalogSourceId) -> Result<(Vec<String>,Vec<String>,Vec<String>),CatalogStoreError> {
        let connection=self.connection()?;
        let mut staged=connection.prepare("SELECT DISTINCT staged_path FROM catalog_epub_extraction p WHERE source_id=?1 AND staged_path IS NOT NULL
            AND NOT EXISTS(SELECT 1 FROM catalog_epub_extraction other WHERE other.source_id!=?1 AND other.staged_path=p.staged_path)")?;
        let paths=staged.query_map([source.as_str()], |r| r.get(0))?.collect::<Result<Vec<_>,_>>()?;
        let mut own=connection.prepare("SELECT image_url FROM catalog_cover_download WHERE source_id=?1 UNION SELECT cover_url FROM catalog_publication WHERE source_id=?1 AND cover_url IS NOT NULL")?;
        let urls=own.query_map([source.as_str()], |r| r.get(0))?.collect::<Result<Vec<_>,_>>()?;
        let mut others=connection.prepare("SELECT image_url FROM catalog_cover_download WHERE source_id!=?1 UNION SELECT cover_url FROM catalog_publication WHERE source_id!=?1 AND cover_url IS NOT NULL")?;
        let retained=others.query_map([source.as_str()], |r| r.get(0))?.collect::<Result<Vec<_>,_>>()?;
        Ok((paths,urls,retained))
    }
    /// Ensures an authoritative crawl source exists. A URL change invalidates
    /// its canonical rows and advances the server transport epoch.
    pub fn ensure_source(&self, source_id: &CatalogSourceId, source_url: &str) -> Result<CatalogSourceState, CatalogStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_source_in(&transaction, source_id, source_url)?;
        let state = source_state_in(&transaction, source_id)?.expect("source inserted");
        transaction.commit()?;
        Ok(state)
    }

    pub fn remove_source(&self, source_id: &CatalogSourceId) -> Result<(), CatalogStoreError> {
        let _projector = self.serving.exclusive()?;
        // Purge serving rows first. If authority deletion fails, its source row
        // survives so startup reconciliation retries the whole operation.
        let mut serving = self.serving.connection()?;
        let serving_transaction = serving.transaction_with_behavior(TransactionBehavior::Immediate)?;
        super::serving_projection::purge_source(&serving_transaction,source_id.as_str(),true)?;
        serving_transaction.commit()?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        super::serving_projection::purge_source(&transaction,source_id.as_str(),false)?;
        transaction.execute("DELETE FROM catalog_source WHERE source_id=?1", [source_id.as_str()])?;
        transaction.execute("DELETE FROM catalog_serving_dirty WHERE source_id=?1", [source_id.as_str()])?;
        transaction.execute("DELETE FROM catalog_serving_dirty_source WHERE source_id=?1", [source_id.as_str()])?;
        advance_catalog_transport_revision(&transaction)?;
        transaction.commit()?;
        Ok(())
    }
}

pub(crate) fn source_state_in(connection: &Connection, source_id: &CatalogSourceId) -> Result<Option<CatalogSourceState>, CatalogStoreError> {
    connection
        .query_row("SELECT epoch,active_generation FROM catalog_source WHERE source_id=?1", [source_id.as_str()], |row| Ok(CatalogSourceState { epoch: row.get(0)?, active_generation: stored_u64(row.get(1)?, "active generation")? }))
        .optional()
        .map_err(Into::into)
}

pub(crate) fn ensure_source_in(transaction: &Transaction<'_>, source_id: &CatalogSourceId, source_url: &str) -> Result<(), CatalogStoreError> {
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

fn insert_source(transaction: &Transaction<'_>, source_id: &CatalogSourceId, source_url: &str) -> Result<(), CatalogStoreError> {
    transaction.execute("INSERT INTO catalog_source(source_id,source_url,epoch,active_generation,next_sequence) VALUES(?1,?2,lower(hex(randomblob(16))),1,0)", params![source_id.as_str(), source_url])?;
    Ok(())
}

pub(crate) fn refresh_active_counts(transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64) -> Result<(), CatalogStoreError> {
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

pub(crate) fn active_generation_in(transaction: &Transaction<'_>, source_id: &CatalogSourceId) -> Result<u64, CatalogStoreError> {
    transaction.query_row("SELECT active_generation FROM catalog_source WHERE source_id=?1", [source_id.as_str()], |row| stored_u64(row.get(0)?, "active generation")).map_err(Into::into)
}

pub(crate) fn active_generation_optional(connection: &Connection, source_id: &CatalogSourceId) -> Result<Option<u64>, CatalogStoreError> {
    connection.query_row("SELECT active_generation FROM catalog_source WHERE source_id=?1", [source_id.as_str()], |row| stored_u64(row.get(0)?, "active generation")).optional().map_err(Into::into)
}
