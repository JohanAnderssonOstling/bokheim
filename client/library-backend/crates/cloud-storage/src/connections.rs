//! Device-owned connections. No Bokheim account or file-storage service is
//! needed to browse or read from a connected cloud provider.

use crate::google_drive::{DriveUpload, GoogleDrive, UploadProgress};
use crate::oauth::{GoogleAccount, GoogleOAuth, GoogleTokens, PendingGoogleLogin};
use crate::scanner::{ScanCursor, ScanPage};
use crate::{ChangesPage, CloudDirectoryIdentity, CloudEntry, CloudError, CloudFolder, FolderPage, ProviderRuntime};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use std::{future::Future, sync::Arc};
use sync_common::storage::{CloudProvider, ProviderFile, ProviderId, ProviderScope};
use tokio::sync::Mutex;

#[derive(Clone, Serialize, Deserialize)]
pub struct GoogleConnection {
    pub account: GoogleAccount,
    pub tokens: GoogleTokens,
}

/// Hosts persist these values in device-owned account storage. Refresh commits
/// compare the previous access token so signing out during refresh cannot
/// recreate a removed connection or overwrite a more recent explicit login.
pub trait ConnectionPersistence: ProviderRuntime {
    fn load_google(&self, account: &ProviderId) -> Result<Option<GoogleConnection>, CloudError>;
    fn save_google(&self, connection: &GoogleConnection, previous_access_token: Option<&str>) -> Result<bool, CloudError>;
}

#[derive(Clone, Default)]
pub struct CloudConnections {
    oauth: Option<GoogleOAuth>,
    persistence: Option<Arc<dyn ConnectionPersistence>>,
    refresh: Arc<Mutex<()>>,
}

impl CloudConnections {
    #[cfg(feature = "test-support")]
    pub fn test_google_drive(&self, scope: ProviderScope, endpoint: Url) -> Result<ConnectedDrive, CloudError> {
        Ok(ConnectedDrive { drive: GoogleDrive::for_test(scope, endpoint)?, connections: self.clone() })
    }

    pub fn new(oauth: GoogleOAuth, persistence: Arc<dyn ConnectionPersistence>) -> Self {
        Self { oauth: Some(oauth), persistence: Some(persistence), refresh: Arc::new(Mutex::new(())) }
    }

    pub fn begin_google_login(&self, redirect: Url) -> Result<PendingGoogleLogin, CloudError> {
        self.oauth.as_ref().ok_or_else(configuration_error)?.begin_login(redirect)
    }

    pub async fn finish_google_login(&self, login: PendingGoogleLogin, callback: Url) -> Result<GoogleAccount, CloudError> {
        let (tokens, account) = login.finish(callback, now()?).await?;
        let connection = GoogleConnection { account: account.clone(), tokens };
        if !self.persistence.as_ref().ok_or_else(configuration_error)?.save_google(&connection, None)? {
            return Err(CloudError::Retryable("Could not save the cloud connection".into()));
        }
        Ok(account)
    }

    pub fn google_drive(&self, scope: ProviderScope) -> Result<ConnectedDrive, CloudError> {
        if scope.provider != CloudProvider::GoogleDrive {
            return Err(CloudError::InvalidRequest("This cloud provider is not implemented yet".into()));
        }
        Ok(ConnectedDrive { drive: GoogleDrive::new(scope)?, connections: self.clone() })
    }

    async fn token(&self, scope: &ProviderScope, rejected: Option<&str>) -> Result<String, CloudError> {
        let persistence = self.persistence.as_ref().ok_or_else(configuration_error)?;
        let load = || -> Result<GoogleConnection, CloudError> {
            let connection = persistence.load_google(&scope.account_id)?.ok_or(CloudError::AuthenticationRequired)?;
            if connection.account.id != scope.account_id {
                return Err(CloudError::AuthenticationRequired);
            }
            Ok(connection)
        };
        let needs_refresh = |connection: &GoogleConnection, now: u64| connection.tokens.expires_at <= now.saturating_add(60) || rejected.is_some_and(|token| token == connection.tokens.access_token);
        let mut connection = load()?;
        if !needs_refresh(&connection, now()?) {
            return Ok(connection.tokens.access_token);
        }
        let _refresh = self.refresh.lock().await;
        connection = load()?;
        if !needs_refresh(&connection, now()?) {
            return Ok(connection.tokens.access_token);
        }
        let previous = connection.tokens.access_token.clone();
        connection.tokens = self.oauth.as_ref().ok_or_else(configuration_error)?.refresh(&connection.tokens, now()?).await?;
        if !persistence.save_google(&connection, Some(&previous))? {
            return Err(CloudError::AuthenticationRequired);
        }
        Ok(connection.tokens.access_token)
    }

