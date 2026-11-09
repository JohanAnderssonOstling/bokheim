//! One durable configuration owns account access and cloud book storage.

use super::{LocalRegistry, RegistryResult};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use sync_common::{LibraryId, storage::LibraryStorage};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum LibraryConfiguration {
    DeviceOnly,
    Unbound {
        storage: LibraryStorage,
    },
    Account {
        owner: String,
        storage: LibraryStorage,
    },
    /// Existing caches must discover their owner before accessing an account.
    Undiscovered {
        storage: LibraryStorage,
    },
}

impl LibraryConfiguration {
    pub fn storage(&self) -> LibraryStorage {
        match self {
            Self::DeviceOnly => LibraryStorage::Local,
            Self::Unbound { storage } | Self::Account { storage, .. } | Self::Undiscovered { storage } => storage.clone(),
        }
    }

    pub fn device_only(&self) -> bool {
        matches!(self, Self::DeviceOnly)
    }

    pub fn has_account(&self) -> bool {
        matches!(self, Self::Account { .. })
    }

    pub fn accessible_to_account(&self, account: Option<&str>) -> bool {
        match self {
            Self::DeviceOnly | Self::Unbound { .. } => true,
            Self::Account { owner, .. } => account.is_none_or(|account| account == owner),
            Self::Undiscovered { .. } => account.is_none(),
        }
    }

    fn bind(self, owner: &str) -> RegistryResult<Self> {
        if owner.is_empty() {
            return Err("library account is empty".into());
        }
        match self {
            Self::DeviceOnly => Err("this library is kept only on this device".into()),
            Self::Account { owner: existing, .. } if existing != owner => Err("library belongs to another account".into()),
            configuration => Ok(Self::Account { owner: owner.to_owned(), storage: configuration.storage() }),
        }
    }

    fn with_storage(self, storage: LibraryStorage) -> RegistryResult<Self> {
        Ok(match self {
            Self::DeviceOnly if storage == LibraryStorage::Local => Self::DeviceOnly,
            Self::DeviceOnly => return Err("choose an account before enabling cloud storage".into()),
            Self::Unbound { .. } => Self::Unbound { storage },
            Self::Account { owner, .. } => Self::Account { owner, storage },
            Self::Undiscovered { .. } => Self::Undiscovered { storage },
        })
    }
}

fn read(connection: &Connection, id: &LibraryId) -> RegistryResult<LibraryConfiguration> {
    let bytes: Option<Vec<u8>> = connection.query_row("SELECT configuration FROM library_configuration WHERE library_id=?1", [id.to_string()], |row| row.get(0)).optional()?;
    match bytes {
        Some(bytes) => Ok(serde_json::from_slice(&bytes)?),
        // Remote account snapshots can reference IDs before local attachment.
        None if !connection.query_row("SELECT EXISTS(SELECT 1 FROM library_registry WHERE library_id=?1)", [id.to_string()], |row| row.get::<_, bool>(0))? => Ok(LibraryConfiguration::Unbound { storage: LibraryStorage::Bokheim }),
        None => Err("library configuration is missing".into()),
    }
}

pub(super) fn write(connection: &Connection, id: &LibraryId, configuration: &LibraryConfiguration) -> RegistryResult<()> {
    connection.execute("INSERT INTO library_configuration(library_id,configuration) VALUES(?1,?2) ON CONFLICT(library_id) DO UPDATE SET configuration=excluded.configuration", params![id.to_string(), serde_json::to_vec(configuration)?])?;
    Ok(())
}

pub(super) fn create(connection: &Connection, id: &LibraryId, configuration: &LibraryConfiguration) -> RegistryResult<bool> {
    Ok(connection.execute("INSERT INTO library_configuration(library_id,configuration) VALUES(?1,?2) ON CONFLICT(library_id) DO NOTHING", params![id.to_string(), serde_json::to_vec(configuration)?])? != 0)
}

fn pending_change(tx: &Transaction<'_>, id: &LibraryId, enabled: bool) -> RegistryResult<()> {
    if read(tx, id)?.device_only() {
        tx.execute("DELETE FROM app_value WHERE key=?1", [format!("pending-cloud-storage:{id}")])?;
    } else {
        let pending = format!("{}:{}", uuid::Uuid::new_v4(), u8::from(enabled));
        tx.execute(include_str!("sql/set_sync_disabled.sql"), params![format!("pending-cloud-storage:{id}"), pending.as_bytes()])?;
    }
    Ok(())
}

