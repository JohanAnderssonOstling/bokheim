//! Multi-resource library workflows implemented on the public library handle.

use super::covers::generate_thumbnail_for_sync;
use super::handle::LibraryBackend;
use super::model::{Dir, LibraryAccountState, ReaderFormat, ResolvedBook, generate_content_hash_blake3};
use crate::app::BackendError;
use crate::catalog::{CatalogPublicationIdentity, CatalogSourceId};
use crate::library::database as db;
use crate::platform::scanner::LibraryEvent;
use crate::sync::{SyncOutcome, SyncPolicy};
use crate::{BlobKind, ContentHash, DownloadState};
use library_model::BookFormat;
use std::fs;
use std::fs::File;
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
            for content_hash in db::get_book_hashes(&conn, content_hash)? {
                let path = crate::sync::transfer::thumbnail_path_from_content_hash(&self.library_root, &content_hash);
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
        let format = ReaderFormat::from_book_format(source_format);
        let publication_path = path.with_extension(source_format.canonical_extension());
        if exists_locally {
            match File::open(&path) {
                Ok(reader) => return Ok(ResolvedBook { path, publication_path, format, reader: Box::new(reader) }),
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
            let preferences = self.context.load_transfer_preferences()?;
            if preferences.auto_download_limit_mb().is_some_and(|limit| reader.len() <= u64::from(limit) * 1024 * 1024) {
                self.request_book_download(content_hash)?;
            }
            return Ok(ResolvedBook { path, publication_path, format, reader: Box::new(reader) });
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

    pub(crate) async fn synchronize(&self, policy: SyncPolicy) -> Result<SyncOutcome, Box<dyn std::error::Error>> {
        self.sync_manager.synchronize(policy).await
    }

    pub(crate) async fn resume_transfers(&self) -> Result<(), Box<dyn std::error::Error>> {
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
    pub(super) fn import_local_book(&self, parent_id: &DirId, source_path: &Path, file_name: &str) -> Result<ContentHash, Box<dyn std::error::Error>> {
        self.import_book_file(parent_id, source_path, file_name, None)
    }

    pub(super) fn import_catalog_book_file(&self, parent_id: &DirId, source_path: &Path, file_name: &str, source_id: &CatalogSourceId, publication: &CatalogPublicationIdentity) -> Result<ContentHash, Box<dyn std::error::Error>> {
        self.import_book_file(parent_id, source_path, file_name, Some((source_id, publication)))
    }

    fn import_book_file(&self, parent_id: &DirId, source_path: &Path, file_name: &str, catalog_publication: Option<(&CatalogSourceId, &CatalogPublicationIdentity)>) -> Result<ContentHash, Box<dyn std::error::Error>> {
        let mut file_name = file_name.trim().to_owned();
        let extension = Path::new(file_name.as_str()).extension().and_then(|extension| extension.to_str()).unwrap_or_default();
        let source_path = source_path.canonicalize()?;
        if !source_path.is_file() {
            return Err(format!("selected book is not a file: {}", source_path.display()).into());
        }
        let expected_format = BookFormat::from_extension(extension).ok_or("only EPUB, PDF, DRM-free MOBI/AZW/AZW3, FB2, CBZ, HTML, Readium Web Publication, and M4B files can be added to a library")?;
        let inspection = book_metadata::inspect_book(&source_path, expected_format)?;
        let metadata = inspection.metadata;

        // Bundling rewrites the file to a Web Publication, so the format stored on
        // the book is the bundled one rather than the format that was inspected.
        let stored_format = if expected_format == BookFormat::Html { BookFormat::WebPub } else { expected_format };
        let bundled_html = if expected_format == BookFormat::Html {
            let bytes = html_publication::bundle_webpub(&source_path).map_err(|error| format!("could not package Web Publication: {error}"))?;
            let stem = Path::new(file_name.as_str()).file_stem().and_then(|value| value.to_str()).unwrap_or("publication");
            file_name = format!("{stem}.webpub");
            Some(bytes)
        } else {
            None
        };

        if !db::directory_exists(&*self.db.get()?, &parent_id.to_string())? {
            return Err(format!("library folder does not exist: {parent_id}").into());
        }
        let mut conn = self.db.get()?;
        let destination_parent = if *parent_id == sync_common::ROOT_DIR_ID {
            self.library_root.as_path().to_path_buf()
        } else {
            db::materialize_dir_with_markers(&conn, self.library_root.as_path(), &parent_id.to_string())?.ok_or_else(|| format!("could not resolve library folder: {parent_id}"))?
        };
        let destination = destination_parent.join(file_name.as_str());
        if destination.exists() {
            return Err(format!("a book named '{}' already exists in this folder", file_name.as_str()).into());
        }

        let temporary = destination_parent.join(format!("{}-import-{}.tmp", crate::APP_HIDDEN_DIR, uuid::Uuid::new_v4()));
        let mut destination_created = false;
        let copy_result = (|| -> std::io::Result<()> {
            let mut target = fs::OpenOptions::new().write(true).create_new(true).open(&temporary)?;
            if let Some(bytes) = bundled_html.as_deref() {
                use std::io::Write as _;
                target.write_all(bytes)?;
            } else {
                let mut source = File::open(&source_path)?;
                std::io::copy(&mut source, &mut target)?;
            }
            target.sync_all()?;
            match fs::hard_link(&temporary, &destination) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                    // Android's app-data filesystem permits creating and
                    // renaming files but rejects hard links with EACCES.
                    // Recheck the destination before using rename, since it
                    // replaces an existing file on Unix.
                    if destination.exists() {
                        return Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, format!("a book named '{}' already exists in this folder", file_name.as_str())));
                    }
                    fs::rename(&temporary, &destination)?;
                }
                Err(error) => return Err(error),
            }
            destination_created = true;
            match fs::remove_file(&temporary) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error),
            }
        })();
        if let Err(error) = copy_result {
            let _ = fs::remove_file(&temporary);
            if destination_created {
                let _ = fs::remove_file(&destination);
            }
            return Err(error.into());
        }

        let indexed = (|| -> Result<ContentHash, Box<dyn std::error::Error>> {
            let mut hash_buffer = vec![0_u8; 1024 * 1024];
            let content_hash = generate_content_hash_blake3(&destination, &mut hash_buffer)?;
            let local_hash = db::local_file_fingerprint(&destination)?;
            let scan_id = db::next_scan_id(&conn)?;
            let transaction = conn.transaction()?;
            db::insert_scanned_book_metadata(&transaction, content_hash.clone(), &metadata, stored_format)?;
            db::insert_local_book_dir(&transaction, &parent_id, content_hash.clone(), &file_name, local_hash, scan_id)?;
            if let Some((source_id, publication)) = catalog_publication {
                db::record_catalog_publication(&transaction, source_id, publication, &content_hash)?;
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
        drop(conn);

        if let Some(audiobook) = inspection.audiobook {
            let conn = self.db.get()?;
            db::upsert_audiobook_metadata(&conn, &content_hash, &audiobook)?;
        }

        let _ = generate_thumbnail_for_sync(&self.context, &self.library_root, &destination, &content_hash);
        let relative_path = destination.strip_prefix(self.library_root.as_path()).unwrap_or(&destination).to_string_lossy().into_owned();
        let _ = self.event_tx.send(LibraryEvent::BookAdded { content_hash: content_hash.clone(), path: relative_path });
        Ok(content_hash)
    }
}
