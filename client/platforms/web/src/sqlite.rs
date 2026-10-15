//! Browser SQLite connection mechanisms over the installed OPFS VFS.
use rusqlite::{Connection, OpenFlags};
use std::path::Path;

/// OPFS must never wait synchronously for another same-worker borrower.
pub const SINGLE_OWNER: bool = true;

/// The CPU worker owns its own reference file handles. It never opens the
/// database worker's library/asset pool or performs asset reclamation.
pub async fn initialize_reference_storage() -> Result<(), String> {
    use sqlite_wasm_vfs::sahpool::{install, OpfsSAHPoolCfgBuilder};
    let config = OpfsSAHPoolCfgBuilder::new().vfs_name("bokheim-cpu-reference-v1").directory(".bokheim-cpu-reference-v1").initial_capacity(2).clear_on_init(false).build();
    let storage = install::<rusqlite::ffi::WasmOsCallback>(&config, true).await.map_err(|e| e.to_string())?;
    storage.reserve_minimum_capacity(storage.count().saturating_add(2)).await.map_err(|e| e.to_string())
}

pub fn open_additional(path: &Path) -> rusqlite::Result<Connection> {
    Connection::open(path)
}

/// Check the VFS namespace rather than probing the host filesystem.
pub fn open_existing(path: &Path) -> Result<Option<Connection>, String> {
    let locator = path.to_str().ok_or("browser database locator is not UTF-8")?;
    if !crate::web_storage::database_exists(locator)? {
        return Ok(None);
    }
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).map(Some).map_err(|error| error.to_string())
}