pub(super) fn cache_cloud_storage(tx: &Transaction<'_>, id: &LibraryId, enabled: bool) -> RegistryResult<()> {
    let configuration = read(tx, id)?;
    if !configuration.device_only() {
        let storage = if enabled { LibraryStorage::Bokheim } else { LibraryStorage::Local };
        write(tx, id, &configuration.with_storage(storage)?)?;
        tx.execute(include_str!("sql/set_sync_disabled.sql"), params![format!("cloud-storage-reset:{id}"), uuid::Uuid::new_v4().as_bytes().as_slice()])?;
    }
    Ok(())
}

impl LocalRegistry {
    pub fn configuration(&self, id: &LibraryId) -> RegistryResult<LibraryConfiguration> {
        read(&self.connection, id)
    }

    pub(super) fn accessible_to_account(&self, id: &LibraryId, account: Option<&str>) -> RegistryResult<bool> {
        Ok(self.configuration(id)?.accessible_to_account(account))
    }

    pub(super) fn device_only(&self, id: &LibraryId) -> RegistryResult<bool> {
        Ok(self.configuration(id)?.device_only())
    }

    pub(super) fn has_account(&self, id: &LibraryId) -> RegistryResult<bool> {
        Ok(self.configuration(id)?.has_account())
    }

    pub(super) fn make_device_only(&mut self, id: &LibraryId) -> RegistryResult<()> {
        self.entry(id)?.ok_or("library not found")?;
        let tx = self.connection.transaction()?;
        if read(&tx, id)?.has_account() {
            return Err("account libraries must retain their owner; change cloud book storage instead".into());
        }
        write(&tx, id, &LibraryConfiguration::DeviceOnly)?;
        tx.execute("DELETE FROM app_value WHERE key=?1 OR key=?2", params![format!("pending-cloud-storage:{id}"), format!("pending-library-rename/{id}")])?;
        tx.commit()?;
        Ok(())
    }

    pub(super) fn bind_library_account(&mut self, id: &LibraryId, account: &str) -> RegistryResult<()> {
        let tx = self.connection.transaction()?;
        create(&tx, id, &LibraryConfiguration::Unbound { storage: LibraryStorage::Bokheim })?;
        let configuration = read(&tx, id)?.bind(account)?;
        write(&tx, id, &configuration)?;
        tx.commit()?;
        Ok(())
    }

