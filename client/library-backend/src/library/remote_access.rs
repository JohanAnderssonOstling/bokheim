//! Account-backed remote reading owned by one library session.

pub(crate) use super::{playback::DirectPlayback, LibrarySession, RemoteFile};
pub(crate) use crate::BackendError;
pub(crate) use crate::ContentHash;
use crate::LibraryId;
use book_access::RemoteFileError;
use std::future::Future;

impl LibrarySession {
    pub(crate) fn account(&self) -> Result<library_runtime::account::LibraryAccount, BackendError> {
        self.sync.clone().account().ok_or_else(|| BackendError::message("library account access is unavailable"))
    }

    pub(crate) fn downloaded_books_only(&self) -> Result<bool, BackendError> {
        Ok(self.account()?.current().map_err(BackendError::operation)?.is_none())
    }

    pub(crate) async fn direct_audio_source(&self, hash: ContentHash) -> Result<DirectPlayback, String> {
        let account = self.account().map_err(|error| error.to_string())?;
        let library = self.id;
        crate::executor::timeout(
            std::time::Duration::from_secs(30),
            authenticated_file_request(&account, |session| async move {
                let mut endpoint = sync_transport::blob_endpoint(session.server_url(), &library, &hash);
                endpoint.set_path(&format!("{}/playback", endpoint.path()));
                let response = self.sync.http_client.post(endpoint).bearer_auth(session.token()).header(reqwest::header::ACCEPT, sync_common::transport::MEDIA_TYPE).send().await.map_err(RemoteFileError::transport)?;
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
                Ok(DirectPlayback { url: url.to_string(), expires_in_seconds: grant.expires_in_seconds, content_hash: grant.content_hash, length: grant.length })
            }),
        )
        .await
        .map_err(|_| "Playback authorization timed out".to_owned())?
        .map_err(|error| error.to_string())
    }

    pub(crate) async fn read_file_range(&self, source: &RemoteFile, offset: u64, length: usize) -> Result<Vec<u8>, RemoteFileError> {
        if self.db.book_format(&source.hash).map_err(|error| RemoteFileError::Failed(error.to_string()))?.is_none() {
            return Err("file is not in this library".into());
        }
        read_remote_file(&self.sync.http_client, &self.account().map_err(|error| RemoteFileError::Failed(error.to_string()))?, self.id, source, offset, length).await
    }

    pub(crate) async fn read_file_bundle(&self, source: &RemoteFile, ranges: Vec<(u64, usize)>) -> Result<Vec<u8>, RemoteFileError> {
        if self.db.book_format(&source.hash).map_err(|error| RemoteFileError::Failed(error.to_string()))?.is_none() {
            return Err("file is not in this library".into());
        }
        let account = self.account().map_err(|error| RemoteFileError::Failed(error.to_string()))?;
        let client = self.sync.http_client.clone();
        let library = self.id;
        authenticated_file_request(&account, move |session| {
            let client = client.clone();
            let source = source.clone();
            let ranges = ranges.clone();
            async move {
                let url = sync_transport::blob_endpoint(session.server_url(), &library, &source.hash);
                book_access::remote_file::fetch_bundle(&client, url, &source, session.token(), ranges).await
            }
        })
        .await
    }

    pub(crate) async fn describe_remote_file(&self, hash: ContentHash) -> Result<(RemoteFile, Vec<u8>), RemoteFileError> {
        describe_remote_file(&self.sync.http_client, &self.account().map_err(|error| RemoteFileError::Failed(error.to_string()))?, self.id, hash).await
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

pub(crate) async fn authenticated_file_request<T, F: Future<Output = Result<T, RemoteFileError>>>(account: &library_runtime::account::LibraryAccount, mut request: impl FnMut(account_client::Session) -> F) -> Result<T, RemoteFileError> {
    let session = account.refresh_if_needed().await.map_err(file_auth_error)?.ok_or("sign in to read this remote file")?;
    let token = session.token().to_owned();
    match request(session).await {
        Err(RemoteFileError::Unauthorized) => {
            let refreshed = account.refresh_after_unauthorized(&token).await.map_err(file_auth_error)?.ok_or("sign in to read this remote file")?;
            request(refreshed).await
        }
        result => result,
    }
}

pub(crate) async fn describe_remote_file(client: &reqwest::Client, account: &library_runtime::account::LibraryAccount, library: LibraryId, hash: ContentHash) -> Result<(RemoteFile, Vec<u8>), RemoteFileError> {
    authenticated_file_request(account, |session| async move {
        let url = sync_transport::blob_endpoint(session.server_url(), &library, &hash);
        book_access::remote_file::describe(client, url, session.token(), hash).await
    })
    .await
}

pub(crate) async fn read_remote_file(client: &reqwest::Client, account: &library_runtime::account::LibraryAccount, library: LibraryId, source: &RemoteFile, offset: u64, length: usize) -> Result<Vec<u8>, RemoteFileError> {
    authenticated_file_request(account, |session| async move {
        let url = sync_transport::blob_endpoint(session.server_url(), &library, &source.hash);
        book_access::remote_file::fetch_range(client, url, source, session.token(), offset, length).await
    })
    .await
}
