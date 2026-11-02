//! Copy into a separate library; provider files and the source stay untouched.
use super::{core::AppBackend, LibraryEntry};
use crate::{BackendError, Failure};
use library_backend::account::account_error;
use library_backend::library::handle::{LibraryActorCommand, LibraryControl};
use library_backend::management::StorageCopyJournal;
use library_backend::{LibraryCommand, PendingCommand, ReadyCommand};
use library_model::CloudQuotaBook;
use sync_common::{api::assets::BlobManifestEntry, storage::LibraryStorage, LibraryId};

impl AppBackend {
    pub(crate) async fn copy_library_storage(&self, source: LibraryId, bokheim: bool) -> Result<LibraryEntry, BackendError> {
        // Requests from different windows must resume the same pending copy.
        let _copy = self.state.storage_copy_lock.lock().await;
        self.copy_library_storage_inner(source, bokheim, false).await
    }

    async fn copy_library_storage_inner(&self, source: LibraryId, bokheim: bool, keep_journal: bool) -> Result<LibraryEntry, BackendError> {
        self.library_entry(&source)?;
        if bokheim && !self.state.account_service.account_status()?.signed_in() {
            return Err(BackendError::classified(Failure::SignInRequired, "sign in before copying a library to Bokheim storage"));
        }
        let key = format!("storage-copy:v1:{source}:{}", u8::from(bokheim));
        if bokheim && self.state.registry.read_value(&key).map_err(BackendError::operation)?.is_none() {
            let library = self.state.library_directory.get(&source)?;
            let books: Vec<CloudQuotaBook> = library.dispatch(LibraryCommand::Ready(ReadyCommand::CloudQuotaBooks)).await?.take()?;
            let _quota = self.state.library_service.admit_cloud_books(books.into_iter().map(|book| BlobManifestEntry { byte_checksum: book.byte_checksum, book_id: book.book_id, size_bytes: book.size_bytes }).collect()).await?;
        }
        let journal_key = key.clone();
        let (entry, mut journal) = self.with_storage(move |this| {
            let original = this.library_entry(&source)?;
            let journal: Option<StorageCopyJournal> = this.state.registry.read_value(&journal_key).map_err(BackendError::operation)?.map(|bytes| serde_json::from_slice(&bytes).map_err(BackendError::operation)).transpose()?;
            if let Some(journal) = journal {
                if let Some(entry) = this.state.registry.entry(&journal.destination).map_err(BackendError::operation)? {
                    let expected = if bokheim { LibraryStorage::Bokheim } else { LibraryStorage::Local };
                    if this.state.registry.storage(&journal.destination).map_err(BackendError::operation)? != expected {
                        return Err(BackendError::message("the copy's storage has changed; restore its storage setting before retrying"));
                    }
                    if !bokheim {
                        this.state.registry.make_device_only(entry.library_id()).map_err(BackendError::operation)?;
                    }
                    return Ok((entry, journal));
                }
            }
            let name = format!("{} ({})", original.library_name(), if bokheim { "Bokheim copy" } else { "local copy" });
            let entry = this.state.registry.create_with_asset_storage(&name, None, bokheim).map_err(BackendError::operation)?;
            if bokheim {
                this.state.library_service.bind_to_current_account(entry.library_id())?;
            } else {
                this.state.registry.make_device_only(entry.library_id()).map_err(BackendError::operation)?;
            }
            let journal = StorageCopyJournal { destination: *entry.library_id(), progress: Default::default() };
            this.state.registry.write_value(&journal_key, &serde_json::to_vec(&journal).map_err(BackendError::operation)?).map_err(BackendError::operation)?;
            Ok((entry, journal))
        })?;
        // Opening failures also leave the durable destination available to retry.
        self.open_registered_library(entry.library_id())?;
        self.send_library_list_changed();
        let original = self.state.library_directory.get(&source)?;
        let destination = self.state.library_directory.get(entry.library_id())?;
        let registry = self.state.registry.clone();
        let checkpoint_key = key.clone();
        destination
            .copy_storage_from(&original, journal.progress.clone(), move |progress| {
                journal.progress = progress.clone();
                registry.write_value(&checkpoint_key, &serde_json::to_vec(&journal).map_err(BackendError::operation)?).map_err(BackendError::operation)
            })
            .await?;
        if !keep_journal {
            self.state.registry.remove_value(&key).map_err(BackendError::operation)?;
        }
        self.state.account_wake.send_replace(());
        self.send_library_list_changed();
        Ok(entry)
    }

    pub(super) async fn make_library_local_only(&self, source: LibraryId) -> Result<LibraryEntry, BackendError> {
        let _copy = self.state.storage_copy_lock.lock().await;
        let name = self.library_entry(&source)?.library_name().to_owned();
        let account = self.state.account.require().map_err(account_error)?;
        let identity = format!("{}|{}", account.server_url(), account.user_id());
        self.state.library_directory.get(&source)?.dispatch(LibraryCommand::Pending(PendingCommand::RefreshStorageCopy)).await?.take::<()>()?;
        let entry = self.copy_library_storage_inner(source, false, true).await?;
        // Only a complete verified copy permits cloud removal. Re-check the
        // captured account on every attempt, including token-refresh retries.
        self.state
            .account
            .request(|session| {
                let matches = format!("{}|{}", session.server_url(), session.user_id()) == identity;
                Box::pin(async move {
                    if !matches {
                        return Err(account_client::AuthError::SignInRequired("sign in to the account that owns this library"));
                    }
                    match account_client::delete_library(&session, &source).await.map(|_| ()) {
                        Err(error) if error.is_not_found() => Ok(()),
                        result => result,
                    }
                })
            })
            .await
            .map_err(account_error)?;
        let destination_id = *entry.library_id();
        let entry = self.with_storage(move |this| {
            if this.state.registry.entry(&source).map_err(BackendError::operation)?.is_some() {
                this.state.registry.begin_local_removal(&source).map_err(BackendError::operation)?;
                this.state.library_directory.retire(&source)?;
            }
            this.state.registry.complete_server_deletion(&source).map_err(BackendError::operation)?;
            this.state.registry.remove_value(&format!("storage-copy:v1:{source}:0")).map_err(BackendError::operation)?;
            let entry = this.state.registry.attach_remote(&destination_id, &name).map_err(BackendError::operation)?;
            if let Some(handle) = this.state.library_directory.try_get(&destination_id)? {
                let _ = handle.commands.try_send(LibraryActorCommand::Control(LibraryControl::Rename { name, refresh_remote: false }));
            }
            Ok(entry)
        })?;
        if let Err(error) = self.state.library_service.finish_local_purge(source).await {
            log::warn!("cloud library was saved locally; old cache cleanup will retry: {error}");
        }
        self.state.account_wake.send_replace(());
        self.send_library_list_changed();
        Ok(entry)
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "../../tests/app/storage_copy/tests.rs"]
mod tests;
