use catalog_model::{CatalogSourcedPublication, CatalogSubjectBrowseEntryPayload};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use std::collections::HashSet;

use crate::{client_store::ClientCatalogStore, CatalogClientError};

impl ClientCatalogStore {
    pub(super) fn missing_entry_ids(&self, epoch: &str, entry_ids: &[u64]) -> Result<Vec<u64>, CatalogClientError> {
        if entry_ids.len() > 256 {
            return Err(CatalogClientError::new("too many catalogue entry IDs"));
        }
        let connection = self.connection.lock().map_err(|_| CatalogClientError::new("catalogue subject payload cache lock was poisoned"))?;
        let mut available = connection.prepare_cached("SELECT EXISTS(SELECT 1 FROM catalog_subject_publication_cache WHERE epoch=?1 AND entry_id=?2)")?;
        let mut missing = Vec::new();
        for entry_id in entry_ids {
            if !available.query_row(params![epoch, stored_entry_id(*entry_id)?,], |row| row.get::<_, bool>(0))? {
                missing.push(*entry_id);
            }
        }
        Ok(missing)
    }

    pub(super) fn resolve(&self, epoch: &str, entry_ids: &[u64], payloads: &[CatalogSubjectBrowseEntryPayload]) -> Result<Option<Vec<CatalogSourcedPublication>>, CatalogClientError> {
        let entries = entry_ids.iter().copied().collect::<HashSet<_>>();
        if entries.len() != entry_ids.len() {
            return Err(CatalogClientError::new("catalogue returned duplicate subject entry IDs"));
        }
        let mut connection = self.connection.lock().map_err(|_| CatalogClientError::new("catalogue subject payload cache lock was poisoned"))?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute("DELETE FROM catalog_subject_publication_cache WHERE epoch<>?1", [epoch])?;
        for payload in payloads {
            if !entries.contains(&payload.entry_id) {
                return Err(CatalogClientError::new("catalogue returned a publication outside the requested subject range"));
            }
            transaction.execute(
                "INSERT INTO catalog_subject_publication_cache(epoch,entry_id,publication_payload)
                     VALUES(?1,?2,?3)
                     ON CONFLICT(epoch,entry_id) DO UPDATE SET publication_payload=excluded.publication_payload",
                params![epoch, stored_entry_id(payload.entry_id)?, wire::encode(&payload.publication).map_err(CatalogClientError::new)?],
            )?;
        }
        let publications = {
            let mut cached = transaction.prepare_cached("SELECT publication_payload FROM catalog_subject_publication_cache WHERE epoch=?1 AND entry_id=?2")?;
            entry_ids
                .iter()
                .map(|entry_id| {
                    let payload = cached.query_row(params![epoch, stored_entry_id(*entry_id)?], |row| row.get::<_, Vec<u8>>(0)).optional()?;
                    payload.map(|bytes| wire::decode::<CatalogSourcedPublication>(&bytes, wire::MAX_DECODED_RESPONSE_BYTES).map_err(CatalogClientError::new)).transpose()
                })
                .collect::<Result<Option<Vec<_>>, CatalogClientError>>()?
        };
        transaction.commit()?;
        Ok(publications)
    }
}

fn stored_entry_id(entry_id: u64) -> Result<i64, CatalogClientError> {
    i64::try_from(entry_id).map_err(|_| CatalogClientError::new("catalogue entry ID exceeds SQLite's integer range"))
}
