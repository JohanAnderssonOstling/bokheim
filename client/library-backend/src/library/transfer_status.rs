//! A library-owned snapshot for app-level transfer presentation.

use super::LibrarySession;
use crate::BackendError;
use crate::ContentHash;
use std::sync::atomic::Ordering;

impl LibrarySession {
    pub(crate) fn transfer_snapshot(&self) -> Result<library_model::LibraryActivitySnapshot, BackendError> {
        let sync = self.sync.clone();
        let library_name = {
            use library_database::shared_sql::SharedSql;
            let mut name: Option<String> = None;
            self.db
                .connection
                .shared_owner_remote_library_name(|row| {
                    name = row.get(0)?;
                    Ok(())
                })
                .map_err(BackendError::operation)?;
            name
        }
        .unwrap_or_else(|| "Library".to_owned());
        let file_work_pending = self.db.has_pending_file_work()?;
        let file_work_error = if file_work_pending { self.file_work_error.borrow().clone() } else { None };
        let mut transfers = sync.transfer_queue.statuses();
        let mut history = sync.transfer_queue.history();
        let hashes: Vec<ContentHash> =
            transfers.iter_mut().chain(history.iter_mut()).filter_map(|transfer| if transfer.state == library_model::TransferState::Failed { transfer.failed_content_hash } else { Some(transfer.content_hash) }).collect();
        let file_names: std::collections::HashMap<ContentHash, String> = self.db.transfer_first_file_names(&hashes).map_err(BackendError::operation)?.into_iter().collect();
        for transfer in transfers.iter_mut().chain(history.iter_mut()) {
            let hash = if transfer.state == library_model::TransferState::Failed { transfer.failed_content_hash } else { Some(transfer.content_hash) };
            transfer.file_name = hash.and_then(|hash| file_names.get(&hash).cloned());
        }
        let scanning = self.filesystem_scanning.get();
        let scan_progress = self.runtime.directory_import_progress();
        let signed_in = sync.account().map(|account| account.current().ok().flatten().is_some()).unwrap_or(false);
        let library_database::LibraryWork { total_books, uploaded_books, pending_uploads, pending_covers, pending_changes, pending_thumbnails, scan_pending, scan_failed, sync_configured } = ({
            use library_database::assets::ActivitySql;
            use library_database::{DatabaseError, LibraryWork};
            let database = &self.db;
            let mut work = None;
            database
                .connection
                .activity_progress(|row| {
                    let count = |index| row.get::<_, i64>(index).map(|value| value.max(0) as u64);
                    work = Some(LibraryWork {
                        total_books: count(0)?,
                        uploaded_books: count(1)?,
                        pending_uploads: count(2)?,
                        pending_changes: count(3)?,
                        pending_thumbnails: count(4)?,
                        pending_covers: count(5)?,
                        scan_pending: row.get(6)?,
                        scan_failed: row.get(7)?,
                        sync_configured: row.get(8)?,
                    });
                    Ok(())
                })
                .map_err(BackendError::operation)?;
            work.ok_or_else(|| DatabaseError::message("library activity query returned no row"))
        })
        .map_err(BackendError::operation)?;
        let sync_configured = sync_configured || signed_in;
        let uploads_enabled = sync_configured && sync.asset_storage_enabled();
        let storage_quota_blocked = sync.asset_storage_enabled()
            && (({
                use library_database::TransfersSql;
                let cutoff = library_database::assets::rejection_cutoff().map_err(BackendError::operation)?;
                self.db.connection.transfer_uploads_waiting_for_storage(cutoff, |row| row.get(0))
            })
            .map_err(BackendError::operation)?
                || transfers.iter().any(|transfer| {
                    transfer.kind == library_model::TransferJobKind::UploadBook
                        && matches!(transfer.state, library_model::TransferState::Retrying | library_model::TransferState::Failed)
                        && matches!(transfer.last_error.as_deref(), Some("book storage quota exceeded" | "batch book upload HTTP 507"))
                }));
        Ok(library_model::LibraryActivitySnapshot {
            storage_quota_blocked,
            sync_configured,
            signed_in,
            pending_uploads: if uploads_enabled { pending_uploads } else { 0 },
            pending_covers: if uploads_enabled { pending_covers } else { 0 },
            pending_changes: if sync_configured { pending_changes } else { 0 },
            pending_thumbnails,
            scan_pending,
            scan_failed,
            file_work_pending,
            file_work_error,
            library_id: self.id,
            library_name,
            scanner_running: scanning || self.runtime.directory_import_progress().is_some(),
            thumbnail_work_running: self.runtime.thumbnails.running(),
            thumbnail_progress: self.runtime.thumbnails.progress(),
            thumbnail_waiting: self.runtime.thumbnails.waiting(),
            thumbnail_download_coverage: self.db.thumbnail_download_coverage().map_err(BackendError::operation)?,
            metadata_sync_running: sync.metadata_activity.load(Ordering::Acquire) > 0,
            preparing_transfers: sync.asset_activity.load(Ordering::Acquire) > 0,
            total_books,
            uploaded_books,
            scan_progress,
            transfers,
            history,
        })
    }
}
