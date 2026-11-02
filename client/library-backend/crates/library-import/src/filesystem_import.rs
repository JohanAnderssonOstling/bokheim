//! Native file and folder imports from filesystem paths or picker entries.
//! Place external files on disk, then import them in place.

use std::collections::HashMap;
use std::fs;
use std::io::{self, Error};
use std::path::{Path, PathBuf};

use book_model::BookFormat;
use filesystem_scanner::discovery::{audiobook_member, is_audiobook_folder, next_book_entry, supported};
use filesystem_scanner::ingestion::{ingest_audiobook_folder, ingest_file};
use library_files::filesystem::{place_into_directory, walk};
use library_model::PublishedStagedBook;
use sync_common::DirId;

use client_platform_runtime::executor::run_blocking;
use client_runtime::BackendError;
use library_replica::filenames;

use crate::import::{validate_import_destination, ValidatedImport};
use crate::{DirectoryImport, ImportFailure, LibraryImport};

impl LibraryImport {
    pub(crate) async fn import_destination(&self, parent: DirId) -> Result<PathBuf, BackendError> {
        validate_import_destination(&self.database, &parent)?;
        if !self.assets.local_access_enabled() {
            return Err(BackendError::message("library storage is unavailable"));
        }
        library_placement::prepare_directory(&self.assets, &self.database, parent).await?;
        self.prepared_import_destination(parent)
    }

    /// Resolve a destination whose directory recovery is already complete.
    /// Keep current liveness and storage checks when a batch reuses it.
    pub(crate) fn prepared_import_destination(&self, parent: DirId) -> Result<PathBuf, BackendError> {
        validate_import_destination(&self.database, &parent)?;
        if !self.assets.local_access_enabled() {
            return Err(BackendError::message("library storage is unavailable"));
        }
        let root = PathBuf::from(&self.root).canonicalize()?;
        Ok(self.database.dir_path_components(&parent)?.into_iter().fold(root, |path, component| path.join(component.name.as_str())))
    }

    /// Reader-backed picker entries are copied into hidden staging first, then
    /// go through the same placement and in-place import as filesystem drops.
    pub async fn import_selected_directory(&self, parent: DirId, directory: DirectoryImport, create_root: bool, cancelled: &dyn Fn() -> bool) -> Result<Vec<ImportFailure>, BackendError> {
        let destination = self.import_destination(parent).await?;
        let staging = tempfile::Builder::new().prefix(".bokheim-copy-").tempdir_in(destination)?;
        let payload = if create_root { staging.path().join(filenames::validate_component(&directory.name)?) } else { staging.path().to_owned() };
        fs::create_dir_all(&payload)?;
        for path in directory.directories {
            fs::create_dir_all(selected_path(&payload, &path)?)?;
        }
        let mut failures = Vec::new();
        let mut failed_members = Vec::new();
        for file in directory.files {
            if cancelled() {
                return Err(BackendError::message("directory import cancelled"));
            }
            if !selected_folder_file_supported(&file.path) {
                continue;
            }
            let path = selected_path(&payload, &file.path)?;
            if path.try_exists()? {
                failures.push(ImportFailure::file(&file.path, "duplicate selected file path"));
                if audiobook_member(&path) {
                    failed_members.push(path);
                }
                continue;
            }
            let copied_path = path.clone();
            let result = async {
                let mut reader = file.source.open().await.map_err(BackendError::message)?;
                run_blocking(move || -> io::Result<()> {
                    fs::create_dir_all(copied_path.parent().ok_or_else(|| Error::other("selected file has no parent"))?)?;
                    let mut writer = fs::OpenOptions::new().write(true).create_new(true).open(copied_path)?;
                    io::copy(&mut reader, &mut writer)?;
                    Ok(())
                })
                .await
                .map_err(BackendError::operation)?
                .map_err(BackendError::operation)
            }
            .await;
            if let Err(error) = result {
                // A failed copy stays hidden; publish only completed entries.
                match fs::remove_file(&path) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
                failures.push(ImportFailure::file(&file.path, &error));
                if audiobook_member(&path) {
                    failed_members.push(path);
                }
            }
        }
        if cancelled() {
            return Err(BackendError::message("directory import cancelled"));
        }
        if !failed_members.is_empty() {
            let complete = run_blocking({
                let payload = payload.clone();
                move || discard_incomplete_audiobooks(&payload, &failed_members)
            })
            .await
            .map_err(BackendError::operation)??;
            if !complete {
                return Ok(failures);
            }
        }
        if cancelled() {
            return Err(BackendError::message("directory import cancelled"));
        }
        let paths = if create_root {
            vec![payload]
        } else {
            // Loose tracks and sidecars belong to their enclosing folder;
            // they are not standalone books when importing only its contents.
            fs::read_dir(&payload)?.map(|entry| entry.map(|entry| entry.path())).collect::<io::Result<Vec<_>>>()?.into_iter().filter(|path| path.is_dir() || supported(path)).collect()
        };
        // Staging is on the destination filesystem: this publishes by rename,
        // without copying again, before book inspection starts.
        failures.extend(self.place_and_import_paths(parent, paths, true, Vec::new()).await?);
        Ok(failures)
    }

