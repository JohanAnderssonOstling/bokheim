use super::{params, CatalogCache, CatalogCacheError, TransactionBehavior};

pub const CATALOGUE_CRAWLING_PROCESS: &str = "catalogue_crawling";
pub const COVER_DOWNLOADS_PROCESS: &str = "cover_downloads";
pub const EPUB_ENRICHMENT_PROCESS: &str = "epub_enrichment";
pub const CATALOGUE_ENRICHMENT_PROCESS: &str = "catalogue_enrichment";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CatalogProcessSettings {
    pub catalogue_crawling: bool,
    pub cover_downloads: bool,
    pub epub_enrichment: bool,
    pub catalogue_enrichment: bool,
}

impl Default for CatalogProcessSettings {
    fn default() -> Self {
        Self { catalogue_crawling: false, cover_downloads: false, epub_enrichment: false, catalogue_enrichment: false }
    }
}

impl CatalogCache {
    pub fn process_settings(&self) -> Result<CatalogProcessSettings, CatalogCacheError> {
        let connection = self.pool.get()?;
        Ok(CatalogProcessSettings {
            catalogue_crawling: process_enabled(&connection, CATALOGUE_CRAWLING_PROCESS)?,
            cover_downloads: process_enabled(&connection, COVER_DOWNLOADS_PROCESS)?,
            epub_enrichment: process_enabled(&connection, EPUB_ENRICHMENT_PROCESS)?,
            catalogue_enrichment: process_enabled(&connection, CATALOGUE_ENRICHMENT_PROCESS)?,
        })
    }

    pub fn set_process_enabled(&self, process: &str, enabled: bool) -> Result<(), CatalogCacheError> {
        if !matches!(process, CATALOGUE_CRAWLING_PROCESS | COVER_DOWNLOADS_PROCESS | EPUB_ENRICHMENT_PROCESS | CATALOGUE_ENRICHMENT_PROCESS) {
            return Err(CatalogCacheError::new("unknown catalogue process"));
        }
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = transaction.execute("UPDATE catalog_process_control SET enabled=?2 WHERE process=?1", params![process, enabled])?;
        if changed != 1 {
            return Err(CatalogCacheError::new("catalogue process control is unavailable"));
        }
        transaction.commit()?;
        Ok(())
    }

    /// Every service boot starts from a quiescent state. Administrators must
    /// explicitly resume each worker after checking the current deployment.
    pub fn pause_all_processes(&self) -> Result<(), CatalogCacheError> {
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute("UPDATE catalog_process_control SET enabled=0", [])?;
        transaction.commit()?;
        Ok(())
    }
}

fn process_enabled(connection: &rusqlite::Connection, process: &str) -> Result<bool, CatalogCacheError> {
    connection.query_row("SELECT enabled FROM catalog_process_control WHERE process=?1", [process], |row| row.get(0)).map_err(Into::into)
}
