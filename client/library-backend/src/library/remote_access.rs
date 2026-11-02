//! Account-backed remote reading owned by one library session.

pub(crate) use super::{playback::DirectPlayback, LibrarySession, RemoteFile};
pub(crate) use crate::BackendError;
pub(crate) use crate::BookId;
use crate::LibraryId;
use account_client::{authenticated_request, AuthenticatedRequestError, RefreshPolicy};
use book_access::remote_file::range_byte_count;
use book_access::{AsyncBookSource, FetchFuture, RemoteFileError};
use library_runtime::account::LibraryAccount;
use reqwest::Client;
use std::future::Future;

#[derive(Clone)]
pub(crate) struct RemoteBookAccess {
    client: Client,
    account: LibraryAccount,
    library: LibraryId,
    source: RemoteFile,
}

impl RemoteBookAccess {
    pub(crate) fn fetch(self) -> impl Fn(u64, usize) -> FetchFuture<'static, Result<Vec<u8>, RemoteFileError>> {
        move |offset, length| {
            let access = self.clone();
            Box::pin(async move { access.read(offset, length).await })
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn batch_fetch(self) -> impl Fn(Vec<(u64, usize)>) -> FetchFuture<'static, Result<Vec<u8>, RemoteFileError>> {
        move |ranges| {
            let access = self.clone();
            Box::pin(async move { access.read_ranges(ranges).await })
        }
    }

    pub(crate) fn async_source(self, length: u64, prefix: Vec<u8>) -> Result<AsyncBookSource, BackendError> {
        let fetch = self.fetch();
        #[cfg(not(target_arch = "wasm32"))]
        let source = AsyncBookSource::remote(length, prefix, fetch);
        #[cfg(target_arch = "wasm32")]
        let source = AsyncBookSource::browser(length, prefix, move |offset, length| {
            let response = fetch(offset, length);
            Box::pin(async move { response.await.map_err(|error| error.to_string()) })
        });
        source.map_err(BackendError::operation)
    }

    /// Read a logical range batch using multipart HTTP.
    pub(crate) async fn read_ranges(&self, ranges: Vec<(u64, usize)>) -> Result<Vec<u8>, RemoteFileError> {
        let Self { client, account, library, source } = self;

        let ranges = ranges.as_slice();
        authenticated_file_request(account, |session| async move {
            let url = sync_transport::blob_endpoint(session.server_url(), library, &source.hash).map_err(|error| RemoteFileError::Failed(error.to_string()))?;
            book_access::remote_file::fetch_ranges(client, url, source, session.token(), ranges).await
        })
        .await
    }
    pub(crate) async fn read_bundle(&self, ranges: Vec<(u64, usize)>) -> Result<Vec<u8>, RemoteFileError> {
        let Self { client, account, library, source } = self;

        let ranges = ranges.as_slice();
        authenticated_file_request(account, |session| async move {
            let url = sync_transport::blob_endpoint(session.server_url(), library, &source.hash).map_err(|error| RemoteFileError::Failed(error.to_string()))?;
            book_access::remote_file::fetch_bundle(client, url, source, session.token(), ranges).await
        })
        .await
    }

    pub(crate) async fn read(&self, offset: u64, length: usize) -> Result<Vec<u8>, RemoteFileError> {
        let Self { client, account, library, source } = self;
        read_remote_file(client, account, *library, source, offset, length).await
    }
}

pub(crate) fn range_backend_error(error: RemoteFileError) -> BackendError {
    match error {
        RemoteFileError::Unauthorized => BackendError::classified(client_runtime::Failure::SignInRequired, "reconnect the storage account before retrying"),
        RemoteFileError::Retryable(message) => BackendError::classified(client_runtime::Failure::ServiceUnavailable, message),
        error => BackendError::operation(error),
    }
}

impl LibrarySession {
    pub(crate) fn account(&self) -> Result<LibraryAccount, BackendError> {
        self.sync.account().ok_or_else(|| BackendError::message("library account access is unavailable"))
    }

    pub(crate) fn downloaded_books_only(&self) -> Result<bool, BackendError> {
        let storage = self.db.storage_policy()?;

        Ok(!storage.uses_bokheim_assets() || self.account()?.current().map_err(BackendError::operation)?.is_none())
    }

    pub(crate) fn remote_book_access(&self, source: &RemoteFile) -> Result<RemoteBookAccess, BackendError> {
        Ok(RemoteBookAccess { client: self.sync.http_client().clone(), account: self.account()?, library: self.id, source: source.clone() })
    }

