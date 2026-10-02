//! Bounded asset work paging, remote negotiation, and transfer job selection.
//!
//! Nothing here touches a library's database connection while an `.await` is
//! outstanding: callers read a snapshot, hand this module plain data to
//! negotiate with the server, and commit the result themselves afterward.
use crate::deadline::transfer_deadline;
use crate::policy::SMALL_REQUEST_TIMEOUT;
use crate::{TransferError, TransferJob};
use library_files::AssetStore;
use library_model::BookUploadIntent;
use library_model::TransferOrigin;
use std::collections::HashSet;
use sync_common::ContentHash;

/// The server's answer to a book-upload manifest, before it is committed.
pub struct BookUploadNegotiation {
    pub present_remotely: Vec<ContentHash>,
    pub rejected: Vec<(ContentHash, client_runtime::BackendError)>,
    pub upload_required: HashSet<ContentHash>,
}

/// The server's answer to a thumbnail-availability check, before it is committed.
pub struct ThumbnailAvailabilityNegotiation {
    pub available: Vec<ContentHash>,
    pub missing: Vec<ContentHash>,
}

/// Negotiates remote availability using the host account and library connection.
pub struct TransferPlanner<C> {
    pub http_client: reqwest::Client,
    pub library_id: sync_common::LibraryId,
    pub credentials: C,
}
impl<C> TransferPlanner<C>
where
    C: Fn() -> Result<sync_transport::SyncCredentials, TransferError> + Sync,
{
    

    

    

    /// Confirms the server identity has not changed since a snapshot was
    /// read, before its negotiated result is committed.
    pub fn check_transfer_account(&self, credentials: &sync_transport::SyncCredentials) -> Result<(), TransferError> {
        if (self.credentials)()?.server_url() != credentials.server_url() {
            return Err(TransferError::retryable("credentials changed during asset transfer"));
        }
        Ok(())
    }
}

pub fn prepare_requested_download(assets: &AssetStore, database: &library_database::Database, hash: &ContentHash) -> Result<Option<TransferJob>, TransferError> {
    if assets.book_exists(hash, &database.book_paths(hash)?)? {
        ({
            use library_database::assets::*;
            use library_database::shared_sql::*;
            use library_database::*;
            use library_model::{BookPlacement, BookUploadIntent, RelativeBookPath};
            use sync_common::ContentHash;
            let database = database;
            let hash = *hash;
            (|| -> Result<(), library_database::DatabaseError> {
                let transaction = database.connection.unchecked_transaction()?;
                library_database::Database::complete_local_book_request_on(&transaction, &hash)?;
                transaction.commit().map_err(Into::into)
            })()
        })?;
        return Ok(None);
    }
    Ok(({
        use library_database::assets::*;
        use library_database::shared_sql::*;
        use library_database::*;
        use library_model::{BookPlacement, BookUploadIntent};
        use sync_common::ContentHash;
        let database = database;
        let hash = *hash;
        (|| -> Result<Option<BookPlacement>, library_database::DatabaseError> {
            // The book's first remembered or live path on this device.
            let path = database.book_paths(&hash)?.into_iter().next();
            match path {
                Some(path) => Ok(Some(BookPlacement::new(hash, path))),
                None => {
                    database.connection.shared_transfer_complete_download(hash.as_str())?;
                    Ok(None)
                }
            }
        })()
    })?
    .map(|placement| TransferJob::download_blob(*hash, placement, TransferOrigin::UserInitiated)))
}
