//! Bounded asset work paging, remote negotiation, and transfer job selection.
//!
//! Nothing here touches a library's database connection while an `.await` is
//! outstanding: callers read a snapshot, hand this module plain data to
//! negotiate with the server, and commit the result themselves afterward.
use crate::deadline::transfer_deadline;
use crate::policy::SMALL_REQUEST_TIMEOUT;
use crate::{TransferError, TransferJob, TransferOrigin};
use library_database::BookUploadIntent;
use library_files::AssetStore;
use library_replica::BlobKind;
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
    fn credentials(&self) -> Result<sync_transport::SyncCredentials, TransferError> {
        (self.credentials)()
    }

    /// Pure network negotiation: no database reference is ever held across
    /// the request. The caller reads `intents` from its own database
    /// snapshot beforehand and commits `present_remotely`/`rejected`
    /// afterward.
    pub async fn negotiate_book_uploads(&self, intents: &[BookUploadIntent]) -> Result<BookUploadNegotiation, TransferError> {
        let credentials = self.credentials()?;
        let manifest = intents.iter().map(|job| sync_common::api::assets::BlobManifestEntry { content_hash: job.content_hash, checksum: job.checksum, size_bytes: job.size_bytes }).collect();
        let response = transfer_deadline(SMALL_REQUEST_TIMEOUT, crate::negotiate_blobs(&self.http_client, &credentials, &self.library_id, manifest)).await?.map_err(|error| match error {
            sync_transport::SyncRequestError::AuthenticationRequired => TransferError::AuthenticationRequired,
            error => TransferError::retryable(error),
        })?;
        let present_remotely = response.owned.iter().chain(&response.claimed).copied().collect::<Vec<_>>();
        let rejected = response
            .rejected
            .iter()
            .filter_map(|rejection| match TransferError::from_upload_admission(rejection.reason) {
                TransferError::Rejected(reason) => Some((rejection.content_hash, reason)),
                _ => None,
            })
            .collect::<Vec<_>>();
        let upload_required = response.upload.into_iter().collect::<HashSet<_>>();
        Ok(BookUploadNegotiation { present_remotely, rejected, upload_required })
    }

    /// Pure network negotiation, same shape as [`Self::negotiate_book_uploads`].
    pub async fn negotiate_thumbnail_availability(&self, hashes: &[ContentHash]) -> Result<ThumbnailAvailabilityNegotiation, TransferError> {
        let credentials = self.credentials()?;
        let response = transfer_deadline(SMALL_REQUEST_TIMEOUT, sync_transport::thumbnail_presence(&self.http_client, &credentials, &self.library_id, hashes.to_vec())).await?.map_err(|error| match error {
            sync_transport::SyncRequestError::AuthenticationRequired => TransferError::AuthenticationRequired,
            error => TransferError::retryable(error),
        })?;
        let requested: HashSet<_> = hashes.iter().copied().collect();
        let mut answered = HashSet::new();
        for hash in &response.present {
            if !requested.contains(hash) || !answered.insert(*hash) {
                return Err(TransferError::retryable("thumbnail availability response is invalid"));
            }
        }
        let available = response.present;
        let missing = requested.difference(&answered).copied().collect();
        Ok(ThumbnailAvailabilityNegotiation { available, missing })
    }

    /// Confirms the server identity has not changed since a snapshot was
    /// read, before its negotiated result is committed.
    pub fn check_transfer_account(&self, credentials: &sync_transport::SyncCredentials) -> Result<(), TransferError> {
        if self.credentials()?.server_url() != credentials.server_url() {
            return Err(TransferError::retryable("credentials changed during asset transfer"));
        }
        Ok(())
    }
}

pub fn prepare_requested_download(assets: &AssetStore, database: &library_database::Database, hash: &ContentHash) -> Result<Option<TransferJob>, TransferError> {
    if assets.exists(BlobKind::Book, hash)? {
        database.complete_local_book_request(*hash)?;
        return Ok(None);
    }
    Ok(database.requested_book_download(*hash)?.map(|placement| TransferJob::download_blob(*hash, placement, TransferOrigin::UserInitiated)))
}