    pub(super) fn bind_unowned_libraries(&mut self, account: &str) -> RegistryResult<()> {
        let tx = self.connection.transaction()?;
        let entries = tx.prepare("SELECT library_id,configuration FROM library_configuration")?.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)))?.collect::<Result<Vec<_>, _>>()?;
        for (id, bytes) in entries {
            let configuration: LibraryConfiguration = serde_json::from_slice(&bytes)?;
            if matches!(configuration, LibraryConfiguration::Unbound { .. }) {
                write(&tx, &LibraryId::parse_str(&id)?, &configuration.bind(account)?)?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn storage(&self, id: &LibraryId) -> RegistryResult<LibraryStorage> {
        Ok(self.configuration(id)?.storage())
    }

    pub fn asset_storage_enabled(&self, id: &LibraryId) -> RegistryResult<bool> {
        Ok(self.storage(id)?.uses_bokheim_assets())
    }

    pub fn cache_remote_storage(&mut self, id: &LibraryId, storage: LibraryStorage) -> RegistryResult<()> {
        self.entry(id)?.ok_or("library not found")?;
        let tx = self.connection.transaction()?;
        let configuration = read(&tx, id)?;
        let pending: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM app_value WHERE key=?1)", [format!("pending-cloud-storage:{id}")], |row| row.get(0))?;
        if !configuration.device_only() && !pending {
            write(&tx, id, &configuration.with_storage(storage)?)?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn set_asset_storage_enabled(&mut self, id: &LibraryId, enabled: bool) -> RegistryResult<()> {
        self.entry(id)?.ok_or("library not found")?;
        let tx = self.connection.transaction()?;
        let storage = if enabled { LibraryStorage::Bokheim } else { LibraryStorage::Local };
        write(&tx, id, &read(&tx, id)?.with_storage(storage)?)?;
        tx.commit()?;
        Ok(())
    }

    pub fn cache_cloud_storage_with_reset(&mut self, id: &LibraryId, enabled: bool) -> RegistryResult<()> {
        self.entry(id)?.ok_or("library not found")?;
        let tx = self.connection.transaction()?;
        cache_cloud_storage(&tx, id, enabled)?;
        tx.commit()?;
        Ok(())
    }

    /// Account adoption, policy and the remote intent commit as one transition.
    pub fn configure_cloud_storage(&mut self, id: &LibraryId, enabled: bool, account: Option<&str>) -> RegistryResult<()> {
        self.entry(id)?.ok_or("library not found")?;
        let tx = self.connection.transaction()?;
        let current = read(&tx, id)?;
        let configuration = if current.device_only() {
            if enabled {
                LibraryConfiguration::Account { owner: account.filter(|account| !account.is_empty()).ok_or("sign in before enabling cloud storage")?.to_owned(), storage: LibraryStorage::Bokheim }
            } else {
                LibraryConfiguration::DeviceOnly
            }
        } else if account.is_none() && !current.has_account() && !enabled {
            LibraryConfiguration::DeviceOnly
        } else {
            let bound = match account {
                Some(account) => current.bind(account)?,
                None => current,
            };
            bound.with_storage(if enabled { LibraryStorage::Bokheim } else { LibraryStorage::Local })?
        };
        write(&tx, id, &configuration)?;
        pending_change(&tx, id, enabled)?;
        tx.execute(include_str!("sql/set_sync_disabled.sql"), params![format!("cloud-storage-reset:{id}"), uuid::Uuid::new_v4().as_bytes().as_slice()])?;
        if configuration.device_only() {
            tx.execute("DELETE FROM app_value WHERE key=?1", [format!("pending-library-rename/{id}")])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn queue_cloud_storage(&mut self, id: &LibraryId, enabled: bool) -> RegistryResult<()> {
        self.entry(id)?.ok_or("library not found")?;
        let tx = self.connection.transaction()?;
        let storage = if enabled { LibraryStorage::Bokheim } else { LibraryStorage::Local };
        write(&tx, id, &read(&tx, id)?.with_storage(storage)?)?;
        pending_change(&tx, id, enabled)?;
        tx.commit()?;
        Ok(())
    }
}

/// Import old flags once, retaining ownership and outstanding deletion intents.
/// An explicit device-only choice wins over a conflicting cached server policy.
pub(super) fn migrate(connection: &mut Connection) -> RegistryResult<()> {
    let tx = connection.transaction()?;
    let old_accounts: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='library_account')", [], |row| row.get(0))?;
    let keys =
        tx.prepare("SELECT key FROM app_value WHERE key GLOB 'library-storage:*' OR key GLOB 'library_sync_disabled:*' OR key GLOB 'device-only-library:*'")?.query_map([], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
    let mut ids = tx
        .prepare("SELECT library_id FROM library_registry UNION SELECT library_id FROM detached_library UNION SELECT library_id FROM pending_local_purge")?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<BTreeSet<_>, _>>()?;
    if old_accounts {
        ids.extend(tx.prepare("SELECT library_id FROM library_account")?.query_map([], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?);
    }
    ids.extend(keys.iter().filter_map(|key| key.split_once(':').map(|(_, id)| id.to_owned())));
    for id in ids {
        let parsed = LibraryId::parse_str(&id)?;
        let exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM library_configuration WHERE library_id=?1)", [&id], |row| row.get(0))?;
        if exists {
            continue;
        }
        let value = |key: String| -> rusqlite::Result<Option<Vec<u8>>> { tx.query_row("SELECT value FROM app_value WHERE key=?1", [key], |row| row.get(0)).optional() };
        let configuration = if value(format!("device-only-library:{id}"))?.is_some() {
            tx.execute("DELETE FROM app_value WHERE key=?1 OR key=?2", params![format!("pending-cloud-storage:{id}"), format!("pending-library-rename/{id}")])?;
            LibraryConfiguration::DeviceOnly
        } else {
            let storage = match value(format!("library-storage:{id}"))? {
                Some(bytes) => serde_json::from_slice(&bytes)?,
                None => {
                    if value(format!("library_sync_disabled:{id}"))?.as_deref() == Some(b"1") {
                        LibraryStorage::Local
                    } else {
                        LibraryStorage::Bokheim
                    }
                }
            };
            let owner: Option<Option<String>> = if old_accounts { tx.query_row("SELECT account FROM library_account WHERE library_id=?1", [&id], |row| row.get(0)).optional()? } else { None };
            match owner {
                Some(Some(owner)) => LibraryConfiguration::Account { owner, storage },
                Some(None) => LibraryConfiguration::Unbound { storage },
                None => LibraryConfiguration::Undiscovered { storage },
            }
        };
        write(&tx, &parsed, &configuration)?;
    }
    tx.execute("DELETE FROM app_value WHERE key GLOB 'library-storage:*' OR key GLOB 'library_sync_disabled:*' OR key GLOB 'device-only-library:*'", [])?;
    if old_accounts {
        tx.execute_batch("DROP TABLE library_account;")?;
    }
    tx.commit()?;
    Ok(())
}