    pub async fn import_paths(&self, parent: DirId, paths: Vec<PathBuf>, moving: bool) -> Result<Vec<ImportFailure>, BackendError> {
        let destination = self.import_destination(parent).await?;
        // Loose native files use the same claimed-source move as metadata
        // adapters. Folder imports continue through the tree inventory below.
        let mut remaining = Vec::new();
        let mut move_failures = Vec::new();
        for path in paths {
            let already_here = path.canonicalize().ok().and_then(|path| path.parent().map(Path::to_owned)).as_deref() == Some(destination.as_path());
            if moving && path.is_file() && supported(&path) && !already_here {
                if let Err(error) = self.move_book_from_path(parent, &path).await {
                    move_failures.push(failure(&path, &error));
                }
            } else {
                remaining.push(path);
            }
        }
        let paths = remaining;
        if paths.is_empty() {
            return Ok(move_failures);
        }
        self.place_and_import_paths(parent, paths, moving, move_failures).await
    }

    /// Publish completed picker copies before inspection so failed books remain
    /// available for retry, while external file moves keep their rollback path.
    async fn place_and_import_paths(&self, parent: DirId, paths: Vec<PathBuf>, moving: bool, prior_failures: Vec<ImportFailure>) -> Result<Vec<ImportFailure>, BackendError> {
        let destination = self.import_destination(parent).await?;
        let root = PathBuf::from(&self.root).canonicalize()?;
        let destination_relative = destination.strip_prefix(&root).map_err(BackendError::operation)?.to_owned();
        let occupied = self.database.shared_occupied_file_names(&parent)?;
        // Place the complete selection on disk before inspecting any books.
        let (placed, mut failures) = run_blocking(move || {
            let mut placed = Vec::new();
            let mut failures = Vec::new();
            for path in paths {
                let result = if path.is_dir() || supported(&path) { place_into_directory(&path, &destination, &occupied, moving) } else { Err(io::Error::other("not a supported book or folder")) };
                match result {
                    Ok(path) => placed.push(path),
                    Err(error) => failures.push(failure(&path, &BackendError::from(error))),
                }
            }
            (placed, failures)
        })
        .await
        .map_err(BackendError::operation)?;
        failures.extend(prior_failures);
        let inventory = run_blocking({
            let root = root.clone();
            move || inventory(&root, &placed)
        })
        .await
        .map_err(BackendError::operation)??;
        failures.extend(inventory.failures.into_iter().map(|(path, message)| ImportFailure::folder(&[path.to_string_lossy().into_owned()], &message)));
        let mut parents = HashMap::from([(destination_relative, parent)]);
        // Existing folders are reused on retry; register new folders at their
        // actual paths instead of creating another physical copy of the tree.
        for relative in inventory.directories {
            let Some(directory) = relative.parent().and_then(|path| parents.get(path)).copied() else {
                failures.push(failure(&relative, "parent folder could not be imported"));
                continue;
            };
            match self.adopt_directory(directory, &root, &relative) {
                Ok(id) => {
                    parents.insert(relative, id);
                }
                Err(error) => failures.push(failure(&relative, &error)),
            }
        }
        for relative in inventory.files {
            let Some(directory) = relative.parent().and_then(|path| parents.get(path)).copied() else {
                failures.push(failure(&relative, "parent folder could not be imported"));
                continue;
            };
            if let Err(error) = self.import_file_in_place(directory, &root, &relative).await {
                failures.push(failure(&relative, &error));
            }
        }
        Ok(failures)
    }

    /// Registers a folder placed under `directory`, or reuses the one already
    /// registered there on retry, and marks it on disk with its id.
    fn adopt_directory(&self, directory: DirId, root: &Path, relative: &Path) -> Result<DirId, BackendError> {
        let name = file_name(relative)?;
        let id = match self.database.child_directory(&directory, name)? {
            Some(id) => id,
            None => {
                let (_, path) = self.database.directory_creation_path(&directory, name)?;
                if Path::new(&path) != relative {
                    return Err(BackendError::message("destination folder changed before import"));
                }
                let id = DirId::new_v4();
                self.database.register_created_directory(&id, &directory, name, &path)?;
                id
            }
        };
        fs::write(root.join(relative).join(".biblos_uuid"), id.to_string())?;
        Ok(id)
    }

