use crate::UnifiedConceptDefinition;
use rusqlite::{Connection, OpenFlags};
use std::path::Path;

#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) fn read_bundled_unified_taxonomy() -> Result<Vec<UnifiedConceptDefinition>, String> {
    read_unified_taxonomy_sqlite(&crate::bundled::path()?)
}

pub fn read_unified_taxonomy_sqlite(path: &Path) -> Result<Vec<UnifiedConceptDefinition>, String> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|error| format!("failed to open unified taxonomy database {}: {error}", path.display()))?;
    read_unified_taxonomy_connection(&connection, &path.display().to_string())
}

pub use super::taxonomy_reader::read_unified_taxonomy_connection;