    pub(crate) async fn direct_audio_source(&self, hash: BookId) -> Result<DirectPlayback, String> {
        let account = self.account().map_err(|error| error.to_string())?;
        let library = self.id;
        crate::executor::timeout(
            std::time::Duration::from_secs(30),
            authenticated_file_request(&account, |session| async move {
                let mut endpoint = sync_transport::blob_endpoint(session.server_url(), &library, &hash).map_err(|error| RemoteFileError::Failed(error.to_string()))?;
                endpoint.set_path(&format!("{}/playback", endpoint.path()));
                let response = self.sync.http_client().post(endpoint).bearer_auth(session.token()).header(reqwest::header::ACCEPT, sync_common::transport::MEDIA_TYPE).send().await.map_err(RemoteFileError::transport)?;
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
        .map_err(|_| "Playback authorization timed out".to_owned())?
        .map_err(|error| error.to_string())
    }

    fn require_library_book(&self, hash: &BookId) -> Result<(), RemoteFileError> {
        if self.db.book_format(hash).map_err(|error| RemoteFileError::Failed(error.to_string()))?.is_none() {
            return Err("file is not in this library".into());
        }
        Ok(())
    }

    pub(crate) async fn read_file_range(&self, source: &RemoteFile, offset: u64, length: usize) -> Result<Vec<u8>, RemoteFileError> {
        self.require_library_book(&source.hash)?;
        self.remote_book_access(source).map_err(|error| RemoteFileError::Failed(error.to_string()))?.read(offset, length).await
    }

    pub(crate) async fn read_file_ranges(&self, source: &RemoteFile, ranges: Vec<(u64, usize)>) -> Result<Vec<u8>, RemoteFileError> {
        self.require_library_book(&source.hash)?;
        // Validate the worker command before it can reach the storage server.
        range_byte_count(&ranges, source.length, 4 * 1024 * 1024).ok_or("invalid book ranges")?;
        self.remote_book_access(source).map_err(|error| RemoteFileError::Failed(error.to_string()))?.read_ranges(ranges).await
    }

    pub(crate) async fn read_file_bundle(&self, source: &RemoteFile, ranges: Vec<(u64, usize)>) -> Result<Vec<u8>, RemoteFileError> {
        self.require_library_book(&source.hash)?;
        self.remote_book_access(source).map_err(|error| RemoteFileError::Failed(error.to_string()))?.read_bundle(ranges).await
    }

    pub(crate) async fn describe_remote_file(&self, hash: BookId) -> Result<(RemoteFile, Vec<u8>), RemoteFileError> {
        describe_remote_file(self.sync.http_client(), &self.account().map_err(|error| RemoteFileError::Failed(error.to_string()))?, self.id, hash).await
    }
}

pub(crate) fn file_auth_error(error: library_runtime::account::AccountSessionError) -> RemoteFileError {
    use library_runtime::account::AccountSessionError;
    match error {
        AccountSessionError::Authentication(account_client::AuthError::Http(error)) => match error.status() {
            Some(status) => RemoteFileError::status(status),
            None => RemoteFileError::transport(error),
        },
        error => RemoteFileError::Failed(error.to_string()),
    }
}

/// Opening and reading books is interactive and always allowed on metered
/// connections. Automatic transfer callers apply their own transfer policy.
pub(crate) async fn authenticated_file_request<T, F: Future<Output = Result<T, RemoteFileError>>>(account: &LibraryAccount, request: impl FnMut(account_client::Session) -> F) -> Result<T, RemoteFileError> {
    authenticated_request(account, RefreshPolicy::BeforeRequest, request, |error| matches!(error, RemoteFileError::Unauthorized)).await.map_err(|error| match error {
        AuthenticatedRequestError::Request(error) => error,
        AuthenticatedRequestError::Session(error) => file_auth_error(error),
        AuthenticatedRequestError::MissingSession => RemoteFileError::Unauthorized,
    })
}

pub(crate) async fn describe_remote_file(client: &Client, account: &LibraryAccount, library: LibraryId, hash: BookId) -> Result<(RemoteFile, Vec<u8>), RemoteFileError> {
    authenticated_file_request(account, |session| async move {
        let url = sync_transport::blob_endpoint(session.server_url(), &library, &hash).map_err(|error| RemoteFileError::Failed(error.to_string()))?;
        book_access::remote_file::describe(client, url, session.token(), hash).await
    })
    .await
}

pub(crate) async fn read_remote_file(client: &Client, account: &LibraryAccount, library: LibraryId, source: &RemoteFile, offset: u64, length: usize) -> Result<Vec<u8>, RemoteFileError> {
    authenticated_file_request(account, |session| async move {
        let url = sync_transport::blob_endpoint(session.server_url(), &library, &source.hash).map_err(|error| RemoteFileError::Failed(error.to_string()))?;
        book_access::remote_file::fetch_range(client, url, source, session.token(), offset, length).await
    })
    .await
}
