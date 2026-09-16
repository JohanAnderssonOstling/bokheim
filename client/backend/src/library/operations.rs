//! Multi-resource library workflows implemented on the public library handle.
use super::LibrarySession;
use super::filenames;
use crate::BackendError;
use crate::{BlobKind, ContentHash, DownloadState};
use library_model::BookFormat;
use library_model::Dir;
use std::collections::HashMap;
use std::io::Read;
use sync_common::DirId;

pub(super) struct ValidatedImport {
    pub(super) parent_id: DirId,
    pub(super) file_name: String,
    pub(super) format: BookFormat,
}

use client_library::toc::inspected_navigation;
#[cfg(test)]
use client_library::toc::pdf_navigation;

fn validate_import_destination(conn: &rusqlite::Connection, parent_id: &DirId) -> Result<(), BackendError> {
    let live: bool = client_library::placements::is_live_destination(conn, parent_id)?;
    if !live {
        return Err(format!("library folder does not exist: {parent_id}").into());
    }
    Ok(())
}

impl LibrarySession {
    pub(super) fn persistent_download_state(&self, conn: &rusqlite::Connection, content_hash: ContentHash) -> Result<DownloadState, BackendError> {
        if client_library::assets::is_book_downloaded(conn, content_hash)? && self.assets.exists(BlobKind::Book, &content_hash)? {
            Ok(DownloadState::Downloaded)
        } else if client_library::assets::is_book_download_requested(conn, content_hash)? {
            Ok(DownloadState::Queued)
        } else {
            Ok(DownloadState::NotDownloaded)
        }
    }

    pub(crate) async fn thumbnail(&self, content_hash: ContentHash, resolution: crate::ThumbnailResolution) -> Result<Option<Vec<u8>>, BackendError> {
        let bytes = match resolution {
            crate::ThumbnailResolution::Browse => self.assets.read_thumbnail_variant(&content_hash, resolution.width()).await?,
            crate::ThumbnailResolution::HighDensity => self.assets.read_bytes(BlobKind::Thumbnail, &content_hash).await?,
        };
        if bytes.is_none() && self.assets.exists(BlobKind::Book, &content_hash)? {
            let conn = self.db.get()?;
            if client_library::assets::get_book_format(&conn, &content_hash)?.is_some() {
                client_library::thumbnail_jobs::request_missing_thumbnail(&conn, &content_hash)?;
            }
        }
        Ok(bytes)
    }

    /// Evicts downloaded publication files without deleting the library book,
    /// its placements, reading state, or relationships.
    pub(crate) fn remove_local_book_copy(&self, content_hash: ContentHash) -> Result<DownloadState, BackendError> {
        let mut conn = self.db.get()?;
        self.assets.evict_local_book(&mut conn, &content_hash)?;
        self.persistent_download_state(&conn, content_hash)
    }

    pub fn create_directory(&self, parent_id: &DirId, name: &str) -> Result<Dir, BackendError> {
        let name = name.trim().to_owned();
        let mut conn = self.db.get()?;
        let created = client_library::placements::create_directory(&mut conn, parent_id, &name)?;
        drop(conn);
        self.notify_contents_changed();
        Ok(created)
    }

    pub(crate) async fn prepare_directory_import(&self, parent_id: &DirId, name: Option<String>, directories: Vec<Vec<String>>) -> Result<super::PreparedDirectoryImport, BackendError> {
        self.prepare_directory_import_job(parent_id, name, directories, None).await
    }

    pub(crate) async fn prepare_directory_import_job(&self, parent_id: &DirId, name: Option<String>, mut directories: Vec<Vec<String>>, job: Option<String>) -> Result<super::PreparedDirectoryImport, BackendError> {
        validate_import_destination(&*self.db.get()?, parent_id)?;
        if let Some(job) = &job {
            uuid::Uuid::parse_str(job).map_err(BackendError::operation)?;
        }
        let create = |parent: &DirId, name: &str, path: &[String]| -> Result<Dir, BackendError> {
            if let Some(job) = &job {
                let hash = blake3::hash(&serde_json::to_vec(&(job, path)).map_err(BackendError::operation)?);
                let id = DirId::from_bytes(hash.as_bytes()[..16].try_into().unwrap());
                let name = filenames::validate_component(name)?;
                Ok(client_library::placements::create_directory_with_id(&mut *self.db.get()?, &id, parent, &name)?)
            } else {
                self.create_directory(parent, name)
            }
        };
        // Let queued import status reach subscribers before doing SQLite work;
        // large directory trees must not monopolize the executor.
        crate::executor::sleep(std::time::Duration::ZERO).await;
        let root_id = match name {
            Some(name) => create(parent_id, &name, &[])?.id,
            None => *parent_id,
        };
        let mut imported_directories = HashMap::from([(Vec::<String>::new(), root_id)]);
        let mut failures = Vec::new();

        // A parent path sorts before every path that extends it.
        directories.sort_unstable();
        directories.dedup();
        let mut slice_started = web_time::Instant::now();
        for path in directories {
            if slice_started.elapsed() >= std::time::Duration::from_millis(8) {
                crate::executor::sleep(std::time::Duration::ZERO).await;
                slice_started = web_time::Instant::now();
            }
            let Some((name, parent_path)) = path.split_last() else { continue };
            let Some(parent_id) = imported_directories.get(parent_path) else {
                failures.push(crate::import_workflow::folder_failure(&path, "parent folder could not be imported"));
                continue;
            };
            match create(parent_id, name, &path) {
                Ok(directory) => {
                    imported_directories.insert(path, directory.id);
                }
                Err(error) => failures.push(crate::import_workflow::folder_failure(&path, &error.to_string())),
            }
        }

        Ok(super::PreparedDirectoryImport { directories: imported_directories.into_iter().collect(), failures })
    }

