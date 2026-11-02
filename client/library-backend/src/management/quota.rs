//! Serialize quota admission with local commits across every open library.
use std::collections::{HashMap, HashSet};

use super::LibraryService;
use crate::account::account_error;
use crate::BackendError;
use crate::{LibraryCommand, ReadyCommand};
use client_runtime::Failure;
use content_address::ContentHash;
use library_model::CloudQuotaBook;
use library_runtime::quota::CloudQuotaPermit;
use sync_common::api::assets::BlobManifestEntry;
use sync_common::BookId;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum ContentBudgetKey {
    Bytes(ContentHash),
    Book(BookId),
}

impl ContentBudgetKey {
    fn new(book: BookId, checksum: Option<ContentHash>) -> Self {
        checksum.or_else(|| book.content_hash()).map(Self::Bytes).unwrap_or(Self::Book(book))
    }
}

impl LibraryService {
    pub async fn admit_cloud_books(&self, books: Vec<BlobManifestEntry>) -> Result<CloudQuotaPermit, BackendError> {
        self.admit_cloud_capacity(books, 0).await
    }

    pub async fn admit_cloud_capacity(&self, books: Vec<BlobManifestEntry>, additional_bytes: u64) -> Result<CloudQuotaPermit, BackendError> {
        let permit = CloudQuotaPermit::new(self.state.cloud_quota_lock.clone().lock_owned().await);
        let session = self.require_account()?;
        let identity = (session.server_url().clone(), session.user_id().to_owned());
        let (mut pending, owned) = self.cloud_book_budget().await?;
        for book in books {
            pending.entry(ContentBudgetKey::new(book.book_id, book.byte_checksum)).and_modify(|(_, size)| *size = (*size).max(book.size_bytes)).or_insert((book.book_id, book.size_bytes));
        }
        // Known server books cost no further capacity, including another
        // placement of the same book in a different library.
        let required = pending
            .into_iter()
            .filter(|(hash, _)| !owned.contains(hash))
            .try_fold(additional_bytes, |total, (_, (_, bytes))| total.checked_add(bytes))
            .ok_or_else(|| BackendError::classified(Failure::CloudStorageFull, "the books exceed the cloud storage quota"))?;
        let snapshot = self
            .state
            .account
            .request(|session| {
                let matches = session.server_url() == &identity.0 && session.user_id() == identity.1;
                Box::pin(async move {
                    if !matches {
                        return Err(account_client::AuthError::SignInRequired("the account changed while checking cloud storage"));
                    }
                    account_client::account_snapshot(&session).await
                })
            })
            .await
            .map_err(account_error)?;
        if required > snapshot.storage.available_bytes() {
            return Err(BackendError::classified(Failure::CloudStorageFull, "not enough cloud storage for these books; free space or use a local-only library"));
        }
        Ok(permit)
    }

    async fn cloud_book_inventory(&self) -> Result<Vec<CloudQuotaBook>, BackendError> {
        let mut books = Vec::new();
        for entry in self.list()? {
            if self.library_asset_storage_enabled(entry.library_id())? {
                let library = self.state.library_directory.get(entry.library_id())?;
                books.extend(library.dispatch(LibraryCommand::Ready(ReadyCommand::CloudQuotaBooks)).await?.take::<Vec<CloudQuotaBook>>()?);
            }
        }
        Ok(books)
    }

    async fn cloud_book_budget(&self) -> Result<(HashMap<ContentBudgetKey, (BookId, u64)>, HashSet<ContentBudgetKey>), BackendError> {
        let mut pending = HashMap::new();
        let mut owned = HashSet::new();
        for book in self.cloud_book_inventory().await? {
            let key = ContentBudgetKey::new(book.book_id, book.byte_checksum);
            if book.server_has_book {
                owned.insert(key);
            }
            pending.entry(key).and_modify(|(_, size): &mut (BookId, u64)| *size = (*size).max(book.size_bytes)).or_insert((book.book_id, book.size_bytes));
        }
        Ok((pending, owned))
    }

    pub async fn pending_cloud_books(&self) -> Result<HashMap<BookId, u64>, BackendError> {
        let (mut pending, owned) = self.cloud_book_budget().await?;
        pending.retain(|hash, _| !owned.contains(hash));
        Ok(pending.into_values().collect())
    }
}
