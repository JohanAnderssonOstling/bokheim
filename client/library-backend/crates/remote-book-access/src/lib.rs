//! Account-backed book reads and playback authorization shared by native and web.
//! Callers retain ownership of library admission and resource lifetimes.
use account_client::{authenticated_request, AuthenticatedRequestError, RefreshPolicy};
use book_access::{AsyncBookSource, FetchFuture, RemoteFile, RemoteFileError};
use library_services::account::LibraryAccount;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::future::Future;
use sync_common::{BookId, LibraryId};

#[derive(Clone, Serialize, Deserialize)]
pub struct DirectPlayback {
    pub url: String,
    pub expires_in_seconds: u32,
    #[serde(rename = "content_hash")]
    pub book_id: BookId,
    pub length: u64,
}

impl std::fmt::Debug for DirectPlayback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DirectPlayback").field("content_hash", &self.book_id).finish_non_exhaustive()
    }
}

#[derive(Clone)]
pub struct RemoteBookAccess {
    client: Client,
    account: LibraryAccount,
    library: LibraryId,
    source: RemoteFile,
}

impl RemoteBookAccess {
    pub fn new(client: Client, account: LibraryAccount, library: LibraryId, source: RemoteFile) -> Self {
        Self { client, account, library, source }
    }

    pub fn fetch(self) -> impl Fn(u64, usize) -> FetchFuture<'static, Result<Vec<u8>, RemoteFileError>> {
        move |offset, length| {
            let access = self.clone();
            Box::pin(async move { access.read(offset, length).await })
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn batch_fetch(self) -> impl Fn(Vec<(u64, usize)>) -> FetchFuture<'static, Result<Vec<u8>, RemoteFileError>> {
        move |ranges| {
            let access = self.clone();
            Box::pin(async move { access.read_ranges(ranges).await })
        }
    }

    pub fn async_source(self, length: u64, prefix: Vec<u8>) -> std::io::Result<AsyncBookSource> {
        let fetch = self.fetch();
        #[cfg(not(target_arch = "wasm32"))]
        let source = AsyncBookSource::remote(length, prefix, fetch);
        #[cfg(target_arch = "wasm32")]
        let source = AsyncBookSource::browser(length, prefix, move |offset, length| {
            let response = fetch(offset, length);
            Box::pin(async move { response.await.map_err(|error| error.to_string()) })
        });
        source
    }

    /// Read a logical range batch using the source's supported transport.
    pub async fn read_ranges(&self, ranges: Vec<(u64, usize)>) -> Result<Vec<u8>, RemoteFileError> {
        let Self { client, account, library, source } = self;

        let ranges = ranges.as_slice();
        authenticated_file_request(account, |session| async move {
            let url = sync_transport::blob_endpoint(session.server_url(), library, &source.hash).map_err(|error| RemoteFileError::Failed(error.to_string()))?;
            book_access::remote_file::fetch_ranges(client, url, source, session.token(), ranges).await
        })
        .await
    }

    pub async fn read(&self, offset: u64, length: usize) -> Result<Vec<u8>, RemoteFileError> {
        let Self { client, account, library, source } = self;
        authenticated_file_request(account, |session| async move {
            let url = sync_transport::blob_endpoint(session.server_url(), library, &source.hash).map_err(|error| RemoteFileError::Failed(error.to_string()))?;
            book_access::remote_file::fetch_range(client, url, source, session.token(), offset, length).await
        })
        .await
    }
}

pub async fn direct_audio_source(client: &Client, account: &LibraryAccount, library: LibraryId, hash: BookId) -> Result<DirectPlayback, RemoteFileError> {
    client_platform_runtime::executor::timeout(
        std::time::Duration::from_secs(30),
        authenticated_file_request(account, |session| async move {
            let mut endpoint = sync_transport::blob_endpoint(session.server_url(), &library, &hash).map_err(|error| RemoteFileError::Failed(error.to_string()))?;
            endpoint.set_path(&format!("{}/playback", endpoint.path()));
            let response = client.post(endpoint).bearer_auth(session.token()).header(reqwest::header::ACCEPT, sync_common::transport::MEDIA_TYPE).send().await.map_err(RemoteFileError::transport)?;
            if !response.status().is_success() {
                return Err(RemoteFileError::status(response.status()));
            }
            let bytes = response.bytes().await.map_err(RemoteFileError::transport)?;
            let grant: sync_common::api::assets::PlaybackGrant = sync_common::transport::decode(&bytes, 16 * 1024).map_err(|_| RemoteFileError::Failed("Invalid playback grant".into()))?;
            let ticket = grant.path.strip_prefix("/api/playback/").ok_or("Invalid playback grant path")?;
            if ticket.len() != 64 || !ticket.bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f')) || grant.length == 0 || grant.expires_in_seconds == 0 {
                return Err("Invalid playback grant".into());
            }
            let url = session.server_url().join(&grant.path).map_err(|_| RemoteFileError::Failed("Invalid playback grant path".into()))?;
            Ok(DirectPlayback { url: url.to_string(), expires_in_seconds: grant.expires_in_seconds, book_id: grant.book_id, length: grant.length })
        }),
    )
    .await
    .map_err(|_| RemoteFileError::Retryable("Playback authorization timed out".into()))?
}

fn file_auth_error(error: library_services::account::AccountSessionError) -> RemoteFileError {
    use library_services::account::AccountSessionError;
    match error {
        AccountSessionError::Changed | AccountSessionError::Authentication(account_client::AuthError::SignInRequired(_)) => RemoteFileError::Unauthorized,
        AccountSessionError::Authentication(account_client::AuthError::Http(error)) => match error.status() {
            Some(status) => RemoteFileError::status(status),
            None => RemoteFileError::transport(error),
        },
        error => RemoteFileError::Failed(error.to_string()),
    }
}

/// Opening and reading books is interactive and always allowed on metered
/// connections. Automatic transfer callers apply their own transfer policy.
async fn authenticated_file_request<T, F: Future<Output = Result<T, RemoteFileError>>>(account: &LibraryAccount, request: impl FnMut(account_client::Session) -> F) -> Result<T, RemoteFileError> {
    account.while_authorized(authenticated_request(account, RefreshPolicy::BeforeRequest, request, |error| matches!(error, RemoteFileError::Unauthorized))).await.map_err(file_auth_error)?.map_err(|error| match error {
        AuthenticatedRequestError::Request(error) => error,
        AuthenticatedRequestError::Session(error) => file_auth_error(error),
        AuthenticatedRequestError::MissingSession => RemoteFileError::Unauthorized,
    })
}

pub async fn describe_remote_file(client: &Client, account: &LibraryAccount, library: LibraryId, hash: BookId) -> Result<(RemoteFile, Vec<u8>), RemoteFileError> {
    authenticated_file_request(account, |session| async move {
        let url = sync_transport::blob_endpoint(session.server_url(), &library, &hash).map_err(|error| RemoteFileError::Failed(error.to_string()))?;
        book_access::remote_file::describe(client, url, session.token(), hash).await
    })
    .await
}

/// Start ZIP-based books with their directory tail on every host.
pub async fn describe_archive(client: &Client, account: &LibraryAccount, library: LibraryId, hash: BookId) -> Result<(RemoteFile, Vec<u8>), RemoteFileError> {
    authenticated_file_request(account, |session| async move {
        let url = sync_transport::blob_endpoint(session.server_url(), &library, &hash).map_err(|error| RemoteFileError::Failed(error.to_string()))?;
        book_access::remote_file::describe_tail(client, url, session.token(), hash).await
    })
    .await
}
