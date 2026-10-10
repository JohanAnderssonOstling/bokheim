//! Place external files on disk, then import them in place.

use std::collections::HashMap;
use std::fs;
use std::io::{self, Error};
use std::path::{Path, PathBuf};

use book_model::BookFormat;
use library_model::PublishedStagedBook;
use library_files::filesystem::{place_into_directory, walk};
use sync_common::DirId;

use client_platform_runtime::executor::run_blocking;
use client_runtime::BackendError;
use library_replica::filenames;

use crate::import::validate_import_destination;
use crate::{DirectoryImport, ImportFailure, LibraryImport};

impl LibraryImport {
    pub async fn import_paths(&self, parent: DirId, paths: Vec<PathBuf>, moving: bool) -> Result<Vec<ImportFailure>, BackendError> {
        self.place_and_import_paths(parent, paths, moving).await
    }

    async fn import_destination(&self, parent: DirId) -> Result<PathBuf, BackendError> {
        validate_import_destination(&self.database, &parent)?;
        if !self.assets.local_access_enabled() {
            return Err(BackendError::message("library storage is unavailable"));
        }
        library_placement::prepare_directory(&self.assets, &self.database, parent).await?;
        validate_import_destination(&self.database, &parent)?;
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
        for file in directory.files {
            if cancelled() {
                return Err(BackendError::message("directory import cancelled"));
            }
            if !crate::import_workflow::supported(&file.path) {
                continue;
            }
            let path = selected_path(&payload, &file.path)?;
            if path.try_exists()? {
                failures.push(crate::import_workflow::file_failure(&file.path, "duplicate selected file path"));
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
                failures.push(crate::import_workflow::file_failure(&file.path, &error.to_string()));
            }
        }
        if cancelled() {
            return Err(BackendError::message("directory import cancelled"));
        }
        let paths = if create_root { vec![payload] } else { fs::read_dir(&payload)?.map(|entry| entry.map(|entry| entry.path())).collect::<io::Result<Vec<_>>>()? };
        // Staging is on the destination filesystem: this publishes by rename,
        // without copying again, before book inspection starts.
        failures.extend(self.place_and_import_paths(parent, paths, true).await?);
        Ok(failures)
    }

    async fn place_and_import_paths(&self, parent: DirId, paths: Vec<PathBuf>, moving: bool) -> Result<Vec<ImportFailure>, BackendError> {
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
                    Err(error) => failures.push(failure(&path, &error.to_string())),
                }
            }
            (placed, failures)
        })
        .await
        .map_err(BackendError::operation)?;
        let inventory = run_blocking({
            let root = root.clone();
            move || inventory(&root, &placed)
        })
        .await
        .map_err(BackendError::operation)??;
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
                Err(error) => failures.push(failure(&relative, &error.to_string())),
            }
        }
        for relative in inventory.files {
            let Some(directory) = relative.parent().and_then(|path| parents.get(path)).copied() else {
                failures.push(failure(&relative, "parent folder could not be imported"));
                continue;
            };
            if let Err(error) = self.import_file_in_place(directory, &root, &relative).await {
                failures.push(failure(&relative, &error.to_string()));
            }
        }
        Ok(failures)
    }

    /// Registers a folder placed under `directory`, or reuses the one already
    /// registered there on retry, and marks it on disk with its id.
    fn adopt_directory(&self, directory: DirId, root: &Path, relative: &Path) -> Result<DirId, BackendError> {
        let name = file_name(relative)?;
        if let Some(id) = self.database.child_directory(&directory, name)? {
            fs::write(root.join(relative).join(".biblos_uuid"), id.to_string())?;
            return Ok(id);
        }
        let (_, path) = self.database.directory_creation_path(&directory, name)?;
        if Path::new(&path) != relative {
            return Err(BackendError::message("destination folder changed before import"));
        }
        let id = DirId::new_v4();
        self.database.register_created_directory(&id, &directory, name, &path)?;
        fs::write(root.join(relative).join(".biblos_uuid"), id.to_string())?;
        Ok(id)
    }

    async fn import_file_in_place(&self, parent: DirId, root: &Path, relative: &Path) -> Result<(), BackendError> {
        let name = file_name(relative)?;
        let import = self.validate_import(&parent, name)?;
        let path = root.join(relative);
        let ingested = filesystem_scanner::ingestion::ingest_file(path, name.to_owned(), &*self.cpu).await?;
        let _lease = self.assets.lease_book(&ingested.content_hash).await?;
        // A concurrent folder move must not register these bytes at another path.
        validate_import_destination(&self.database, &parent)?;
        let placement = self.database.native_book_path(parent, name)?.ok_or_else(|| BackendError::message("destination folder no longer exists"))?;
        if Path::new(placement.as_str().trim_start_matches('/')) != relative {
            return Err(BackendError::message("destination folder changed during import"));
        }
        let published = PublishedStagedBook { name: name.to_owned(), relative_path: placement.to_string(), published: true, fingerprint: Some(ingested.fingerprint.to_string()) };
        self.commit_import(import, ingested.content_hash, ingested.size_bytes, Some(ingested.inspection), published)
    }
}

struct Inventory {
    directories: Vec<PathBuf>,
    files: Vec<PathBuf>,
}

fn inventory(root: &Path, placed: &[PathBuf]) -> Result<Inventory, BackendError> {
    let mut directories = Vec::new();
    let mut files = Vec::new();
    for path in placed {
        if path.is_file() {
            files.push(path.strip_prefix(root).map_err(BackendError::operation)?.to_owned());
            continue;
        }
        directories.push(path.strip_prefix(root).map_err(BackendError::operation)?.to_owned());
        let mut walk = walk(path);
        while let Some(entry) = walk.next() {
            let entry = entry?;
            if entry.file_name().to_string_lossy().starts_with('.') {
                if entry.is_dir() {
                    walk.skip_current_dir();
                }
                continue;
            }
            let relative = entry.path().strip_prefix(root).map_err(BackendError::operation)?.to_owned();
            if entry.is_dir() {
                directories.push(relative);
            } else if entry.is_file() && supported(entry.path()) {
                files.push(relative);
            }
        }
    }
    directories.sort();
    directories.dedup();
    files.sort();
    files.dedup();
    Ok(Inventory { directories, files })
}

fn supported(path: &Path) -> bool {
    path.extension().and_then(|extension| extension.to_str()).is_some_and(|extension| BookFormat::from_import_extension(extension).is_some())
}

fn file_name(path: &Path) -> Result<&str, BackendError> {
    path.file_name().and_then(|name| name.to_str()).ok_or_else(|| BackendError::message("invalid file name"))
}

fn failure(path: &Path, message: &str) -> ImportFailure {
    crate::import_workflow::file_failure(&[path.to_string_lossy().into_owned()], message)
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
