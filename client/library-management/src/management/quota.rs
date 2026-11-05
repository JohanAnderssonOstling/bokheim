//! Serialize quota admission with local commits across every open library.
use std::collections::{HashMap, HashSet};

use super::LibraryService;
use crate::account::account_error;
use crate::BackendError;
use client_protocol::Failure;
use content_address::ContentHash;
use library_model::CloudQuotaBook;
use library_services::quota::CloudQuotaPermit;
use std::sync::Arc;
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
    pub async fn admit_cloud_capacity(self: &Arc<Self>, books: Vec<BlobManifestEntry>, additional_bytes: u64) -> Result<CloudQuotaPermit, BackendError> {
        let permit = CloudQuotaPermit::new(self.config.cloud_quota_lock.clone().lock_owned().await);
        let session = self.require_account().await?;
        let identity = (session.server_url().clone(), session.user_id().to_owned());
        let (mut pending, owned) = cloud_book_budget(self.cloud_book_inventory().await?);
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
            .config
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

    async fn cloud_book_inventory(self: &Arc<Self>) -> Result<Vec<CloudQuotaBook>, BackendError> {
        let mut books = Vec::new();
        for entry in self.list().await? {
            if self.library_asset_storage_enabled(entry.library_id()).await? {
                let library = self.handle(entry.library_id())?;
                books.extend(library.cloud_quota_books().await?);
            }
        }
        Ok(books)
    }

    pub async fn pending_cloud_books(self: &Arc<Self>) -> Result<HashMap<BookId, u64>, BackendError> {
        let (mut pending, owned) = cloud_book_budget(self.cloud_book_inventory().await?);
        pending.retain(|hash, _| !owned.contains(hash));
        Ok(pending.into_values().collect())
    }
}

fn cloud_book_budget(books: Vec<CloudQuotaBook>) -> (HashMap<ContentBudgetKey, (BookId, u64)>, HashSet<ContentBudgetKey>) {
    let mut pending = HashMap::new();
    let mut owned = HashSet::new();
    for book in books {
        let key = ContentBudgetKey::new(book.book_id, book.byte_checksum);
        if book.server_has_book {
            owned.insert(key);
        }
        pending.entry(key).and_modify(|(_, size): &mut (BookId, u64)| *size = (*size).max(book.size_bytes)).or_insert((book.book_id, book.size_bytes));
    }
    (pending, owned)
}
