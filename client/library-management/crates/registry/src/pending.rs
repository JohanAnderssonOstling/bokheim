//! Typed access to durable account writes. Their storage format stays private.
use super::{LibraryEntry, LocalRegistry, RegistryResult};
use rusqlite::{params, OptionalExtension};
use sync_common::{api::libraries::CloudStorageChange, LibraryId};

#[derive(Clone)]
pub struct PendingCloudStorage {
    pub change: CloudStorageChange,
    token: Vec<u8>,
}

#[derive(Clone)]
pub struct CloudStorageReset(Vec<u8>);

impl LocalRegistry {
    pub fn cloud_storage_reset(&mut self, id: &LibraryId) -> RegistryResult<Option<CloudStorageReset>> {
        Ok(self.read_value(&format!("cloud-storage-reset:{id}"))?.map(CloudStorageReset))
    }

    pub fn acknowledge_cloud_storage_reset(&mut self, id: &LibraryId, reset: CloudStorageReset) -> RegistryResult<()> {
        self.clear_value_if_current(&format!("cloud-storage-reset:{id}"), &reset.0)
    }

    pub fn pending_library_name(&mut self, id: &LibraryId) -> RegistryResult<Option<String>> {
        self.read_value(&format!("pending-library-rename/{id}"))?.map(String::from_utf8).transpose().map_err(Into::into)
    }

    pub fn rename_library(&mut self, id: &LibraryId, name: &str) -> RegistryResult<LibraryEntry> {
        let existing = self.entry(id)?.ok_or("library not found")?;
        let entry = super::renamed_entry(&existing, name)?;
        let device_only = self.device_only(id)?;
        let tx = self.connection.transaction()?;
        tx.execute(include_str!("sql/update_library_name.sql"), params![id.to_string(), entry.library_name()])?;
        if !device_only {
            tx.execute(include_str!("sql/set_sync_disabled.sql"), params![format!("pending-library-rename/{id}"), entry.library_name().as_bytes()])?;
        }
        tx.commit()?;
        Ok(entry)
    }

    pub fn acknowledge_library_name(&mut self, id: &LibraryId, name: &str) -> RegistryResult<()> {
        self.clear_value_if_current(&format!("pending-library-rename/{id}"), name.as_bytes())
    }

    pub fn pending_cloud_storage(&mut self, id: &LibraryId) -> RegistryResult<Option<PendingCloudStorage>> {
        let Some(token) = self.read_value(&format!("pending-cloud-storage:{id}"))? else { return Ok(None) };
        let text = std::str::from_utf8(&token)?;
        let (change, value) = text.split_once(':').ok_or("invalid pending cloud storage change")?;
        let enabled = match value {
            "0" => false,
            "1" => true,
            _ => return Err("invalid pending cloud storage value".into()),
        };
        let change = CloudStorageChange { change_id: change.parse()?, enabled };
        Ok(Some(PendingCloudStorage { change, token }))
    }

    /// Do not acknowledge a newer intent queued while this request was running.
    pub fn acknowledge_cloud_storage(&mut self, id: &LibraryId, pending: PendingCloudStorage, enabled: bool) -> RegistryResult<bool> {
        self.entry(id)?.ok_or("library not found")?;
        let key = format!("pending-cloud-storage:{id}");
        let tx = self.connection.transaction()?;
        let current: Option<Vec<u8>> = tx.query_row("SELECT value FROM app_value WHERE key=?1", [&key], |row| row.get(0)).optional()?;
        if current.as_ref() != Some(&pending.token) {
            return Ok(false);
        }
        super::configuration::cache_cloud_storage(&tx, id, enabled)?;
        tx.execute("DELETE FROM app_value WHERE key=?1", params![key])?;
        tx.commit()?;
        Ok(true)
    }
}
