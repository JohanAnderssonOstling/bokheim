use crate::api::SyncCredentials;
use reqwest::StatusCode;
use std::time::Duration;
use sync_common::api::assets::BlobManifestValidationError;
use sync_common::transport::WireError;
use sync_common::{LibraryId, ReplicaId, SyncCursor, SyncExchangeRequest, SyncExchangeResponse};
use url::ParseError;

/// Failures from synchronization endpoints, including the statuses that drive recovery.
#[derive(Debug, thiserror::Error)]
pub enum SyncRequestError {
    #[error("synchronization authentication is required")]
    AuthenticationRequired,
    #[error("the synchronization cursor was rejected")]
    CursorRejected,
    #[error("{0}")]
    Http(#[source] reqwest::Error),
    #[error("{0}")]
    Wire(#[from] WireError),
    #[error("{0}")]
    Url(#[from] ParseError),
    #[error("{0}")]
    InvalidManifest(#[source] BlobManifestValidationError),
    #[error("sync server returned {status}; retry after {} seconds", .delay.as_secs())]
    RetryAfter { status: StatusCode, delay: Duration },
}

impl From<reqwest::Error> for SyncRequestError {
    fn from(error: reqwest::Error) -> Self {
        match error.status() {
            Some(StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) => Self::AuthenticationRequired,
            Some(StatusCode::GONE) => Self::CursorRejected,
            _ => Self::Http(error),
        }
    }
}

pub async fn exchange_changes(
    client: &reqwest::Client, credentials: &SyncCredentials, library_id: LibraryId, replica_id: ReplicaId, mutations: Vec<sync_common::OpaquePublication>, cursor: SyncCursor,
) -> Result<SyncExchangeResponse, SyncRequestError> {
    let request = SyncExchangeRequest { library_id, replica_id, mutations, cursor };
    let mut trace = crate::PerformanceTrace::new("exchange_http", "encode_request_response_decode");
    log::debug!(target: "sync_performance", "trace_id={} library_id={} push_count={} cursor={:?}", trace.id(), request.library_id, request.mutations.len(), request.cursor);
    crate::api::send_with_trace_byte_fields(client.post(credentials.server_url().join("api/sync/exchange")?).bearer_auth(credentials.access_token()), &request, &mut trace).await
}

pub(crate) fn retry_after(value: &str) -> Option<Duration> {
    let now = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).ok()?;
    retry_after_at(value, now)
}

fn retry_after_at(value: &str, now: Duration) -> Option<Duration> {
    let value = value.trim();
    let delay = match value.parse::<u64>() {
        Ok(seconds) => Duration::from_secs(seconds),
        Err(_) => httpdate::parse_http_date(value).ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.saturating_sub(now),
    };
    Some(delay.min(Duration::from_secs(24 * 60 * 60)))
}
