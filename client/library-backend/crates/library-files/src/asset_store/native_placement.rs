//! Concrete native placement operations; inputs are selected by the caller.

use std::fs::File;
use super::{imp, AssetStore, AssetStoreError};
use library_model::RelativeBookPath;
use std::path::Path;
use sync_common::{ContentHash, DirId};

pub struct PreparedPlacement(imp::PreparedPlacement);

impl PreparedPlacement {
    pub fn publish(self) -> Result<(), AssetStoreError> {
        self.0.publish()
    }
}

pub struct PreparedDirectory(imp::projection::PreparedDirectory);

pub struct DirectoryPlacement {
    pub renamed_to: Option<String>,
    pub materialized: bool,
}

impl PreparedDirectory {
    /// None means an inspected filesystem entry changed; inspect again.
    pub fn publish(self) -> Result<Option<DirectoryPlacement>, AssetStoreError> {
        if !self.0.publish()? {
            return Ok(None);
        }
        Ok(Some(DirectoryPlacement { renamed_to: self.0.chosen, materialized: !self.0.missing_external }))
    }
}

impl AssetStore {
    pub fn create_directory(&self, relative: &str, id: DirId) -> Result<String, AssetStoreError> {
        imp::create_directory(&self.0, relative, id)
    }

    pub fn root_available(&self) -> bool {
        imp::root_available(&self.0)
    }

    pub fn placement_conflicts(&self, hash: &ContentHash, relative: &RelativeBookPath) -> Result<bool, AssetStoreError> {
        imp::placement_conflicts(&self.0, hash, relative)
    }

    pub fn prepare_removal(&self, hash: &ContentHash, relative: &RelativeBookPath) -> Result<PreparedPlacement, AssetStoreError> {
        imp::prepare_removal(&self.0, hash, relative).map(PreparedPlacement)
    }

    pub fn prepare_placement(
        &self, hash: &ContentHash, relative: &RelativeBookPath, expected: Option<&str>, source_paths: &[RelativeBookPath], movable_sources: &[RelativeBookPath],
    ) -> Result<Option<PreparedPlacement>, AssetStoreError> {
        use super::native::{content_matches, placement_path, VerifiedPath};
        let handle = &self.0;
        use std::io::Write as _;
        let destination = placement_path(handle, relative);
        if destination.exists() {
            return Ok(Some(PreparedPlacement(imp::PreparedPlacement::Existing(VerifiedPath::new(destination, hash)?))));
        }
        let matches_version = |path: &Path| -> Result<bool, AssetStoreError> {
            if !content_matches(path, hash)? {
                return Ok(false);
            }
            // The ZIP is a transport representation. The ordered source bytes
            // already established the folder's identity above.
            if path.is_dir() {
                return Ok(true);
            }
            let Some(expected) = expected else { return Ok(true) };
            let mut hasher = blake3::Hasher::new();
            hasher.update_reader(File::open(path)?)?;
            Ok(hasher.finalize().to_hex().as_str() == expected)
        };
        let found = source_paths
            .iter()
            .map(|relative| (relative, placement_path(handle, relative)))
            .find(|(_, path)| (path.is_file() || path.is_dir()) && matches_version(path).unwrap_or(false));
        // Only a source with a queued removal job for this path is safe to move.
        if let Some((source_relative, source)) = &found {
            if movable_sources.iter().any(|movable| movable == *source_relative) {
                if !destination.parent().unwrap().is_dir() {
                    return Err(AssetStoreError::operation("placement folder is unavailable"));
                }
                return Ok(Some(PreparedPlacement(imp::PreparedPlacement::Renamed { source: source.clone(), destination })));
            }
        }
        let Some((_, source)) = found else { return Ok(None) };
        if !destination.parent().unwrap().is_dir() {
            return Err(AssetStoreError::operation("placement folder is unavailable"));
        }
        if source.is_dir() {
            let output = tempfile::Builder::new().prefix(".bokheim-copy-").tempdir_in(destination.parent().unwrap())?;
            for track in audiobook_folder::discover(&source)? {
                let target = output.path().join(&track.name);
                std::fs::create_dir_all(target.parent().unwrap())?;
                std::fs::copy(&track.path, target)?;
            }
            if let Some(nfo) = audiobook_folder::discover_nfo(&source)? {
                std::fs::copy(nfo.path, output.path().join(nfo.name))?;
            }
            if let Some(cue) = audiobook_folder::discover_cue(&source)? {
                std::fs::copy(cue.path, output.path().join(cue.name))?;
            }
            if let Some(cover) = audiobook_folder::discover_cover(&source)? {
                std::fs::copy(cover.path, output.path().join(cover.name))?;
            }
            if !matches_version(output.path())? {
                return Err(AssetStoreError::operation("audiobook changed during copy"));
            }
            return Ok(Some(PreparedPlacement(imp::PreparedPlacement::CopiedFolder { output, destination })));
        }
        let mut output = tempfile::Builder::new().prefix(".bokheim-copy-").tempfile_in(destination.parent().unwrap())?;
        let mut input = File::open(source)?;
        std::io::copy(&mut input, &mut output)?;
        output.flush()?;
        output.as_file().sync_all()?;
        // Verify the staged bytes too: an external editor can change the source
        // between source discovery and copying it.
        if !matches_version(output.path())? {
            return Err(AssetStoreError::operation("book changed during copy"));
        }
        Ok(Some(PreparedPlacement(imp::PreparedPlacement::Copied { output, destination })))
    }

    pub fn inspect_directory(
        &self, id: DirId, relative: &str, projected: Option<&str>, name: &str, siblings: &[String],
    ) -> Result<PreparedDirectory, AssetStoreError> {
        imp::projection::PreparedDirectory::inspect(&self.0, id, relative, projected, name, siblings).map(PreparedDirectory)
    }

    /// Move a managed directory aside, or recover an interrupted staging move.
    pub fn stage_directory(&self, id: DirId, old: &str, desired: Option<&str>) -> Result<Option<String>, AssetStoreError> {
        use super::native::projection::directory_entry_stamp;
        use super::native::{library_path, sync_parent};
        let handle = &self.0;
        let staged = format!("/.biblos-move-{id}");
        if old == staged {
            return Ok(None);
        }
        let source = library_path(handle, old);
        let destination = library_path(handle, &staged);
        let source_exists = directory_entry_stamp(&source)?.is_some();
        let destination_exists = directory_entry_stamp(&destination)?.is_some();
        let owned = |path: &Path| -> Result<bool, AssetStoreError> {
            let metadata = std::fs::symlink_metadata(path)?;
            Ok(metadata.is_dir() && !metadata.file_type().is_symlink() && std::fs::read_to_string(path.join(".biblos_uuid"))?.trim() == id.to_string())
        };
        if destination_exists {
            if source_exists || !owned(&destination)? {
                return Err(AssetStoreError::operation("directory staging destination is occupied"));
            }
        } else if desired.is_none_or(|desired| old == desired) {
            return Ok(None);
        } else if source_exists {
            if !owned(&source)? {
                return Err(AssetStoreError::operation("folder identity changed before staging"));
            }
            std::fs::rename(&source, &destination)?;
            sync_parent(&source)?;
            sync_parent(&destination)?;
        } else {
            return Ok(None);
        }
        Ok(Some(staged))
    }
}
