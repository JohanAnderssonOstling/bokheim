use super::TransfersSql;
use super::{BookUploadCompletion, BookUploadSnapshot};
use crate::shared_sql::SharedSql;
use crate::*;
use library_replica::BlobKind;
use sync_common::ContentHash;

impl Database {
    fn local_versions(&self, hash: &ContentHash) -> Result<Vec<(String, String, String)>, DatabaseError> {
        Self::local_versions_of(&self.connection, hash)
    }

    fn local_versions_of(connection: &rusqlite::Connection, hash: &ContentHash) -> Result<Vec<(String, String, String)>, DatabaseError> {
        let mut versions = Vec::new();
        connection.transfer_local_versions(hash.as_str(), |row| {
            versions.push((row.get(0)?, row.get(1)?, row.get(2)?));
            Ok(())
        })?;
        Ok(versions)
    }

    fn upload_intent(&self, hash: &ContentHash) -> Result<Option<BookUploadIntent>, DatabaseError> {
        Self::upload_intent_of(&self.connection, hash)
    }

    pub(super) fn upload_intent_of(connection: &rusqlite::Connection, hash: &ContentHash) -> Result<Option<BookUploadIntent>, DatabaseError> {
        let mut intent = None;
        connection.transfer_upload_intent(hash.as_str(), |row| {
            intent = Some(BookUploadIntent { id: row.get(0)?, content_hash: hash.clone(), checksum: ContentHash::new(&row.get::<_, String>(1)?), size_bytes: row.get::<_, i64>(2)? as u64 });
            Ok(())
        })?;
        Ok(intent)
    }

    pub fn ready_upload_hashes(&self, hashes: &[ContentHash]) -> Result<std::collections::HashSet<ContentHash>, DatabaseError> {
        let hashes = serde_json::to_string(&hashes.iter().map(ContentHash::as_str).collect::<Vec<_>>()).map_err(DatabaseError::operation)?;
        let mut ready = std::collections::HashSet::new();
        self.connection.transfer_ready_uploads(&hashes, |row| {
            ready.insert(ContentHash::new(&row.get::<_, String>(0)?));
            Ok(())
        })?;
        Ok(ready)
    }

    pub fn book_upload_intents(&self, hashes: &[ContentHash]) -> Result<Vec<BookUploadIntent>, DatabaseError> {
        hashes.iter().filter_map(|hash| self.upload_intent(hash).transpose()).collect()
    }

    pub fn pending_book_uploads(&self, after: i64, limit: i64) -> Result<Vec<BookUploadIntent>, DatabaseError> {
        let mut intents = Vec::new();
        self.connection.transfer_pending_book_uploads(after, limit, super::rejection_cutoff()?, |row| {
            intents.push(BookUploadIntent { id: row.get(0)?, content_hash: ContentHash::new(&row.get::<_, String>(1)?), checksum: ContentHash::new(&row.get::<_, String>(2)?), size_bytes: row.get::<_, i64>(3)? as u64 });
            Ok(())
        })?;
        Ok(intents)
    }

    /// Covers are derived from local thumbnail state and confirmed parent
    /// presence, rather than being represented by a second durable queue.
    pub fn pending_cover_uploads(&self, limit: i64) -> Result<Vec<ContentHash>, DatabaseError> {
        let mut hashes = Vec::new();
        self.connection.transfer_pending_cover_uploads(limit, super::rejection_cutoff()?, |row| {
            hashes.push(ContentHash::new(&row.get::<_, String>(0)?));
            Ok(())
        })?;
        Ok(hashes)
    }

    pub fn book_upload_snapshot(&self, hash: &ContentHash) -> Result<BookUploadSnapshot, DatabaseError> {
        Ok(BookUploadSnapshot { local_versions: self.local_versions(hash)?, intent: self.upload_intent(hash)? })
    }

    pub fn complete_book_upload_if_current(&self, hash: &ContentHash, expected_versions: &[(String, String, String)], intent: Option<&BookUploadIntent>) -> Result<bool, DatabaseError> {
        let transaction = self.connection.unchecked_transaction()?;
        let current = Self::complete_book_upload_in_transaction(&transaction, hash, expected_versions, intent)?;
        transaction.commit()?;
        Ok(current)
    }

    pub fn complete_book_upload_batch(&self, completed: &[BookUploadCompletion]) -> Result<Vec<bool>, DatabaseError> {
        let transaction = self.connection.unchecked_transaction()?;
        let mut current = Vec::with_capacity(completed.len());
        for completion in completed {
            current.push(Self::complete_book_upload_in_transaction(&transaction, &completion.content_hash, &completion.local_versions, Some(&completion.intent))?);
        }
        transaction.commit()?;
        Ok(current)
    }

    pub(super) fn complete_book_upload_in_transaction(connection: &rusqlite::Connection, hash: &ContentHash, expected_versions: &[(String, String, String)], intent: Option<&BookUploadIntent>) -> Result<bool, DatabaseError> {
        let current = Self::local_versions_of(connection, hash)? == expected_versions;
        if current {
            connection.record_remote_asset(BlobKind::Book.storage(), hash.as_str())?;
            if let Some(intent) = intent {
                connection.transfer_complete_upload(intent.id, intent.content_hash.as_str(), intent.checksum.as_str())?;
            }
        } else {
            connection.forget_remote_asset(BlobKind::Book.storage(), hash.as_str())?;
        }
        Ok(current)
    }

