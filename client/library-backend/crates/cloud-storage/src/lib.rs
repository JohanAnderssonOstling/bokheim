//! Provider APIs for cloud folder discovery, seekable reads, and committed
//! uploads. No operation in this crate knows a Bokheim storage endpoint.

use futures_util::StreamExt;
use reqwest::{Response, StatusCode};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::{future::Future, pin::Pin};
use sync_common::storage::{ProviderFile, ProviderId};
use sync_common::{DirId, LibraryId};

pub mod connections;
pub mod google_drive;
#[cfg(not(target_arch = "wasm32"))]
pub mod native_login;
pub mod oauth;
pub mod scanner;

#[cfg(not(target_arch = "wasm32"))]
pub type StorageFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, CloudError>> + Send + 'a>>;
#[cfg(target_arch = "wasm32")]
pub type StorageFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, CloudError>> + 'a>>;

#[cfg(not(target_arch = "wasm32"))]
pub trait ProviderRuntime: Send + Sync {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Send + Sync> ProviderRuntime for T {}
#[cfg(target_arch = "wasm32")]
pub trait ProviderRuntime {}
#[cfg(target_arch = "wasm32")]
impl<T> ProviderRuntime for T {}

/// Provider-specific authentication stays with the connection owner. Expired
/// credentials produce a typed result so that owner can refresh and retry.
pub trait CloudStorage: ProviderRuntime {
    fn list_folder<'a>(&'a self, token: &'a str, folder: &'a ProviderId, page: Option<&'a str>, folders_only: bool) -> StorageFuture<'a, FolderPage>;
    fn describe<'a>(&'a self, token: &'a str, id: &'a ProviderId) -> StorageFuture<'a, CloudEntry>;
    fn read_range<'a>(&'a self, token: &'a str, file: &'a ProviderFile, offset: u64, length: usize) -> StorageFuture<'a, Vec<u8>>;
    fn start_checkpoint<'a>(&'a self, token: &'a str) -> StorageFuture<'a, String>;
    fn changes<'a>(&'a self, token: &'a str, checkpoint: &'a str) -> StorageFuture<'a, ChangesPage>;
    fn create_folder<'a>(&'a self, token: &'a str, parent: &'a ProviderId, name: &'a str) -> StorageFuture<'a, CloudFolder>;
}

pub const MAX_RANGE_BYTES: usize = 8 * 1024 * 1024;
const MAX_METADATA_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct CloudFolder {
    pub id: ProviderId,
    pub name: String,
    pub parent_ids: Vec<ProviderId>,
    #[serde(default)]
    pub directory_hint: Option<CloudDirectoryIdentity>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct CloudDirectoryIdentity {
    pub library_id: LibraryId,
    pub directory_id: DirId,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct CloudBookFile {
    pub file: ProviderFile,
    pub name: String,
    pub parent_ids: Vec<ProviderId>,
    pub mime_type: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum CloudEntry {
    Folder(CloudFolder),
    File(CloudBookFile),
}

/// An unfinished listing is never evidence of deletion. The scanner commits
/// a complete inventory only after consuming every continuation page.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct FolderPage {
    pub entries: Vec<CloudEntry>,
    pub next_page: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum CloudChange {
    Updated(CloudEntry),
    Removed(ProviderId),
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ChangesPage {
    pub changes: Vec<CloudChange>,
    pub next_page: Option<String>,
    /// Checkpoint for a subsequent pass, available only on the final page.
    pub checkpoint: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CloudError {
    AuthenticationRequired,
    PermissionDenied,
    NotFound,
    RevisionChanged,
    CursorExpired,
    Retryable(String),
    InvalidResponse(String),
    InvalidRequest(String),
}

impl fmt::Display for CloudError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AuthenticationRequired => formatter.write_str("Reconnect your cloud storage account"),
            Self::PermissionDenied => formatter.write_str("Cloud storage denied access to this file or folder"),
            Self::NotFound => formatter.write_str("The cloud file or folder is unavailable"),
            Self::RevisionChanged => formatter.write_str("The cloud file changed; refresh the library and retry"),
            Self::CursorExpired => formatter.write_str("The cloud storage scan checkpoint expired"),
            Self::Retryable(message) | Self::InvalidResponse(message) | Self::InvalidRequest(message) => formatter.write_str(message),
        }
    }
}
impl std::error::Error for CloudError {}

impl CloudError {
    pub(crate) fn transport(error: reqwest::Error) -> Self {
        Self::Retryable(error.without_url().to_string())
    }
    pub(crate) fn status(status: StatusCode) -> Self {
        match status {
            StatusCode::UNAUTHORIZED => Self::AuthenticationRequired,
            StatusCode::FORBIDDEN => Self::PermissionDenied,
            StatusCode::NOT_FOUND => Self::NotFound,
            StatusCode::GONE => Self::CursorExpired,
            StatusCode::PRECONDITION_FAILED => Self::RevisionChanged,
            status if status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS || status == StatusCode::REQUEST_TIMEOUT => Self::Retryable(format!("Cloud storage returned HTTP {status}")),
            status => Self::InvalidResponse(format!("Cloud storage returned HTTP {status}")),
        }
    }
}

pub(crate) async fn bounded_bytes(response: Response, limit: usize) -> Result<Vec<u8>, CloudError> {
    if response.content_length().is_some_and(|length| length > limit as u64) {
        return Err(CloudError::InvalidResponse("Cloud storage response is too large".into()));
    }
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(CloudError::transport)?;
        if chunk.len() > limit - bytes.len() {
            return Err(CloudError::InvalidResponse("Cloud storage response is too large".into()));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

pub(crate) async fn metadata<T: serde::de::DeserializeOwned>(response: Response) -> Result<T, CloudError> {
    if !response.status().is_success() {
        return Err(CloudError::status(response.status()));
    }
    let bytes = bounded_bytes(response, MAX_METADATA_BYTES).await?;
    serde_json::from_slice(&bytes).map_err(|_| CloudError::InvalidResponse("Invalid cloud storage metadata".into()))
}
