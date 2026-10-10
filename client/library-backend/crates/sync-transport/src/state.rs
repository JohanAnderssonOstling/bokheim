use crate::api::SyncCredentials;
use reqwest::StatusCode;
use std::{error::Error, fmt};
use sync_common::api::assets::BlobManifestValidationError;
use sync_common::transport::WireError;
use sync_common::{LibraryId, ReplicaId, SyncCursor, SyncExchangeRequest, SyncExchangeResponse};
use url::ParseError;

/// Failures from synchronization endpoints, including the statuses that drive recovery.
#[derive(Debug)]
pub enum SyncRequestError {
    AuthenticationRequired,
    CursorRejected,
    Http(reqwest::Error),
    Wire(WireError),
    Url(ParseError),
    InvalidManifest(BlobManifestValidationError),
}

impl fmt::Display for SyncRequestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AuthenticationRequired => formatter.write_str("synchronization authentication is required"),
            Self::CursorRejected => formatter.write_str("the synchronization cursor was rejected"),
            Self::Http(error) => error.fmt(formatter),
            Self::Wire(error) => error.fmt(formatter),
            Self::Url(error) => error.fmt(formatter),
            Self::InvalidManifest(error) => error.fmt(formatter),
        }
    }
}

impl Error for SyncRequestError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Http(error) => Some(error),
            Self::Wire(error) => Some(error),
            Self::Url(error) => Some(error),
            Self::InvalidManifest(error) => Some(error),
            Self::AuthenticationRequired | Self::CursorRejected => None,
        }
    }
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

impl From<WireError> for SyncRequestError {
    fn from(error: WireError) -> Self {
        Self::Wire(error)
    }
}

impl From<ParseError> for SyncRequestError {
    fn from(error: ParseError) -> Self {
        Self::Url(error)
    }
}

pub async fn exchange_changes(
    client: &reqwest::Client, credentials: &SyncCredentials, library_id: LibraryId, replica_id: ReplicaId, mutations: Vec<sync_common::OpaquePublication>, cursor: SyncCursor,
) -> Result<SyncExchangeResponse, SyncRequestError> {
    let request = SyncExchangeRequest { library_id, replica_id, mutations, cursor };
    let mut trace = crate::PerformanceTrace::new("exchange_http", "encode_request_response_decode");
    log::debug!(target: "sync_performance", "trace_id={} library_id={} push_count={} cursor={:?}", trace.id(), request.library_id, request.mutations.len(), request.cursor);
    crate::api::send_with_trace(client.post(credentials.server_url().join("api/sync/exchange")?).bearer_auth(credentials.access_token()), &request, &mut trace).await
}
