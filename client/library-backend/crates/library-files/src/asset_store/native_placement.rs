//! Concrete native placement operations; inputs are selected by the caller.

use super::{imp, AssetStore, AssetStoreError, PublishedStagedBook, StagedBook};
use client_platform_native::filesystem::{parent_directory, sync_directory};
use content_address::ContentHash;
use library_model::RelativeBookPath;
use library_replica::portable_name_key;
use std::fs::File;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use sync_common::{BookId, DirId};

/// Verified download bytes ready for a short publication. Keep this value until
/// after releasing the library gate so abandoned audiobook trees are removed
/// on the blocking executor.
pub struct PreparedDownload {
    book_id: BookId,
    checksum: ContentHash,
    _source: Option<tempfile::TempPath>,
    requested_name: String,
    relative: RelativeBookPath,
    output: DownloadOutput,
}

enum DownloadOutput {
    Existing(imp::VerifiedPath),
    File(Option<imp::PreparedFile>),
    Audiobook { folder: tempfile::TempDir, fingerprint: String },
}

impl StagedBook {
    /// Inspect collisions, extract audiobook tracks, verify their identity and
    /// flush their bytes before acquiring the library gate.
    pub fn prepare_download(self, store: &AssetStore, relative: &RelativeBookPath, occupied: &[String], audiobook: bool, track_titles: &[(String, String)]) -> Result<PreparedDownload, AssetStoreError> {
        if !store.root_available() {
            return Err(folder_unavailable());
        }
        if self.inner.direct.is_some() {
            return Err(AssetStoreError::operation("a direct import is not a staged download"));
        }
        let hash = BookId::from(self.inner.byte_checksum);
        let requested_name = relative.file_name().to_owned();
        let names = occupied.iter().map(String::as_str).chain(std::iter::once(requested_name.as_str()));
        let mut alternatives: Box<dyn Iterator<Item = String> + '_> =
            if audiobook { Box::new(library_replica::folder_name_candidates(&requested_name, names)) } else { Box::new(library_replica::file_name_candidates(&requested_name, names)) };
        let mut relative = relative.clone();
        let existing = loop {
            let destination = imp::placement_path(&store.0, &relative);
            match std::fs::symlink_metadata(&destination) {
                Ok(metadata) => {
                    if !metadata.file_type().is_symlink() && (if audiobook { metadata.is_dir() } else { metadata.is_file() }) {
                        if let Some(verified) = imp::VerifiedPath::matching(destination, &hash)? {
                            break Some(verified);
                        }
                    }
                }
                Err(error) if error.kind() == ErrorKind::NotFound => break None,
                Err(error) => return Err(error.into()),
            }
            let chosen = alternatives.next().ok_or_else(|| AssetStoreError::operation("no collision-free file name remains"))?;
            relative = relative.with_file_name(&chosen).map_err(AssetStoreError::operation)?;
        };
        drop(alternatives);
        let mut output = if let Some(existing) = existing {
            DownloadOutput::Existing(existing)
        } else if audiobook {
            let destination = imp::placement_path(&store.0, &relative);
            let folder = tempfile::Builder::new().prefix(".bokheim-audio-").tempdir_in(parent_directory(&destination)?)?;
            audiobook_folder::extract_download(File::open(&self.inner.temporary_path)?, folder.path(), track_titles)?;
            if BookId::from(audiobook_folder::folder_hash(folder.path())?) != hash {
                return Err(AssetStoreError::operation("downloaded audiobook cannot be reconstructed with its original content identity"));
            }
            let tracks = audiobook_folder::discover(folder.path())?;
            let fingerprint = crate::filesystem::audiobook_fingerprint(folder.path(), &tracks)?.to_string();
            for entry in walkdir::WalkDir::new(folder.path()).contents_first(true) {
                let entry = entry.map_err(AssetStoreError::operation)?;
                if entry.file_type().is_file() {
                    File::options().read(true).write(true).open(entry.path())?.sync_all()?;
                } else if entry.file_type().is_dir() {
                    sync_directory(entry.path())?;
                }
            }
            DownloadOutput::Audiobook { folder, fingerprint }
        } else {
            DownloadOutput::File(None)
        };
        let imp::StagedBook { temporary_path, book_id, byte_checksum, .. } = self.inner;
        let source = if let DownloadOutput::File(file) = &mut output {
            *file = Some(imp::PreparedFile::prepare(temporary_path, imp::placement_path(&store.0, &relative), &hash)?);
            None
        } else {
            Some(temporary_path)
        };
        Ok(PreparedDownload { book_id, checksum: byte_checksum, _source: source, requested_name, relative, output })
    }
}