    pub(super) fn validate_import(&self, parent_id: &DirId, file_name: &str) -> Result<ValidatedImport, BackendError> {
        let file_name = filenames::validate_component(file_name)?;
        let extension = file_name.rsplit_once('.').map(|(_, extension)| extension).unwrap_or_default();
        let format = BookFormat::from_import_extension(extension).ok_or("only EPUB, PDF, DRM-free MOBI/AZW/AZW3, and M4B files can be added to a library")?;
        validate_import_destination(&*self.db.get()?, parent_id)?;
        Ok(ValidatedImport { parent_id: *parent_id, file_name, format })
    }

    pub(crate) async fn import_book_reader(&self, parent_id: &DirId, source: Box<dyn Read + Send>, file_name: String) -> Result<ContentHash, BackendError> {
        let import = self.validate_import(parent_id, &file_name)?;
        let staged = self.assets.stage_import(&self.db, *parent_id, import.file_name.clone(), source).await?;
        self.finish_import(import, staged).await
    }

    pub(super) async fn finish_import(&self, import: ValidatedImport, staged: crate::storage::files::StagedBook) -> Result<ContentHash, BackendError> {
        let reuse = import.format != BookFormat::Pdf && client_library::import_inspection::has_import_inspection(&*self.db.get()?, &staged.content_hash(), import.format)?;
        let inspection = if reuse {
            None
        } else {
            let reader = staged.open_reader().map_err(BackendError::operation)?;
            Some(
                crate::worker_cpu::submit_publication(crate::worker_cpu::Inspect { source_name: import.file_name.clone(), format: import.format }, reader)
                    .await
                    .map_err(|error| BackendError::operation(format!("invalid {} publication: {error}", import.format.canonical_extension().to_ascii_uppercase())))?,
            )
        };
        let content_hash = staged.content_hash();
        let checksum = staged.checksum();
        let size_bytes = staged.size_bytes();
        let _lease = self.assets.lease_book(&content_hash).await.map_err(|error| BackendError::operation(format!("could not lock the imported book: {error}")))?;
        let prepared = staged.prepare_import().await?;

        // Publication precedes the metadata commit. On a database failure the
        // library file remains discoverable by the next scan.
        self.commit_import(import, content_hash, checksum, size_bytes, inspection.as_ref(), prepared)?;
        drop(_lease);
        Ok(content_hash)
    }

    /// Committing the metadata transaction is the import's success point.
    /// Resolve all fallible result data before it, and notify before optional work.
    fn commit_import(
        &self, import: ValidatedImport, content_hash: ContentHash, checksum: ContentHash, size_bytes: u64, inspection: Option<&book_metadata::InspectedBook>, prepared: crate::storage::files::PreparedImport,
    ) -> Result<(), BackendError> {
        let ValidatedImport { parent_id, file_name, format } = import;
        let mut conn = self.db.get()?;
        prepared.prepare_destination(&self.assets, &mut conn, parent_id).map_err(|error| BackendError::operation(format!("could not prepare the destination library folder for publication: {error}")))?;
        // Acquire the writer before reading the destination. A deferred
        // transaction cannot upgrade a stale snapshot after a concurrent scan.
        let transaction = client_library::schema::library_write_transaction(&mut conn)?;
        validate_import_destination(&transaction, &parent_id)?;
        let scan_id = client_library::scanner::next_scan_id(&transaction)?;
        if let Some(inspection) = inspection {
            client_library::metadata::insert_scanned_book_metadata(&transaction, content_hash, &inspection.metadata, format)?;
            if let Some(audiobook) = inspection.audiobook.as_ref() {
                client_library::audiobooks::upsert_audiobook_metadata(&transaction, &content_hash, audiobook)?;
            }
            // A document that yielded nothing is still recorded, so that nothing
            // parses it again hoping for a different answer.
            if let Some((source, entries)) = inspected_navigation(format, &inspection.toc, inspection.pdf.as_ref()) {
                client_library::toc::upsert_book_toc(&transaction, &content_hash, source, &entries)?;
            }
            if let Some(metadata) = &inspection.pdf {
                client_library::pdf_metadata::upsert(&transaction, &content_hash, &checksum, metadata)?;
            }
            client_library::import_inspection::record_import_inspection(&transaction, &content_hash, format)?;
        } else if !client_library::import_inspection::has_import_inspection(&transaction, &content_hash, format)? {
            return Err("book changed during import; retry the import".into());
        }
        client_library::scanner::insert_imported_book_dir(&transaction, &parent_id, content_hash, &file_name, None, scan_id)?;
        if !self.assets.thumbnail_set_exists(&content_hash)? {
            client_library::thumbnail_jobs::request_missing_thumbnail(&transaction, &content_hash)?;
        }
        let published = prepared.publish(&self.assets, &transaction, &parent_id).map_err(|error| BackendError::operation(format!("could not publish the imported book: {error}")))?;
        if published {
            client_library::assets::observe_local_placement_publication(&transaction, &parent_id, &content_hash, &checksum, size_bytes)?;
        }
        self.assets.reconcile_import_placements(&transaction, &content_hash)?;
        transaction.commit()?;
        self.notify_contents_changed();
        self.notify_storage_changed();
        Ok(())
    }
}