    async fn import_file_in_place(&self, parent: DirId, root: &Path, relative: &Path) -> Result<(), BackendError> {
        let name = file_name(relative)?;
        let path = root.join(relative);
        let (import, ingested) = if path.is_dir() {
            validate_import_destination(&self.database, &parent)?;
            let import = ValidatedImport { parent_id: parent, file_name: filenames::validate_component(name)?, format: BookFormat::Mp3Folder };
            (import, ingest_audiobook_folder(path, name.to_owned(), &*self.cpu).await?)
        } else {
            let import = self.validate_import(&parent, name)?;
            (import, ingest_file(path, name.to_owned(), &*self.cpu).await?)
        };

        let byte_checksum = ingested.book_id.content_hash().ok_or_else(|| BackendError::message("imported bytes have no checksum"))?;
        let content = self.resolve_import_content(byte_checksum, ingested.size_bytes, ingested.source_identity)?;
        let identity = content.book_id;
        self.assets.remember_local_content(identity, byte_checksum)?;
        let _lease = self.lease_import_content(&content).await?;
        // A concurrent folder move must not register these bytes at another path.
        validate_import_destination(&self.database, &parent)?;
        let placement = self.database.native_book_path(parent, name)?.ok_or_else(|| BackendError::message("destination folder no longer exists"))?;
        if Path::new(placement.as_str().trim_start_matches('/')) != relative {
            return Err(BackendError::message("destination folder changed during import"));
        }
        let published = PublishedStagedBook { name: name.to_owned(), relative_path: placement.to_string(), published: true, fingerprint: Some(ingested.fingerprint.to_string()) };
        self.commit_import(import, content, Some(ingested.inspection), published)
    }
}

struct Inventory {
    directories: Vec<PathBuf>,
    files: Vec<PathBuf>,
    failures: Vec<(PathBuf, String)>,
}

fn inventory(root: &Path, placed: &[PathBuf]) -> Result<Inventory, BackendError> {
    let mut directories = Vec::new();
    let mut files = Vec::new();
    let mut failures = Vec::new();
    for path in placed {
        let relative = path.strip_prefix(root).map_err(BackendError::operation)?.to_owned();
        if path.is_file() {
            files.push(relative);
            continue;
        }
        match is_audiobook_folder(path) {
            Ok(true) => {
                files.push(relative);
                continue;
            }
            Ok(false) => directories.push(relative),
            Err(error) => {
                failures.push((relative, error.to_string()));
                continue;
            }
        }
        let mut walk = walk(path);
        while let Some(entry) = next_book_entry(&mut walk) {
            let entry = entry?;
            let relative = entry.path().strip_prefix(root).map_err(BackendError::operation)?.to_owned();
            if entry.is_dir() {
                match is_audiobook_folder(entry.path()) {
                    Ok(true) => {
                        files.push(relative);
                        walk.skip_current_dir();
                    }
                    Ok(false) => directories.push(relative),
                    Err(error) => {
                        failures.push((relative, error.to_string()));
                        walk.skip_current_dir();
                    }
                }
            } else {
                files.push(relative);
            }
        }
    }
    directories.sort();
    directories.dedup();
    files.sort();
    files.dedup();
    Ok(Inventory { directories, files, failures })
}

/// Drop only the affected audiobooks while their copies are still hidden.
/// Checking the grouped inventory also catches failures in numbered discs.
/// A false result leaves the entire selected audiobook for staging cleanup.
fn discard_incomplete_audiobooks(payload: &Path, failed_members: &[PathBuf]) -> Result<bool, BackendError> {
    loop {
        let inventory = inventory(payload, &[payload.to_owned()])?;
        let candidates = inventory.files.into_iter().chain(inventory.failures.into_iter().map(|(path, _)| path));
        let mut discarded = false;
        for relative in candidates {
            let path = payload.join(relative);
            if !path.is_dir() || !failed_members.iter().any(|member| member.starts_with(&path)) {
                continue;
            }
            if path == payload {
                return Ok(false);
            }
            fs::remove_dir_all(path)?;
            discarded = true;
        }
        if !discarded {
            return Ok(true);
        }
        // Removing a child may make its parent look like one audiobook.
        // Check the final tree before allowing it out of staging as well.
    }
}

/// Keep the tracks and sidecars until the staged tree can be classified as
/// an audiobook or a regular folder containing individual books.
fn selected_folder_file_supported(path: &[String]) -> bool {
    let Some(name) = path.last() else { return false };
    let path = Path::new(name);
    supported(path) || audiobook_member(path)
}

fn file_name(path: &Path) -> Result<&str, BackendError> {
    path.file_name().and_then(|name| name.to_str()).ok_or_else(|| BackendError::message("invalid file name"))
}