impl PreparedDownload {
    /// The caller rechecks the book snapshot under the gate and supplies the
    /// current occupied names. Publication performs no content hashing/copying.
    pub fn publish(&mut self, store: &AssetStore, occupied: &[String]) -> Result<PublishedStagedBook, AssetStoreError> {
        if !store.root_available() {
            return Err(folder_unavailable());
        }
        let name = self.relative.file_name().to_owned();
        let key = portable_name_key(&name);
        if name != self.requested_name && occupied.iter().any(|other| portable_name_key(other) == key) {
            return Err(AssetStoreError::operation("download destination name changed; retry"));
        }
        if let DownloadOutput::Existing(existing) = &self.output {
            existing.check_metadata()?;
        }
        if self.book_id.content_hash().is_none() {
            imp::remember_local_checksum(&store.0, &self.book_id, self.checksum)?;
        }
        let destination = imp::placement_path(&store.0, &self.relative);
        let (published, fingerprint) = match &mut self.output {
            DownloadOutput::Existing(_) => (false, None),
            DownloadOutput::File(file) => {
                file.take().expect("unpublished download").publish()?;
                (true, super::published_fingerprint(&destination)?)
            }
            DownloadOutput::Audiobook { folder, fingerprint } => {
                crate::filesystem::rename_entry(folder.path(), &destination)?;
                (true, Some(fingerprint.clone()))
            }
        };
        imp::sync_import_file(&store.0, &destination)?;
        Ok(PublishedStagedBook { name, relative_path: self.relative.to_string(), published, fingerprint })
    }
}

fn folder_unavailable() -> AssetStoreError {
    AssetStoreError::operation(client_protocol::BackendError::classified(client_protocol::Failure::LibraryFolderUnavailable, "placement folder is unavailable"))
}

pub struct PreparedPlacement(imp::PreparedPlacement);

impl PreparedPlacement {
    pub fn renames_source(&self) -> bool {
        matches!(self.0, imp::PreparedPlacement::Renamed { .. })
    }
    pub fn publish(self) -> Result<(), AssetStoreError> {
        self.0.publish()
    }

    /// Transactional publication must never fall back to a slow copy.
    pub fn publish_short(self) -> Result<(), AssetStoreError> {
        self.0.publish_short()
    }
}

pub struct PreparedDirectory(imp::projection::PreparedDirectory);

pub struct DirectoryPlacement {
    pub renamed_to: Option<String>,
    pub materialized: bool,
}

impl PreparedDirectory {
    /// Interactive commands have already chosen a database name. Never create
    /// a differently named folder and then reject its logical commit.
    pub fn publish_exact(self) -> Result<Option<DirectoryPlacement>, AssetStoreError> {
        if self.0.chosen.is_some() {
            return Err(AssetStoreError::operation("destination folder is occupied; please try again"));
        }
        self.publish()
    }

    /// None means an inspected filesystem entry changed; inspect again.
    pub fn publish(self) -> Result<Option<DirectoryPlacement>, AssetStoreError> {
        if !self.0.publish()? {
            return Ok(None);
        }
        Ok(Some(DirectoryPlacement { renamed_to: self.0.chosen, materialized: !self.0.missing_external }))
    }
}

impl AssetStore {
    /// Prepare a short, same-directory source rename. The owner records replay
    /// intent before publishing it, so an interrupted long move is recoverable.
    pub fn prepare_move_source_staging(&self, hash: &BookId, source: &RelativeBookPath) -> Result<Option<(PreparedPlacement, RelativeBookPath)>, AssetStoreError> {
        let path = imp::placement_path(&self.0, source);
        match std::fs::symlink_metadata(&path) {
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
            Ok(_) => {}
        }
        let source_file = imp::VerifiedPath::new(path, hash)?;
        let staged = source.with_file_name(&format!(".bokheim-move-{}", uuid::Uuid::new_v4())).map_err(AssetStoreError::operation)?;
        let destination = imp::placement_path(&self.0, &staged);
        Ok(Some((PreparedPlacement(imp::PreparedPlacement::Renamed { source: source_file, destination }), staged)))
    }

    pub fn create_directory(&self, relative: &str, id: DirId) -> Result<String, AssetStoreError> {
        imp::create_directory(&self.0, relative, id)
    }

