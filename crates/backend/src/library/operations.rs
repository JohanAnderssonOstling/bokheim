//! Multi-resource library workflows implemented on the public library handle.

use super::covers::generate_thumbnail_for_sync;
use super::handle::LibraryBackend;
use super::model::{generate_content_hash_blake3, Dir, LibraryAccountState, ReaderFormat, ResolvedBook};
use crate::library::database as db;
use crate::platform::scanner::LibraryEvent;
use crate::sync::{SyncOutcome, SyncPolicy};
use crate::{BlobKind, ContentHash, DownloadState};
use domain::BookFormat;
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

    fn resolved_book_path(&self, content_hash: ContentHash) -> Result<Option<PathBuf>, Box<dyn std::error::Error>> {
        let conn = self.db.get()?;
        let Some(relative_path) = db::get_book_file_path(&conn, content_hash)? else {
            return Ok(None);
        };
        Ok(Some(self.library_root.resolve_book(&relative_path)))
    }

    pub(super) fn resolve_book(&self, content_hash: ContentHash) -> Result<ResolvedBook, Box<dyn std::error::Error>> {
        let path = self.resolved_book_path(content_hash.clone())?.ok_or_else(|| format!("book placement not found: {content_hash}"))?;
        let format = ReaderFormat::from_path(&path);
        match File::open(&path) {
            Ok(reader) => return Ok(ResolvedBook { path, format, reader: Box::new(reader) }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
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
            return Ok(ResolvedBook { path, format, reader: Box::new(reader) });
        }
    }

    /// Evicts downloaded publication files without deleting the catalogue book,
    /// its placements, reading state, or relationships.
    pub(super) fn evict_local_book_copy(&self, content_hash: ContentHash) -> Result<DownloadState, Box<dyn std::error::Error>> {
        let conn = self.db.get()?;
        for asset in db::get_book_assets(&conn)?.into_iter().filter(|asset| asset.content_hash == content_hash && asset.downloaded) {
            db::complete_asset_request(&conn, BlobKind::Book, &asset.content_hash)?;
            let path = self.library_root.resolve_book(&asset.rel_path);
            match fs::symlink_metadata(&path) {
                Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                    return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("refusing to evict non-file publication path {}", path.display())).into());
                }
                Ok(_) => fs::remove_file(&path)?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
            db::set_book_downloaded(&conn, &asset.rel_path, content_hash.clone(), false)?;
            if let Some(path) = path.to_str() {
                db::upsert_blob_local_ref(&conn, BlobKind::Book, &asset.content_hash, path, false)?;
            }
        }
        Ok(Self::persistent_download_state(&conn, content_hash)?)
    }

    pub(crate) fn request_folder_download(&self, dir_id: &DirId) -> Result<usize, Box<dyn std::error::Error>> {
        let conn = self.db.get()?;
        let content_hashes = db::get_descendant_content_hashes(&conn, dir_id.as_str())?
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

    pub(crate) fn transfer_statuses(&self) -> Vec<crate::TransferStatus> {
        self.sync_manager.transfer_statuses()
    }

    pub(super) fn create_directory_on_disk(&self, parent_id: &str, name: &str) -> Result<Dir, Box<dyn std::error::Error>> {
        let parent_id = DirId::parse(parent_id)?;
        let name = sync_common::FileName::parse(name.trim())?;
        let mut conn = self.db.get()?;
        let created = db::create_local_directory(&mut conn, &self.library_root, &parent_id, &name)?;
        let _ = self.event_tx.send(LibraryEvent::DirectoryAdded { path: created.relative_path.as_str().to_owned() });
        Ok(created.directory)
    }

    /// Copies and indexes a supported book in one operation. Mobile builds do not run a
    /// filesystem watcher, so importing must not depend on a later scan.
    pub(super) fn import_local_book(&self, parent_id: &str, source_path: &Path, file_name: &str) -> Result<ContentHash, Box<dyn std::error::Error>> {
        self.import_book_file(parent_id, source_path, file_name)
    }

    fn import_book_file(&self, parent_id: &str, source_path: &Path, file_name: &str) -> Result<ContentHash, Box<dyn std::error::Error>> {
        let parent_id = DirId::parse(parent_id)?;
        let mut file_name = sync_common::FileName::parse(file_name.trim())?;
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
            file_name = sync_common::FileName::parse(&format!("{stem}.webpub"))?;
            Some(bytes)
        } else {
            None
        };

        if !db::directory_exists(&*self.db.get()?, parent_id.as_str())? {
            return Err(format!("library folder does not exist: {parent_id}").into());
        }
        let mut conn = self.db.get()?;
        let destination_parent = if parent_id.as_str() == "__root__" {
            self.library_root.as_path().to_path_buf()
        } else {
            db::materialize_dir_with_markers(&conn, self.library_root.as_path(), parent_id.as_str())?.ok_or_else(|| format!("could not resolve library folder: {parent_id}"))?
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
            fs::hard_link(&temporary, &destination)?;
            destination_created = true;
            fs::remove_file(&temporary)
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
