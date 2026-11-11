//! Copy into a separate library; provider files and the source stay untouched.
use super::{LibraryService, StorageCopyJournal};
use crate::BackendError;
use crate::LibraryHandle;
use client_protocol::Failure;
use library_model::CloudQuotaBook;
use library_registry::LibraryEntry;
use std::sync::Arc;
use sync_common::{LibraryId, api::assets::BlobManifestEntry, storage::LibraryStorage};

impl LibraryService {
    pub async fn copy_library_storage(self: &Arc<Self>, source: LibraryId, bokheim: bool) -> Result<LibraryEntry, BackendError> {
        // Requests from different windows must resume the same pending copy.
        let _copy = self.config.storage_copy_lock.lock().await;
        let original = self.handle(&source)?;
        self.copy_library_storage_inner(&original, bokheim).await
    }

    async fn copy_library_storage_inner(self: &Arc<Self>, original: &LibraryHandle, bokheim: bool) -> Result<LibraryEntry, BackendError> {
        let source = *original.id();
        self.entry(&source).await?;
        if bokheim && self.current_account().await?.is_none() {
            return Err(BackendError::classified(Failure::SignInRequired, "sign in before copying a library to Bokheim storage"));
        }
        let key = format!("storage-copy:v1:{source}:{}", u8::from(bokheim));
        let _quota = if bokheim {
            let books: Vec<CloudQuotaBook> = original.cloud_quota_books().await?;
            Some(self.admit_cloud_capacity(books.into_iter().map(|book| BlobManifestEntry { byte_checksum: book.byte_checksum, book_id: book.book_id, size_bytes: book.size_bytes }).collect(), 0).await?)
        } else {
            None
        };
        let journal_key = key.clone();
        let (entry, mut journal) = self
            .run(move |this| {
                let original = this.entry_on_storage(&source)?;
                let journal: Option<StorageCopyJournal> = this.config.registry.read_value(&journal_key).map_err(BackendError::operation)?.map(|bytes| serde_json::from_slice(&bytes).map_err(BackendError::operation)).transpose()?;
                if let Some(journal) = journal {
                    if let Some(entry) = this.config.registry.entry(&journal.destination).map_err(BackendError::operation)? {
                        let expected = if bokheim { LibraryStorage::Bokheim } else { LibraryStorage::Local };
                        if this.config.registry.storage(&journal.destination).map_err(BackendError::operation)? != expected {
                            return Err(BackendError::message("the copy's storage has changed; restore its storage setting before retrying"));
                        }
                        if !bokheim {
                            this.config.registry.make_device_only(entry.library_id()).map_err(BackendError::operation)?;
                        }
                        return Ok((entry, journal));
                    }
                }
                let name = format!("{} ({})", original.library_name(), if bokheim { "Bokheim copy" } else { "local copy" });
                let entry = this.config.registry.create_with_asset_storage(&name, None, bokheim).map_err(BackendError::operation)?;
                if bokheim {
                    this.bind_to_current_account_on_storage(entry.library_id())?;
                } else {
                    this.config.registry.make_device_only(entry.library_id()).map_err(BackendError::operation)?;
                }
                let journal = StorageCopyJournal { destination: *entry.library_id(), progress: Default::default() };
                this.config.registry.write_value(&journal_key, &serde_json::to_vec(&journal).map_err(BackendError::operation)?).map_err(BackendError::operation)?;
                Ok((entry, journal))
            })
            .await?;
        // Opening failures also leave the durable destination available to retry.
        self.open(entry.library_id()).await?;
        self.send_library_list_changed();
        let destination = self.handle(entry.library_id())?;
        let registry = self.config.registry.clone();
        let checkpoint_key = key.clone();
        destination
            .copy_storage_from(original, journal.progress.clone(), move |progress| {
                journal.progress = progress;
                let bytes = serde_json::to_vec(&journal).map_err(BackendError::operation);
                let key = checkpoint_key.clone();
                let registry = registry.clone();
                async move {
                    let bytes = bytes?;
                    registry.access(move |registry| registry.write_value(&key, &bytes)).await.map_err(BackendError::operation)
                }
            })
            .await?;
        self.config.registry.access(move |registry| registry.remove_value(&key)).await.map_err(BackendError::operation)?;
        self.config.account_wake.send_replace(());
        self.send_library_list_changed();
        Ok(entry)
    }
}