    /// A valid marker identifies a managed folder left by an interrupted import.
    /// Symlinks and unmarked foreign folders are never recovery candidates.
    pub fn import_directory_id(&self, relative: &str) -> Result<Option<DirId>, AssetStoreError> {
        let directory = imp::library_path(&self.0, relative);
        for path in [&directory, &directory.join(".biblos_uuid")] {
            match std::fs::symlink_metadata(path) {
                Ok(metadata) if metadata.file_type().is_symlink() => return Ok(None),
                Ok(metadata) if path == &directory && !metadata.is_dir() => return Ok(None),
                Ok(metadata) if path != &directory && (!metadata.is_file() || metadata.len() > 80) => return Ok(None),
                Ok(_) => {}
                Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(error.into()),
            }
        }
        match std::fs::read_to_string(directory.join(".biblos_uuid")) {
            Ok(marker) => Ok(DirId::parse_str(marker.trim()).ok()),
            Err(error) if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::InvalidData) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub fn retain_import_directory(&self, relative: &str, id: DirId) -> Result<(), AssetStoreError> {
        if self.import_directory_id(relative)? != Some(id) {
            return Err(AssetStoreError::operation("import folder identity changed during recovery"));
        }
        imp::sync_import_file(&self.0, &imp::library_path(&self.0, relative).join(".biblos_uuid"))
    }

    /// Include numbered destinations so retry also recovers a batch created
    /// beside a preexisting foreign folder with the requested name.
    pub fn import_directory_candidates(&self, relative: &str) -> Result<Vec<(String, DirId)>, AssetStoreError> {
        let path = imp::library_path(&self.0, relative);
        let parent = path.parent().ok_or_else(|| AssetStoreError::operation("folder has no parent"))?;
        let name = path.file_name().and_then(|name| name.to_str()).ok_or_else(|| AssetStoreError::operation("invalid folder name"))?;
        let occupied = std::fs::read_dir(parent)?.map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned())).collect::<Result<std::collections::HashSet<_>, _>>()?;
        let prefix = relative.rsplit_once('/').map(|(parent, _)| parent);
        let mut candidates = Vec::new();
        for name in library_replica::folder_name_candidates(name, std::iter::empty()).take(occupied.len() + 1).filter(|name| occupied.contains(name)) {
            let path = prefix.map_or_else(|| name.clone(), |parent| format!("{parent}/{name}"));
            if let Some(id) = self.import_directory_id(&path)? {
                candidates.push((name, id));
            }
        }
        Ok(candidates)
    }

    pub fn root_available(&self) -> bool {
        imp::root_available(&self.0)
    }

    /// Notify once when access detects a missing or replaced storage root.
    /// A recovered store installs a new callback for its own storage identity.
    pub fn on_root_unavailable(&self, callback: impl FnOnce() + Send + 'static) {
        *self.0.root_failure.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Box::new(callback));
    }

    pub fn placement_conflicts(&self, hash: &BookId, relative: &RelativeBookPath) -> Result<bool, AssetStoreError> {
        imp::placement_conflicts(&self.0, hash, relative)
    }

    /// Verify a retained copy now and keep its revision for short validation
    /// immediately before another placement is removed.
    pub fn prepare_retained_placement(&self, hash: &BookId, paths: &[RelativeBookPath]) -> Result<Option<PreparedPlacement>, AssetStoreError> {
        for relative in paths {
            let path = imp::placement_path(&self.0, relative);
            match std::fs::symlink_metadata(&path) {
                Ok(metadata) if !metadata.file_type().is_symlink() && (metadata.is_file() || metadata.is_dir()) => {
                    if let Some(verified) = imp::VerifiedPath::matching(path, hash)? {
                        return Ok(Some(PreparedPlacement(imp::PreparedPlacement::Existing(verified))));
                    }
                }
                Ok(_) => {}
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(None)
    }

    pub fn prepare_removal(&self, hash: &BookId, relative: &RelativeBookPath) -> Result<PreparedPlacement, AssetStoreError> {
        imp::prepare_removal(&self.0, hash, relative).map(PreparedPlacement)
    }

    pub fn prepare_placement<'a>(&self, hash: &BookId, relative: &RelativeBookPath, source_paths: &[RelativeBookPath], movable_sources: impl IntoIterator<Item = &'a RelativeBookPath>) -> Result<Option<PreparedPlacement>, AssetStoreError> {
        let destination = imp::placement_path(&self.0, relative);
        self.prepare_placement_at(hash, destination, source_paths, movable_sources)
    }

    fn prepare_placement_at<'a>(&self, hash: &BookId, destination: PathBuf, source_paths: &[RelativeBookPath], movable_sources: impl IntoIterator<Item = &'a RelativeBookPath>) -> Result<Option<PreparedPlacement>, AssetStoreError> {
        use super::native::{content_matches, placement_path, VerifiedPath};
        let handle = &self.0;
        use std::io::Write as _;
        if destination.exists() {
            return Ok(Some(PreparedPlacement(imp::PreparedPlacement::Existing(VerifiedPath::new(destination, hash)?))));
        }
        let mut found = None;
        for relative in source_paths {
            let path = placement_path(handle, relative);
            match std::fs::symlink_metadata(&path) {
                Ok(metadata) if !metadata.file_type().is_symlink() && (metadata.is_file() || metadata.is_dir()) => {
                    if content_matches(&path, hash)? {
                        found = Some((relative, path));
                        break;
                    }
                }
                Ok(_) => {}
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        // Only a source with a queued removal job for this path is safe to move.
        if let Some((source_relative, source)) = &found {
            if movable_sources.into_iter().any(|movable| movable == *source_relative) {
                if !parent_directory(&destination)?.is_dir() {
                    return Err(folder_unavailable());
                }
                return Ok(Some(PreparedPlacement(imp::PreparedPlacement::Renamed { source: VerifiedPath::new(source.clone(), hash)?, destination })));
            }
        }
        let Some((_, source)) = found else { return Ok(None) };
        if !parent_directory(&destination)?.is_dir() {
            return Err(folder_unavailable());
        }
        if source.is_dir() {
            let output = tempfile::Builder::new().prefix(".bokheim-copy-").tempdir_in(parent_directory(&destination)?)?;
            let copy_member = |path: &Path| -> Result<(), AssetStoreError> {
                let local = path.strip_prefix(&source).map_err(AssetStoreError::operation)?;
                let target = output.path().join(local);
                std::fs::create_dir_all(parent_directory(&target)?)?;
                std::fs::copy(path, target)?;
                Ok(())
            };
            for track in audiobook_folder::discover(&source)? {
                copy_member(&track.path)?;
            }
            for sidecar in [audiobook_folder::discover_nfo(&source)?, audiobook_folder::discover_cue(&source)?, audiobook_folder::discover_cover(&source)?].into_iter().flatten() {
                copy_member(&sidecar.path)?;
            }
            audiobook_folder::copy_path_mapping(&source, output.path())?;
            if !content_matches(output.path(), hash)? {
                return Err(AssetStoreError::operation("audiobook changed during copy"));
            }
            return Ok(Some(PreparedPlacement(imp::PreparedPlacement::CopiedFolder { output, destination })));
        }
        let mut output = tempfile::Builder::new().prefix(".bokheim-copy-").tempfile_in(parent_directory(&destination)?)?;
        let mut input = File::open(source)?;
        std::io::copy(&mut input, &mut output)?;
        output.flush()?;
        output.as_file().sync_all()?;
        // Verify the staged bytes too: an external editor can change the source
        // between source discovery and copying it.
        if !content_matches(output.path(), hash)? {
            return Err(AssetStoreError::operation("book changed during copy"));
        }
        let file = imp::PreparedFile::prepare(output.into_temp_path(), destination, hash)?;
        Ok(Some(PreparedPlacement(imp::PreparedPlacement::Copied(file))))
    }

    /// Build a complete copied tree under one hidden root. Publication then
    /// needs only one rename, regardless of the number or size of its books.
    pub fn prepare_directory_copy<'a>(
        &self, root: &str, directories: impl DoubleEndedIterator<Item = (DirId, &'a str)> + Clone, books: impl IntoIterator<Item = (BookId, &'a RelativeBookPath, &'a [RelativeBookPath])>,
    ) -> Result<(PreparedPlacement, Vec<String>), AssetStoreError> {
        use std::io::Write as _;
        let destination = imp::library_path(&self.0, root);
        let output = tempfile::Builder::new().prefix(".bokheim-copy-").tempdir_in(parent_directory(&destination)?)?;
        for (id, relative) in directories.clone() {
            let suffix = Path::new(relative.trim_start_matches('/')).strip_prefix(root.trim_start_matches('/')).map_err(AssetStoreError::operation)?;
            let directory = output.path().join(suffix);
            std::fs::create_dir_all(&directory)?;
            let mut marker = File::create(directory.join(".biblos_uuid"))?;
            marker.write_all(id.to_string().as_bytes())?;
            marker.sync_all()?;
        }
        let mut materialized = Vec::new();
        for (hash, relative, sources) in books {
            let suffix = Path::new(relative.as_str().trim_start_matches('/')).strip_prefix(root.trim_start_matches('/')).map_err(AssetStoreError::operation)?;
            if let Some(prepared) = self.prepare_placement_at(&hash, output.path().join(suffix), sources, &[])? {
                prepared.publish_short()?;
                materialized.push(relative.to_string());
            }
        }
        for (_, relative) in directories.rev() {
            let suffix = Path::new(relative.trim_start_matches('/')).strip_prefix(root.trim_start_matches('/')).map_err(AssetStoreError::operation)?;
            client_platform_native::filesystem::sync_directory(&output.path().join(suffix))?;
        }
        Ok((PreparedPlacement(imp::PreparedPlacement::CopiedFolder { output, destination }), materialized))
    }

    pub fn inspect_directory(&self, id: DirId, relative: &str, projected: Option<&str>, name: &str, siblings: &[String]) -> Result<PreparedDirectory, AssetStoreError> {
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
