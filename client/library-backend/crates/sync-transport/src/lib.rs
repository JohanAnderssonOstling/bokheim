mod performance;
pub use performance::PerformanceTrace;
mod api;
mod assets;
mod body;
mod state;

pub use api::SyncCredentials;
pub use assets::{
    AssetResponseError, AssetStatus, blob_endpoint, classify_asset_status, download_thumbnail_batch, negotiate_blobs, thumbnail_batch_endpoint, thumbnail_presence, upload_book_stream_with_checksum, upload_thumbnail_batch,
    validate_streaming_asset_response,
};
pub use state::{SyncRequestError, exchange_changes};
pub use sync_common::{PullBatchError, PushResponseError, ValidatedPushResponse, validate_pull_batch, validate_push_response};
pub use url::Url;

#[cfg(not(target_arch = "wasm32"))]
pub type CredentialsFuture<'a> = futures_util::future::BoxFuture<'a, Option<SyncCredentials>>;
#[cfg(target_arch = "wasm32")]
pub type CredentialsFuture<'a> = futures_util::future::LocalBoxFuture<'a, Option<SyncCredentials>>;

/// Supplies an authorized access token without making transport own account state.
pub trait CredentialsSource: Send + Sync + std::fmt::Debug {
    fn credentials(&self) -> CredentialsFuture<'_>;
}

impl CredentialsSource for std::sync::RwLock<Option<SyncCredentials>> {
    fn credentials(&self) -> CredentialsFuture<'_> {
        Box::pin(async move { self.read().ok().and_then(|value| value.clone()) })
    }
}