fn failure(path: &Path, message: &(impl client_runtime::Classified + ?Sized)) -> ImportFailure {
    ImportFailure::file(&[path.to_string_lossy().into_owned()], message)
}

fn selected_path(root: &Path, components: &[String]) -> Result<PathBuf, BackendError> {
    if components.is_empty() {
        return Err(BackendError::message("selected path is empty"));
    }
    let mut path = root.to_owned();
    for component in components {
        path.push(filenames::validate_component(component)?);
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use library_database::Database;
    use library_files::AssetStore;
    use library_placement::LibraryPlacement;
    use library_runtime::{events::LibraryEventRouter, scheduler::SyncScheduler};
    use sync_common::ROOT_DIR_ID;

    fn importer(root: &Path) -> LibraryImport {
        let database = Arc::new(Database::open(":memory:").unwrap());
        database.initialize_library().unwrap();
        let assets = AssetStore::open(root.to_str().unwrap()).unwrap();
        let (sync, _) = SyncScheduler::new();
        let (asset_work, _) = SyncScheduler::new();
        let (files, _) = SyncScheduler::new();
        let (enrichment, _) = SyncScheduler::new();
        let (events, _) = LibraryEventRouter::new(sync, asset_work, files, enrichment);
        let placement = Arc::new(LibraryPlacement::new(database.clone(), assets.clone(), events.clone()));
        LibraryImport::new(placement, Arc::new(client_platform_native::cpu_host::NativeHost))
    }

    #[tokio::test]
    async fn moved_loose_book_is_registered_without_copying() {
        let root = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        let importer = importer(root.path());
        let source = external.path().join("book.epub");
        fs::write(&source, include_bytes!("../../book-inspection/tests/fixtures/back-matter-evidence.epub")).unwrap();
        #[cfg(unix)]
        let inode = {
            use std::os::unix::fs::MetadataExt;
            fs::metadata(&source).unwrap().ino()
        };
        let failures = importer.import_paths(ROOT_DIR_ID, vec![source.clone()], true).await.unwrap();
        assert!(failures.is_empty(), "{failures:?}");
        assert!(!source.exists());
        assert!(importer.database.has_book_named(&ROOT_DIR_ID, "book.epub").unwrap());
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(fs::metadata(root.path().join("book.epub")).unwrap().ino(), inode);
        }
    }

    #[tokio::test]
    async fn invalid_loose_book_move_restores_its_source() {
        let root = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        let importer = importer(root.path());
        let source = external.path().join("invalid.epub");
        fs::write(&source, b"invalid EPUB").unwrap();
        let failures = importer.import_paths(ROOT_DIR_ID, vec![source.clone()], true).await.unwrap();
        assert_eq!(failures.len(), 1);
        assert_eq!(fs::read(source).unwrap(), b"invalid EPUB");
        assert!(!root.path().join("invalid.epub").exists());
        assert!(!importer.database.has_book_named(&ROOT_DIR_ID, "invalid.epub").unwrap());
    }

    #[test]
    fn dropped_inventory_distinguishes_audiobooks_from_mixed_folders() {
        let root = tempfile::tempdir().unwrap();
        let mixed = root.path().join("Mixed");
        let audio = mixed.join("Audio");
        fs::create_dir_all(&audio).unwrap();
        fs::write(mixed.join("book.epub"), b"book").unwrap();
        fs::write(mixed.join("sample.mp3"), b"audio").unwrap();
        fs::write(audio.join("01.mp3"), b"audio").unwrap();
        fs::write(audio.join("notes.txt"), b"sidecar").unwrap();

        let inventory = inventory(root.path(), &[mixed]).unwrap();
        assert_eq!(inventory.directories, vec![PathBuf::from("Mixed")]);
        assert_eq!(inventory.files, vec![PathBuf::from("Mixed/Audio"), PathBuf::from("Mixed/book.epub")]);
    }

    #[test]
    fn irrelevant_cue_errors_do_not_hide_regular_folder_books() {
        let root = tempfile::tempdir().unwrap();
        let mixed = root.path().join("Mixed");
        fs::create_dir(&mixed).unwrap();
        fs::write(mixed.join("book.epub"), b"book").unwrap();
        fs::write(mixed.join("sample.mp3"), b"audio").unwrap();
        fs::write(mixed.join("first.cue"), b"cue").unwrap();
        fs::write(mixed.join("second.cue"), b"cue").unwrap();

        let inventory = inventory(root.path(), &[mixed]).unwrap();
        assert!(inventory.failures.is_empty());
        assert_eq!(inventory.directories, vec![PathBuf::from("Mixed")]);
        assert_eq!(inventory.files, vec![PathBuf::from("Mixed/book.epub")]);
    }
}
