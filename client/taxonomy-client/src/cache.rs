//! Device-local persistence for server-hydrated taxonomy slices.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use subject_projection::{UnifiedCodeResolution, UnifiedConceptDefinition};
use taxonomy_contract::TaxonomyCode;

#[derive(Clone, Debug)]
pub struct TaxonomyCache {
    path: PathBuf,
    connection: Arc<Mutex<rusqlite::Connection>>,
}

impl TaxonomyCache {
    pub fn open(path: PathBuf) -> Result<Self, String> {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| format!("failed to create taxonomy directory {}: {error}", parent.display()))?;
        }
        let connection = rusqlite::Connection::open(&path).map_err(|error| format!("failed to open taxonomy cache {}: {error}", path.display()))?;
        connection.busy_timeout(std::time::Duration::from_secs(5)).map_err(|error| format!("failed to configure taxonomy cache {}: {error}", path.display()))?;
        let cache = Self { path, connection: Arc::new(Mutex::new(connection)) };
        cache.install_runtime()?;
        Ok(cache)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn unresolved_codes(&self, codes: Vec<TaxonomyCode>) -> Result<Vec<TaxonomyCode>, String> {
        let codes = codes
            .into_iter()
            .filter_map(|mut code| {
                code.code = code.code.trim().to_ascii_uppercase();
                (!code.code.is_empty()).then_some(code)
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let candidates = codes.iter().map(|code| (code.system.as_str().to_owned(), code.code.clone())).collect::<Vec<_>>();
        let connection = self.connection.lock().map_err(|_| "taxonomy update lock is poisoned".to_owned())?;
        let resolved = subject_projection::read_resolved_unified_taxonomy_codes_in(&connection, &candidates, &self.path.display().to_string())?;
        Ok(codes.into_iter().filter(|code| !subject_projection::unified_has_exact_source_code(code.system.as_str(), &code.code) && !resolved.contains(&(code.system.as_str().to_owned(), code.code.clone()))).collect())
    }

    pub fn merge_slice(&self, server_release_id: &str, concepts: &[UnifiedConceptDefinition], resolutions: &[UnifiedCodeResolution]) -> Result<String, String> {
        let mut connection = self.connection.lock().map_err(|_| "taxonomy update lock is poisoned".to_owned())?;
        if subject_projection::unified_taxonomy_server_release_id_in(&connection, &self.path.display().to_string())?.is_some_and(|cached| cached != server_release_id) {
            subject_projection::reset_unified_taxonomy_connection(&mut connection)?;
        }
        subject_projection::merge_unified_taxonomy_connection(&mut connection, server_release_id, concepts, resolutions, &self.path.display().to_string())?;
        self.install_from_connection(&connection)?;
        Ok(subject_projection::unified_taxonomy_release_id())
    }

    pub fn invalidate_if_obsolete(&self, server_release_id: &str) -> Result<bool, String> {
        let mut connection = self.connection.lock().map_err(|_| "taxonomy update lock is poisoned".to_owned())?;
        let Some(cached_release_id) = subject_projection::unified_taxonomy_server_release_id_in(&connection, &self.path.display().to_string())? else {
            return Ok(false);
        };
        if cached_release_id == server_release_id {
            return Ok(false);
        }
        subject_projection::reset_unified_taxonomy_connection(&mut connection)?;
        subject_projection::install_unified_taxonomy_overlay(Vec::new())?;
        Ok(true)
    }

    fn install_runtime(&self) -> Result<(), String> {
        let mut connection = self.connection.lock().map_err(|_| "taxonomy update lock is poisoned".to_owned())?;
        let initialized = connection
            .query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='taxonomy_meta')", [], |row| row.get::<_, bool>(0))
            .map_err(|error| format!("failed to inspect taxonomy cache {}: {error}", self.path.display()))?;
        if !initialized {
            subject_projection::reset_unified_taxonomy_connection(&mut connection)?;
            return Ok(());
        }
        match self.install_persisted_overlay(&mut connection) {
            Ok(()) => Ok(()),
            Err(persisted_error) => {
                log::warn!("Resetting invalid taxonomy cache {}: {persisted_error}", self.path.display());
                subject_projection::reset_unified_taxonomy_connection(&mut connection)?;
                self.install_persisted_overlay(&mut connection).map_err(|reset_error| format!("failed to validate bundled taxonomy database {} after persisted taxonomy failed ({persisted_error}): {reset_error}", self.path.display()))
            }
        }
    }

    fn install_persisted_overlay(&self, connection: &mut rusqlite::Connection) -> Result<(), String> {
        let source = self.path.display().to_string();
        if !subject_projection::is_unified_taxonomy_client_seed_in(connection, &source)? {
            subject_projection::reset_unified_taxonomy_connection(connection)?;
        } else if subject_projection::has_unified_taxonomy_overlay_in(connection, &source)? {
            self.install_from_connection(connection)?;
        }
        Ok(())
    }

    fn install_from_connection(&self, connection: &rusqlite::Connection) -> Result<(), String> {
        let concepts = subject_projection::read_unified_taxonomy_connection(connection, &self.path.display().to_string())?;
        subject_projection::install_unified_taxonomy_concepts(concepts).map_err(|error| format!("failed to install unified taxonomy database {}: {error}", self.path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changed_server_release_restores_the_bundled_seed() {
        let directory = tempfile::tempdir().unwrap();
        let cache = TaxonomyCache::open(directory.path().join("unified.sqlite3")).unwrap();
        let resolution = UnifiedCodeResolution::new("lcc".to_owned(), "ZZZZ999".to_owned(), Vec::new());
        cache.merge_slice("release-one", &[], &[resolution]).unwrap();
        assert_eq!(subject_projection::unified_taxonomy_server_release_id(cache.path()).unwrap().as_deref(), Some("release-one"));

        assert!(cache.invalidate_if_obsolete("release-two").unwrap());
        assert_eq!(subject_projection::unified_taxonomy_server_release_id(cache.path()).unwrap(), None);
        assert!((1_000..=2_000).contains(&subject_projection::read_unified_taxonomy_sqlite(cache.path()).unwrap().len()));
        assert!(!cache.invalidate_if_obsolete("release-two").unwrap());
    }
}