    pub fn settle_book_manifest(&self, intents: &[BookUploadIntent], present_remotely: &[ContentHash], rejected: &[(ContentHash, String)]) -> Result<Vec<BookUploadIntent>, DatabaseError> {
        let transaction = self.connection.unchecked_transaction()?;
        let remaining = Self::settle_book_manifest_in_transaction(&transaction, intents, present_remotely, rejected)?;
        transaction.commit()?;
        Ok(remaining)
    }

    pub(super) fn settle_book_manifest_in_transaction(connection: &rusqlite::Connection, intents: &[BookUploadIntent], present_remotely: &[ContentHash], rejected: &[(ContentHash, String)]) -> Result<Vec<BookUploadIntent>, DatabaseError> {
        let mut remaining = Vec::new();
        for intent in intents {
            if Self::upload_intent_of(connection, &intent.content_hash)?.as_ref() != Some(intent) {
                continue;
            }
            if present_remotely.contains(&intent.content_hash) {
                connection.record_remote_asset(BlobKind::Book.storage(), intent.content_hash.as_str())?;
                connection.transfer_complete_upload(intent.id, intent.content_hash.as_str(), intent.checksum.as_str())?;
            } else if let Some((_, reason)) = rejected.iter().find(|(hash, _)| *hash == intent.content_hash) {
                connection.transfer_record_book_rejection(intent.content_hash.as_str(), reason)?;
            } else {
                remaining.push(intent.clone());
            }
        }
        Ok(remaining)
    }

    pub fn book_uploads_waiting_for_storage(&self) -> Result<bool, DatabaseError> {
        self.connection.transfer_uploads_waiting_for_storage(super::rejection_cutoff()?, |row| row.get(0)).map_err(Into::into)
    }

    pub fn book_upload_rejection_reason(&self, hash: &ContentHash) -> Result<Option<String>, DatabaseError> {
        let mut reason = None;
        self.connection.transfer_book_upload_rejection_reason(hash.as_str(), super::rejection_cutoff()?, |row| {
            reason = row.get(0)?;
            Ok(())
        })?;
        Ok(reason)
    }

    /// Queues an upload of bytes no placement of the book has recorded.
    pub fn observe_local_book(&self, hash: &ContentHash, checksum: &ContentHash, size_bytes: u64) -> Result<(), DatabaseError> {
        let size = i64::try_from(size_bytes).map_err(DatabaseError::operation)?;
        let transaction = self.connection.unchecked_transaction()?;
        if !checksum_seen(&transaction, hash.as_str(), checksum.as_str())? {
            Self::enqueue_version(&transaction, hash, checksum, size)?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn observe_local_placement_book(&self, dir: &sync_common::DirId, hash: &ContentHash, checksum: &ContentHash, size_bytes: u64) -> Result<(), DatabaseError> {
        let size = i64::try_from(size_bytes).map_err(DatabaseError::operation)?;
        let transaction = self.connection.unchecked_transaction()?;
        record_local_version(&transaction, &dir.to_string(), hash, checksum, size)?;
        transaction.commit()?;
        Ok(())
    }

    fn enqueue_version(connection: &rusqlite::Connection, hash: &ContentHash, checksum: &ContentHash, size: i64) -> Result<(), DatabaseError> {
        connection.shared_scanner_enqueue_upload(hash.as_str(), checksum.as_str(), size)?;
        connection.shared_rejected_book_cleanup(hash.as_str())?;
        Ok(())
    }

    pub fn book_upload_intent(&self, hash: &ContentHash) -> Result<Option<BookUploadIntent>, DatabaseError> {
        self.upload_intent(hash)
    }

    pub fn queue_book_upload(&self, hash: &ContentHash, checksum: &ContentHash, size_bytes: u64) -> Result<(), DatabaseError> {
        let size = i64::try_from(size_bytes).map_err(DatabaseError::operation)?;
        self.connection.shared_scanner_enqueue_upload(hash.as_str(), checksum.as_str(), size)?;
        Ok(())
    }
}

fn checksum_seen(connection: &rusqlite::Connection, hash: &str, checksum: &str) -> Result<bool, DatabaseError> {
    let mut seen = false;
    connection.shared_checksum_seen(hash, checksum, |row| {
        seen = row.get(0)?;
        Ok(())
    })?;
    Ok(seen)
}

/// Records the bytes now at a placement. New bytes, or bytes replacing a
/// recorded version, are queued for upload.
pub(crate) fn record_local_version(connection: &rusqlite::Connection, dir: &str, hash: &ContentHash, checksum: &ContentHash, size: i64) -> Result<(), DatabaseError> {
    let mut previous = None;
    connection.shared_current_checksum_select(dir, hash.as_str(), |row| {
        previous = Some(row.get::<_, String>(0)?);
        Ok(())
    })?;
    if previous.as_deref() == Some(checksum.as_str()) {
        return Ok(());
    }
    if previous.is_some() || !checksum_seen(connection, hash.as_str(), checksum.as_str())? {
        Database::enqueue_version(connection, hash, checksum, size)?;
    }
    connection.shared_record_checksum(dir, hash.as_str(), checksum.as_str(), size)?;
    Ok(())
}
