//! Multi-resource library workflows implemented on the public library handle.

use super::covers::generate_thumbnail_for_sync;
use super::filenames;
use super::handle::LibraryBackend;
use super::model::{generate_content_hash_blake3, LibraryAccountState, ResolvedBook};
use crate::app::BackendError;
use crate::catalog::{CatalogPublicationIdentity, CatalogSourceId};
use crate::library::database as db;
use crate::platform::scanner::LibraryEvent;
use crate::sync::{SyncError, SyncOutcome, SyncPolicy};
use crate::{BlobKind, ContentHash, DownloadState};
use app_contract::library::{Dir, DirectoryImport};
use library_model::BookFormat;
use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use sync_common::DirId;

impl LibraryBackend {
    fn persistent_download_state(conn: &rusqlite::Connection, content_hash: ContentHash) -> Result<DownloadState, rusqlite::Error> {
        if db::is_book_downloaded(conn, content_hash.clone())? {
            Ok(DownloadState::Downloaded)
        } else if db::is_book_download_requested(conn, content_hash)? {
            Ok(DownloadState::Queued)
        } else {
            Ok(DownloadState::NotDownloaded)
        }
    }

    pub(super) fn load_thumbnail_bytes(&self, content_hash: ContentHash) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error>> {
        let mut bytes = self.context.try_get_thumbnail(content_hash.clone())?;
        if bytes.is_none() {
            let conn = self.db.get()?;
            for equivalent_hash in db::get_book_hashes(&conn, content_hash.clone())? {
                let path = crate::sync::transfer::thumbnail_path_from_content_hash(&self.library_root, &equivalent_hash);
                match fs::read(path) {
                    Ok(found) => {
                        bytes = Some(found);
                        break;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
            }
        }
        if bytes.is_none() {
            let conn = self.db.get()?;
            let is_audiobook = matches!(db::get_book_format(&conn, &content_hash)?, Some(BookFormat::M4b));
            drop(conn);
            if is_audiobook {
                if let Some(path) = self.local_book_path(content_hash.clone())? {
                    if generate_thumbnail_for_sync(&self.context, &self.library_root, &path, &content_hash)? {
                        bytes = self.context.try_get_thumbnail(content_hash)?;
                    }
                }
            }
        }
        Ok(bytes)
    }

    fn resolved_book_path(&self, content_hash: ContentHash) -> Result<Option<(PathBuf, bool)>, Box<dyn std::error::Error>> {
        let conn = self.db.get()?;
        let Some(relative_path) = db::get_book_file_path(&conn, content_hash.clone())? else {
            return Ok(None);
        };
        let fallback = relative_path.resolve_under(&self.library_root);

        if let Some(recorded_path) = db::get_blob_local_path(&conn, BlobKind::Book, &content_hash)? {
            let existing_path = PathBuf::from(recorded_path);
            if existing_path.is_file() {
                return Ok(Some((existing_path, true)));
            }
        }

        for placement in db::get_book_paths_by_content_hash(&conn, &content_hash)? {
            let path = placement.rel_path.resolve_under(&self.library_root);
            if path.is_file() {
                return Ok(Some((path, true)));
            }
        }

        Ok(Some((fallback, false)))
    }

    /// Returns the concrete publication file only when it currently exists on
    /// this device. Unlike reader resolution, this never opens a remote reader
    /// or schedules a download.
    pub fn local_book_path(&self, content_hash: ContentHash) -> Result<Option<PathBuf>, BackendError> {
        self.resolved_book_path(content_hash).map(|resolved| resolved.and_then(|(path, exists_locally)| exists_locally.then_some(path))).map_err(BackendError::operation)
    }

    pub(super) fn resolve_book(&self, content_hash: ContentHash) -> Result<ResolvedBook, Box<dyn std::error::Error>> {
        let (path, exists_locally) = self.resolved_book_path(content_hash.clone())?.ok_or_else(|| format!("book placement not found: {content_hash}"))?;
        let source_format = {
            let conn = self.db.get()?;
            db::get_book_format(&conn, &content_hash)?.ok_or_else(|| format!("book format not found: {content_hash}"))?
        };
        if !matches!(source_format, BookFormat::Epub | BookFormat::Pdf | BookFormat::Mobi | BookFormat::M4b) {
            return Err(format!("the {} format is no longer supported", source_format.canonical_extension()).into());
        }
        let format = source_format;
        if exists_locally {
            match File::open(&path) {
                Ok(reader) => return Ok(ResolvedBook { path, format, reader: Box::new(reader) }),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        {
            let content_hash = {
                let conn = self.db.get()?;
                db::get_book_hashes(&conn, content_hash.clone())?.into_iter().next().ok_or_else(|| format!("book content is unavailable: {content_hash}"))?
            };
            let session = self.sync_manager.current_session().ok_or("authentication is required to open an uncached server book")?;
            let reader = remote_reader::RemoteAssetReader::open(&self.context.app_data_dir().join("remote-book-cache"), session.user_id(), &self.id, content_hash.clone(), self.sync_manager.server_url(), session.token().to_owned())?;
            return Ok(ResolvedBook { path, format, reader: Box::new(reader) });
        }
    }

    /// Evicts downloaded publication files without deleting the catalogue book,
    /// its placements, reading state, or relationships.
    pub(super) fn evict_local_book_copy(&self, content_hash: ContentHash) -> Result<DownloadState, Box<dyn std::error::Error>> {
        let conn = self.db.get()?;
        let assets = db::get_book_assets(&conn)?.into_iter().filter(|asset| asset.content_hash == content_hash && asset.downloaded).collect::<Vec<_>>();
        self.evict_local_assets(&conn, assets)?;
        Ok(Self::persistent_download_state(&conn, content_hash)?)
    }

    /// Evicts only publication placements inside this folder's subtree.
    /// Placements of the same book elsewhere in the library remain local.
    pub(super) fn evict_local_folder_copies(&self, directory_id: &DirId) -> Result<usize, Box<dyn std::error::Error>> {
        let conn = self.db.get()?;
        let prefix = if *directory_id == sync_common::ROOT_DIR_ID {
            "/".to_owned()
        } else {
            let relative = db::directory_relative_path(&conn, directory_id)?.ok_or("folder does not exist")?;
            format!("/{}/", relative.as_str())
        };
        let assets = db::get_book_assets(&conn)?.into_iter().filter(|asset| asset.downloaded && asset.rel_path.as_str().starts_with(&prefix)).collect::<Vec<_>>();
        let removed = assets.len();
        self.evict_local_assets(&conn, assets)?;
        Ok(removed)
    }

    fn evict_local_assets(&self, conn: &rusqlite::Connection, assets: Vec<db::assets::BookAsset>) -> Result<(), Box<dyn std::error::Error>> {
        for asset in assets {
            db::complete_asset_request(&conn, BlobKind::Book, &asset.content_hash)?;
            let path = asset.rel_path.resolve_under(&self.library_root);
            match fs::symlink_metadata(&path) {
                Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                    return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("refusing to evict non-file publication path {}", path.display())).into());
                }
                Ok(_) => fs::remove_file(&path)?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
            db::set_book_downloaded(&conn, &asset.rel_path, asset.content_hash.clone(), false)?;
            if let Some(path) = path.to_str() {
                db::upsert_blob_local_ref(&conn, BlobKind::Book, &asset.content_hash, path, false)?;
            }
        }
        Ok(())
    }

    pub(crate) fn request_folder_download(&self, dir_id: &DirId) -> Result<usize, Box<dyn std::error::Error>> {
        let conn = self.db.get()?;
        let content_hashes = db::get_descendant_content_hashes(&conn, &dir_id.to_string())?
            .into_iter()
            .filter_map(|content_hash| match db::is_book_downloaded(&conn, content_hash.clone()) {
                Ok(false) => Some(Ok(content_hash)),
                Ok(true) => None,
                Err(error) => Some(Err(error)),
            })
            .collect::<Result<Vec<_>, _>>()?;
        drop(conn);
        self.sync_manager.request_downloads(&content_hashes)?;
        Ok(content_hashes.len())
    }

    pub(crate) async fn synchronize(&self, policy: SyncPolicy) -> Result<SyncOutcome, SyncError> {
        self.sync_manager.synchronize(policy).await
    }

    pub(crate) async fn resume_transfers(&self) -> Result<(), SyncError> {
        self.sync_manager.resume_transfers().await
    }

    pub(crate) fn request_book_download(&self, content_hash: ContentHash) -> Result<(), Box<dyn std::error::Error>> {
        self.sync_manager.request_download(content_hash)
    }

    pub(crate) fn account_state(&self) -> Result<LibraryAccountState, String> {
        Ok(LibraryAccountState { user_id: self.sync_manager.user_id(), bound_user_id: self.sync_manager.bound_user_id()?, signed_in: self.sync_manager.is_signed_in() })
    }

    /// Adopt a session obtained out-of-band (e.g. propagated from another live
    /// library after login). Enforces the local account binding.
    pub(crate) fn adopt_session(&self, session: crate::auth::Session) -> Result<(), String> {
        self.sync_manager.adopt_session(session)
    }

    pub(crate) fn clear_session(&self) -> Result<(), String> {
        self.sync_manager.clear_session()
    }

    pub(crate) fn transfer_statuses(&self) -> Vec<crate::TransferStatus> {
        self.sync_manager.transfer_statuses()
    }

    pub(crate) fn transfer_history(&self) -> Vec<crate::TransferStatus> {
        self.sync_manager.transfer_history()
    }

    pub(super) fn create_directory_on_disk(&self, parent_id: &DirId, name: &str) -> Result<Dir, Box<dyn std::error::Error>> {
        let name = name.trim().to_owned();
        let mut conn = self.db.get()?;
        let created = db::create_local_directory(&mut conn, &self.library_root, parent_id, &name)?;
        let _ = self.event_tx.send(LibraryEvent::DirectoryAdded { path: created.relative_path.as_str().to_owned() });
        Ok(created.directory)
    }

    /// Copies and indexes a supported book in one operation. Mobile builds do not run a
    /// filesystem watcher, so importing must not depend on a later scan.
    pub(super) fn import_local_book(&self, parent_id: &DirId, source_path: &Path) -> Result<ContentHash, Box<dyn std::error::Error>> {
        self.import_book_file(parent_id, source_path, None)
    }

    pub(super) fn import_local_folder(&self, parent_id: &DirId, source_path: &Path) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let source_path = source_path.canonicalize()?;
        if !source_path.is_dir() {
            return Err(format!("selected folder is not a directory: {}", source_path.display()).into());
        }
        let folder_name = source_path.file_name().and_then(|name| name.to_str()).ok_or("selected folder has no valid name")?;
        let destination = self.create_directory_on_disk(parent_id, folder_name)?;
        let mut failures = Vec::new();
        self.import_local_folder_contents(&destination.id, &source_path, &mut failures)?;
        Ok(failures)
    }

    pub(super) fn import_selected_directory(&self, parent_id: &DirId, source: DirectoryImport, create_root: bool) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let DirectoryImport { name, mut directories, mut files } = source;
        let root_id = if create_root { self.create_directory_on_disk(parent_id, &name)?.id } else { *parent_id };
        let mut imported_directories = HashMap::from([(Vec::<String>::new(), root_id)]);
        let mut failures = Vec::new();

        directories.sort_by(|left, right| left.len().cmp(&right.len()).then_with(|| left.cmp(right)));
        directories.dedup();
        for path in directories {
            let Some((name, parent_path)) = path.split_last() else { continue };
            let Some(parent_id) = imported_directories.get(parent_path) else {
                failures.push(format!("{}: parent folder could not be imported", path.join("/")));
                continue;
            };
            match self.create_directory_on_disk(parent_id, name) {
                Ok(directory) => {
                    imported_directories.insert(path, directory.id);
                }
                Err(error) => failures.push(format!("{}: {error}", path.join("/"))),
            }
        }

        files.sort_by(|left, right| left.path.cmp(&right.path));
        for file in files {
            let Some((file_name, parent_path)) = file.path.split_last() else { continue };
            let extension = Path::new(file_name).extension().and_then(|extension| extension.to_str()).unwrap_or_default();
            if BookFormat::from_import_extension(extension).is_none() {
                continue;
            }
            let display_path = file.path.join("/");
            let Some(parent_id) = imported_directories.get(parent_path) else {
                failures.push(format!("{display_path}: parent folder could not be imported"));
                continue;
            };
            let mut reader = match futures_executor::block_on((file.open)()) {
                Ok(reader) => reader,
                Err(error) => {
                    failures.push(format!("{display_path}: {error}"));
                    continue;
                }
            };
            if let Err(error) = self.import_book_reader(parent_id, reader.as_mut(), file_name.clone(), None) {
                failures.push(format!("{display_path}: {error}"));
            }
        }
        Ok(failures)
    }

