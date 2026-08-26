//! Device-local persistence for server-hydrated taxonomy slices.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use subject_projection::{UnifiedCodeResolution, UnifiedConceptDefinition};
use taxonomy_contract::TaxonomyCode;

#[derive(Clone, Debug)]
pub struct TaxonomyCache {
    path: PathBuf,
    update: Arc<Mutex<()>>,
}

impl TaxonomyCache {
    pub fn open(path: PathBuf) -> Result<Self, String> {
        let cache = Self { path, update: Arc::new(Mutex::new(())) };
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
        let resolved = subject_projection::read_resolved_unified_taxonomy_codes(&self.path, &candidates)?;
        Ok(codes.into_iter().filter(|code| !subject_projection::unified_has_exact_source_code(code.system.as_str(), &code.code) && !resolved.contains(&(code.system.as_str().to_owned(), code.code.clone()))).collect())
    }

    pub fn merge_slice(&self, server_release_id: &str, concepts: &[UnifiedConceptDefinition], resolutions: &[UnifiedCodeResolution]) -> Result<String, String> {
        let _guard = self.update.lock().map_err(|_| "taxonomy update lock is poisoned".to_owned())?;
        self.ensure_file()?;
        if subject_projection::unified_taxonomy_server_release_id(&self.path)?.is_some_and(|cached| cached != server_release_id) {
            self.reset()?;
        }
        subject_projection::merge_unified_taxonomy_sqlite_with_resolutions(&self.path, server_release_id, concepts, resolutions)?;
        self.install_from_file()?;
        Ok(subject_projection::unified_taxonomy_release_id())
    }

    pub fn invalidate_if_obsolete(&self, server_release_id: &str) -> Result<bool, String> {
        let _guard = self.update.lock().map_err(|_| "taxonomy update lock is poisoned".to_owned())?;
        let Some(cached_release_id) = subject_projection::unified_taxonomy_server_release_id(&self.path)? else {
            return Ok(false);
        };
        if cached_release_id == server_release_id {
            return Ok(false);
        }
        self.reset()?;
        subject_projection::install_unified_taxonomy_overlay(Vec::new())?;
        Ok(true)
    }

    fn ensure_file(&self) -> Result<(), String> {
        if self.path.is_file() {
            return Ok(());
        }
        let parent = self.path.parent().ok_or_else(|| format!("taxonomy path has no parent: {}", self.path.display()))?;
        fs::create_dir_all(parent).map_err(|error| format!("failed to create taxonomy directory {}: {error}", parent.display()))?;
        fs::write(&self.path, subject_projection::DEFAULT_UNIFIED_TAXONOMY_SQLITE).map_err(|error| format!("failed to create editable taxonomy database {}: {error}", self.path.display()))
    }

    fn install_runtime(&self) -> Result<(), String> {
        if !self.path.exists() {
            return Ok(());
        }
        match self.install_persisted_overlay() {
            Ok(()) => Ok(()),
            Err(persisted_error) => {
                let backup_path = self.path.with_extension("sqlite3.invalid");
                fs::copy(&self.path, &backup_path).map_err(|error| format!("failed to preserve invalid taxonomy database {} as {} after {persisted_error}: {error}", self.path.display(), backup_path.display()))?;
                log::warn!("Resetting invalid taxonomy cache {} (backup: {}): {persisted_error}", self.path.display(), backup_path.display());
                self.reset()?;
                self.install_persisted_overlay().map_err(|reset_error| format!("failed to validate bundled taxonomy database {} after persisted taxonomy failed ({persisted_error}): {reset_error}", self.path.display()))
            }
        }
    }

    fn install_persisted_overlay(&self) -> Result<(), String> {
        if !subject_projection::is_unified_taxonomy_client_seed(&self.path)? {
            self.reset()?;
        } else if subject_projection::has_unified_taxonomy_overlay(&self.path)? {
            self.install_from_file()?;
        }
        Ok(())
    }

    fn reset(&self) -> Result<(), String> {
        fs::write(&self.path, subject_projection::DEFAULT_UNIFIED_TAXONOMY_SQLITE).map_err(|error| format!("failed to reset taxonomy cache {}: {error}", self.path.display()))
    }

    fn install_from_file(&self) -> Result<(), String> {
        let concepts = subject_projection::read_unified_taxonomy_sqlite(&self.path)?;
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
