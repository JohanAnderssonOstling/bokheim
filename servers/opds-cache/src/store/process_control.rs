use super::{params, CatalogStore, CatalogStoreError, TransactionBehavior};

pub const CATALOGUE_CRAWLING_PROCESS: &str = "catalogue_crawling";
pub const COVER_DOWNLOADS_PROCESS: &str = "cover_downloads";
pub const ENRICHMENT_PROCESS: &str = "enrichment";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CatalogProcessSettings {
    pub catalogue_crawling: bool,
    pub cover_downloads: bool,
    pub enrichment: bool,
}

impl Default for CatalogProcessSettings {
    fn default() -> Self {
        Self { catalogue_crawling: false, cover_downloads: false, enrichment: false }
    }
}

impl CatalogStore {
    pub fn process_settings(&self) -> Result<CatalogProcessSettings, CatalogStoreError> {
        let connection = self.connection()?;
        Ok(CatalogProcessSettings {
            catalogue_crawling: process_enabled(&connection, CATALOGUE_CRAWLING_PROCESS)?,
            cover_downloads: process_enabled(&connection, COVER_DOWNLOADS_PROCESS)?,
            enrichment: process_enabled(&connection, ENRICHMENT_PROCESS)?,
        })
    }

    pub fn set_process_enabled(&self, process: &str, enabled: bool) -> Result<(), CatalogStoreError> {
        if !matches!(process, CATALOGUE_CRAWLING_PROCESS | COVER_DOWNLOADS_PROCESS | ENRICHMENT_PROCESS) {
            return Err(CatalogStoreError::new("unknown catalogue process"));
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = transaction.execute("UPDATE catalog_process_control SET enabled=?2 WHERE process=?1", params![process, enabled])?;
        if changed != 1 {
            return Err(CatalogStoreError::new("catalogue process control is unavailable"));
        }
        transaction.commit()?;
        Ok(())
    }

    /// Every service boot starts from a quiescent state. Administrators must
    /// explicitly resume each worker after checking the current deployment.
    pub fn pause_all_processes(&self) -> Result<(), CatalogStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute("UPDATE catalog_process_control SET enabled=0", [])?;
        transaction.commit()?;
        Ok(())
    }
}

fn process_enabled(connection: &rusqlite::Connection, process: &str) -> Result<bool, CatalogStoreError> {
    connection.query_row("SELECT enabled FROM catalog_process_control WHERE process=?1", [process], |row| row.get(0)).map_err(Into::into)
}