    fn import_local_folder_contents(&self, parent_id: &DirId, source_path: &Path, failures: &mut Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
        let mut entries = fs::read_dir(source_path)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let file_type = entry.file_type()?;
            if file_type.is_symlink() {
                continue;
            }
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if file_type.is_dir() {
                match self.create_directory_on_disk(parent_id, &name) {
                    Ok(directory) => {
                        if let Err(error) = self.import_local_folder_contents(&directory.id, &path, failures) {
                            failures.push(format!("{}: {error}", path.display()));
                        }
                    }
                    Err(error) => failures.push(format!("{}: {error}", path.display())),
                }
                continue;
            }
            if !file_type.is_file() {
                continue;
            }
            let extension = path.extension().and_then(|extension| extension.to_str()).unwrap_or_default();
            if BookFormat::from_import_extension(extension).is_none() {
                continue;
            }
            if let Err(error) = self.import_book_file(parent_id, &path, None) {
                failures.push(format!("{}: {error}", path.display()));
            }
        }
        Ok(())
    }

    pub(super) fn import_catalog_book_file(&self, parent_id: &DirId, source_path: &Path, file_name: &str, source_id: &CatalogSourceId, publication: &CatalogPublicationIdentity) -> Result<ContentHash, Box<dyn std::error::Error>> {
        let file_name = filenames::validate_component(file_name)?;
        let source_path = source_path.canonicalize()?;
        if !source_path.is_file() {
            return Err(format!("downloaded book is not a file: {}", source_path.display()).into());
        }
        self.import_book_file_as(parent_id, &source_path, file_name, Some((source_id, publication)))
    }

    fn import_book_file(&self, parent_id: &DirId, source_path: &Path, catalog_publication: Option<(&CatalogSourceId, &CatalogPublicationIdentity)>) -> Result<ContentHash, Box<dyn std::error::Error>> {
        let source_path = source_path.canonicalize()?;
        if !source_path.is_file() {
            return Err(format!("selected book is not a file: {}", source_path.display()).into());
        }
        let file_name = source_path.file_name().and_then(|name| name.to_str()).ok_or("selected book has no valid UTF-8 filename")?.to_owned();
        self.import_book_file_as(parent_id, &source_path, file_name, catalog_publication)
    }

    fn import_book_file_as(&self, parent_id: &DirId, source_path: &Path, file_name: String, catalog_publication: Option<(&CatalogSourceId, &CatalogPublicationIdentity)>) -> Result<ContentHash, Box<dyn std::error::Error>> {
        let mut source = File::open(source_path)?;
        self.import_book_reader(parent_id, &mut source, file_name, catalog_publication)
    }

    pub(super) fn import_book_reader(&self, parent_id: &DirId, source: &mut dyn Read, file_name: String, catalog_publication: Option<(&CatalogSourceId, &CatalogPublicationIdentity)>) -> Result<ContentHash, Box<dyn std::error::Error>> {
        let extension = Path::new(file_name.as_str()).extension().and_then(|extension| extension.to_str()).unwrap_or_default();
        let expected_format = BookFormat::from_import_extension(extension).ok_or("only EPUB, PDF, DRM-free MOBI/AZW/AZW3, and M4B files can be added to a library")?;

        if !db::directory_exists(&*self.db.get()?, &parent_id.to_string())? {
            return Err(format!("library folder does not exist: {parent_id}").into());
        }
        let destination_parent = {
            let conn = self.db.get()?;
            if *parent_id == sync_common::ROOT_DIR_ID {
                self.library_root.as_path().to_path_buf()
            } else {
                db::materialize_dir_with_markers(&conn, self.library_root.as_path(), &parent_id.to_string())?.ok_or_else(|| format!("could not resolve library folder: {parent_id}"))?
            }
        };
        let destination = destination_parent.join(file_name.as_str());
        if destination.exists() {
            return Err(format!("a book named '{}' already exists in this folder", file_name.as_str()).into());
        }

        let staged = StagedBook::create(&self.library_root, &file_name)?;
        {
            let mut target = fs::OpenOptions::new().write(true).create_new(true).open(staged.path())?;
            std::io::copy(source, &mut target)?;
            target.sync_all()?;
        }
        let inspection = book_metadata::inspect_book(staged.path(), expected_format).map_err(|error| format!("invalid {} publication: {error}", expected_format.canonical_extension().to_ascii_uppercase()))?;
        let metadata = inspection.metadata;
        let stored_format = expected_format;

        let mut hash_buffer = vec![0_u8; 1024 * 1024];
        let content_hash = generate_content_hash_blake3(staged.path(), &mut hash_buffer)?;
        let local_hash = db::local_file_fingerprint(staged.path())?;

        staged.publish(&destination, &file_name)?;

        let indexed = (|| -> Result<ContentHash, Box<dyn std::error::Error>> {
            let mut conn = self.db.get()?;
            let scan_id = db::next_scan_id(&conn)?;
            let transaction = conn.transaction()?;
            db::insert_scanned_book_metadata(&transaction, content_hash.clone(), &metadata, stored_format)?;
            db::insert_local_book_dir(&transaction, &parent_id, content_hash.clone(), &file_name, local_hash, scan_id)?;
            if let Some((source_id, publication)) = catalog_publication {
                db::record_catalog_publication(&transaction, source_id, publication, &content_hash)?;
            }
            if let Some(audiobook) = inspection.audiobook.as_ref() {
                db::upsert_audiobook_metadata(&transaction, &content_hash, audiobook)?;
            }
            transaction.commit()?;
            Ok(content_hash)
        })();
        let content_hash = match indexed {
            Ok(identity) => identity,
            Err(error) => {
                let _ = fs::remove_file(&destination);
                return Err(error);
            }
        };

        if let Err(error) = generate_thumbnail_for_sync(&self.context, &self.library_root, &destination, &content_hash) {
            log::warn!("could not generate thumbnail for imported book {}: {error}", destination.display());
        }
        let relative_path = destination.strip_prefix(self.library_root.as_path()).unwrap_or(&destination).to_string_lossy().into_owned();
        let _ = self.event_tx.send(LibraryEvent::BookAdded { content_hash: content_hash.clone(), path: relative_path });
        Ok(content_hash)
    }
}

struct StagedBook {
    directory: PathBuf,
    path: PathBuf,
}

impl StagedBook {
    fn create(library_root: &Path, file_name: &str) -> std::io::Result<Self> {
        let staging_root = library_root.join(crate::APP_HIDDEN_DIR).join("staging");
        fs::create_dir_all(&staging_root)?;
        let directory = staging_root.join(uuid::Uuid::new_v4().to_string());
        fs::create_dir(&directory)?;
        let path = directory.join(file_name);
        Ok(Self { directory, path })
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn publish(&self, destination: &Path, file_name: &str) -> std::io::Result<()> {
        match fs::hard_link(&self.path, destination) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                if destination.exists() {
                    return Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, format!("a book named '{file_name}' already exists in this folder")));
                }
                fs::rename(&self.path, destination)
            }
            Err(error) => Err(error),
        }
    }
}

impl Drop for StagedBook {
    fn drop(&mut self) {
        match fs::remove_file(&self.path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => {}
        }
        let _ = fs::remove_dir(&self.directory);
    }
}
