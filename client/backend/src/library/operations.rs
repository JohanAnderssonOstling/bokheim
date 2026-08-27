//! Multi-resource library workflows implemented on the public library handle.

use super::filenames;
use super::handle::LibraryBackend;
use super::model::{LibraryAccountState, ResolvedBook};
use crate::catalog::{CatalogPublicationIdentity, CatalogSourceId};
use crate::library::database as db;
use crate::platform::library_events::LibraryEvent;
use crate::sync::{SyncError, SyncOutcome, SyncPolicy};
use crate::DirectoryImport;
use crate::{BlobKind, ContentHash, DownloadState};
use library_model::BookFormat;
use library_model::Dir;
use std::collections::HashMap;
use std::io::Read;
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
        let conn = self.db.get()?;
        Ok(db::get_local_blob(&conn, BlobKind::Thumbnail, &content_hash)?)
    }

    fn placed_book_path(&self, content_hash: ContentHash) -> Result<Option<String>, Box<dyn std::error::Error>> {
        let conn = self.db.get()?;
        let Some(relative_path) = db::get_book_file_path(&conn, content_hash.clone())? else {
            return Ok(None);
        };
        Ok(Some(relative_path.as_str().to_owned()))
    }

    pub(super) fn resolve_book(&self, content_hash: ContentHash) -> Result<ResolvedBook, Box<dyn std::error::Error>> {
        let path = self.placed_book_path(content_hash.clone())?.ok_or_else(|| format!("book placement not found: {content_hash}"))?;
        let (source_format, bytes) = {
            let conn = self.db.get()?;
            let format = db::get_book_format(&conn, &content_hash)?.ok_or_else(|| format!("book format not found: {content_hash}"))?;
            let bytes = db::get_local_blob(&conn, BlobKind::Book, &content_hash)?;
            (format, bytes)
        };
        if !matches!(source_format, BookFormat::Epub | BookFormat::Pdf | BookFormat::Mobi | BookFormat::M4b) {
            return Err(format!("the {} format is no longer supported", source_format.canonical_extension()).into());
        }
        let format = source_format;
        let bytes = bytes.ok_or_else(|| format!("book content is not downloaded: {content_hash}"))?;
        Ok(ResolvedBook { path, format, reader: Box::new(std::io::Cursor::new(bytes)) })
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
        let content_hashes = assets.iter().map(|asset| asset.content_hash).collect::<std::collections::HashSet<_>>();
        for asset in assets {
            db::complete_asset_request(&conn, BlobKind::Book, &asset.content_hash)?;
            db::set_book_downloaded(&conn, &asset.rel_path, asset.content_hash.clone(), false)?;
        }
        for content_hash in content_hashes {
            if !db::is_book_downloaded(conn, content_hash)? {
                db::remove_local_blob(conn, BlobKind::Book, &content_hash)?;
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
    pub(crate) fn adopt_session(&self, session: sync_client::Session) -> Result<(), String> {
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

    pub(super) fn create_library_directory(&self, parent_id: &DirId, name: &str) -> Result<Dir, Box<dyn std::error::Error>> {
        let name = name.trim().to_owned();
        let mut conn = self.db.get()?;
        let created = db::create_directory(&mut conn, parent_id, &name)?;
        let _ = self.event_tx.try_send(LibraryEvent::DirectoryAdded { path: created.relative_path.as_str().to_owned() });
        Ok(created.directory)
    }

    pub(super) fn import_selected_directory(&self, parent_id: &DirId, source: DirectoryImport, create_root: bool) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let DirectoryImport { name, mut directories, mut files } = source;
        let root_id = if create_root { self.create_library_directory(parent_id, &name)?.id } else { *parent_id };
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
            match self.create_library_directory(parent_id, name) {
                Ok(directory) => {
                    imported_directories.insert(path, directory.id);
                }
                Err(error) => failures.push(format!("{}: {error}", path.join("/"))),
            }
        }

        files.sort_by(|left, right| left.path.cmp(&right.path));
        for file in files {
            let Some((file_name, parent_path)) = file.path.split_last() else { continue };
            let extension = file_name.rsplit_once('.').map(|(_, extension)| extension).unwrap_or_default();
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

    pub(super) fn import_book_reader(&self, parent_id: &DirId, source: &mut dyn Read, file_name: String, catalog_publication: Option<(&CatalogSourceId, &CatalogPublicationIdentity)>) -> Result<ContentHash, Box<dyn std::error::Error>> {
        let file_name = filenames::validate_component(&file_name)?;
        let extension = file_name.rsplit_once('.').map(|(_, extension)| extension).unwrap_or_default();
        let expected_format = BookFormat::from_import_extension(extension).ok_or("only EPUB, PDF, DRM-free MOBI/AZW/AZW3, and M4B files can be added to a library")?;

        if !db::directory_exists(&*self.db.get()?, &parent_id.to_string())? {
            return Err(format!("library folder does not exist: {parent_id}").into());
        }
        let mut bytes = Vec::new();
        source.read_to_end(&mut bytes)?;
        if bytes.is_empty() {
            return Err("publication is empty".into());
        }
        let inspection = book_metadata::inspect_book_bytes(&file_name, expected_format, bytes.clone()).map_err(|error| format!("invalid {} publication: {error}", expected_format.canonical_extension().to_ascii_uppercase()))?;
        let metadata = inspection.metadata;
        let stored_format = expected_format;
        let content_hash = ContentHash::new(&blake3::hash(&bytes).to_hex().to_string());

        let content_hash = (|| -> Result<ContentHash, Box<dyn std::error::Error>> {
            let mut conn = self.db.get()?;
            let scan_id = db::next_scan_id(&conn)?;
            let transaction = conn.transaction()?;
            db::insert_scanned_book_metadata(&transaction, content_hash.clone(), &metadata, stored_format)?;
            db::insert_imported_book_dir(&transaction, parent_id, content_hash.clone(), &file_name, None, scan_id)?;
            db::put_local_blob(&transaction, BlobKind::Book, &content_hash, &bytes)?;
            if let Some((source_id, publication)) = catalog_publication {
                db::record_catalog_publication(&transaction, source_id, publication, &content_hash)?;
            }
            if let Some(audiobook) = inspection.audiobook.as_ref() {
                db::upsert_audiobook_metadata(&transaction, &content_hash, audiobook)?;
            }
            transaction.commit()?;
            Ok(content_hash)
        })()?;

        let in_memory_thumbnail_format = match stored_format {
            BookFormat::Epub => Some(thumbnail::InMemoryPublicationFormat::Epub),
            BookFormat::Mobi => Some(thumbnail::InMemoryPublicationFormat::Mobi),
            _ => None,
        };
        if let Some(format) = in_memory_thumbnail_format {
            match thumbnail::generate_thumbnail_from_publication_bytes(format, &bytes) {
                Ok(Some(thumbnail)) => {
                    let conn = self.db.get()?;
                    db::put_local_blob(&conn, BlobKind::Thumbnail, &content_hash, &thumbnail)?;
                    let _ = self.event_tx.try_send(LibraryEvent::ThumbnailGenerated { content_hash });
                }
                Ok(None) => {}
                Err(error) => log::warn!("could not generate thumbnail for imported book {content_hash}: {error}"),
            }
        }

        let relative_path = db::get_book_file_path(&*self.db.get()?, content_hash.clone())?.ok_or("imported book has no placement")?.as_str().to_owned();
        let _ = self.event_tx.try_send(LibraryEvent::BookAdded { content_hash: content_hash.clone(), path: relative_path });
        Ok(content_hash)
    }
}