    async fn authenticated<T, F, Fut>(&self, scope: &ProviderScope, mut operation: F) -> Result<T, CloudError>
    where
        F: FnMut(String) -> Fut,
        Fut: Future<Output = Result<T, CloudError>>,
    {
        let token = self.token(scope, None).await?;
        match operation(token.clone()).await {
            Err(CloudError::AuthenticationRequired) => operation(self.token(scope, Some(&token)).await?).await,
            result => result,
        }
    }
}

#[derive(Clone)]
pub struct ConnectedDrive {
    drive: GoogleDrive,
    connections: CloudConnections,
}

impl ConnectedDrive {
    pub fn scope(&self) -> &ProviderScope {
        self.drive.scope()
    }

    pub async fn list_folder(&self, folder: &ProviderId, page: Option<&str>, folders_only: bool) -> Result<FolderPage, CloudError> {
        self.connections.authenticated(self.scope(), |token| async move { self.drive.list_folder(&token, folder, page, folders_only).await }).await
    }
    pub async fn describe(&self, file: &ProviderId) -> Result<CloudEntry, CloudError> {
        self.connections.authenticated(self.scope(), |token| async move { self.drive.describe(&token, file).await }).await
    }
    pub async fn read_range(&self, file: &ProviderFile, offset: u64, length: usize) -> Result<Vec<u8>, CloudError> {
        self.connections.authenticated(self.scope(), |token| async move { self.drive.read_range(&token, file, offset, length).await }).await
    }
    pub async fn start_checkpoint(&self) -> Result<String, CloudError> {
        self.connections.authenticated(self.scope(), |token| async move { self.drive.start_checkpoint(&token).await }).await
    }
    pub async fn changes(&self, checkpoint: &str) -> Result<ChangesPage, CloudError> {
        self.connections.authenticated(self.scope(), |token| async move { self.drive.changes(&token, checkpoint).await }).await
    }
    pub async fn scan_page(&self, cursor: &ScanCursor) -> Result<ScanPage, CloudError> {
        self.connections.authenticated(self.scope(), |token| async move { cursor.next_page(&self.drive, &token).await }).await
    }
    pub async fn create_folder(&self, parent: &ProviderId, name: &str) -> Result<CloudFolder, CloudError> {
        self.connections.authenticated(self.scope(), |token| async move { self.drive.create_folder(&token, parent, name).await }).await
    }
    pub async fn create_library_folder(&self, parent: &ProviderId, name: &str, identity: &CloudDirectoryIdentity) -> Result<CloudFolder, CloudError> {
        self.connections.authenticated(self.scope(), |token| async move { self.drive.create_library_folder(&token, parent, name, Some(identity)).await }).await
    }
    pub async fn allocate_file_id(&self) -> Result<ProviderId, CloudError> {
        self.connections.authenticated(self.scope(), |token| async move { self.drive.allocate_file_id(&token).await }).await
    }
    pub async fn begin_upload(&self, parent: &ProviderId, file: &ProviderId, name: &str, size: u64) -> Result<DriveUpload, CloudError> {
        self.connections.authenticated(self.scope(), |token| async move { self.drive.begin_upload(&token, parent, file, name, size).await }).await
    }
    pub async fn upload_chunk(&self, upload: &DriveUpload, offset: u64, bytes: Vec<u8>) -> Result<UploadProgress, CloudError> {
        self.connections
            .authenticated(self.scope(), |token| {
                let bytes = bytes.clone();
                async move { self.drive.upload_chunk(&token, upload, offset, bytes).await }
            })
            .await
    }
    pub async fn upload_status(&self, upload: &DriveUpload) -> Result<UploadProgress, CloudError> {
        self.connections.authenticated(self.scope(), |token| async move { self.drive.upload_status(&token, upload).await }).await
    }
}

fn configuration_error() -> CloudError {
    CloudError::InvalidRequest("Google Drive sign-in requires a configured OAuth client ID".into())
}
fn now() -> Result<u64, CloudError> {
    web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map(|duration| duration.as_secs()).map_err(|_| CloudError::InvalidRequest("Device clock is before the Unix epoch".into()))
}
