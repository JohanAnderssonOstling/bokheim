//! Cloud credentials are device-owned account data. They never live in a
//! library folder or travel with its metadata/progress synchronization.

use super::data::LibraryRegistry;
use cloud_storage::{CloudError, connections::{CloudConnections, ConnectionPersistence, GoogleConnection}, oauth::{GoogleOAuth, GoogleOAuthConfig}};
use std::sync::Arc;
use sync_common::storage::ProviderId;
use super::core::AppBackend;
use crate::{BackendError, LibraryEntry};
use cloud_storage::{CloudEntry, CloudFolder, FolderPage, connections::ConnectedDrive, oauth::GoogleAccount};
use serde::{Deserialize, Serialize};
use sync_common::storage::{CloudLibraryRoot, LibraryStorage, ProviderScope};
use std::collections::HashSet;
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CloudLoginRequest {
    pub attempt_id: Uuid,
    pub authorization_url: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CloudFolderListing {
    pub folder: CloudFolder,
    pub page: FolderPage,
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) type CloudLoginReceiver = tokio::sync::oneshot::Receiver<Result<GoogleAccount, BackendError>>;

const GOOGLE_CONNECTIONS: &str = "cloud-google-connections:v1";

struct RegistryConnections(LibraryRegistry);

impl ConnectionPersistence for RegistryConnections {
    fn load_google(&self, account: &ProviderId) -> Result<Option<GoogleConnection>, CloudError> {
        let bytes = self.0.read_value(GOOGLE_CONNECTIONS).map_err(persistence_error)?;
        let connections: Vec<GoogleConnection> = bytes.map(|bytes| serde_json::from_slice(&bytes)).transpose().map_err(persistence_error)?.unwrap_or_default();
        Ok(connections.into_iter().find(|connection| &connection.account.id == account))
    }

    fn save_google(&self, connection: &GoogleConnection, previous_access_token: Option<&str>) -> Result<bool, CloudError> {
        let connection = connection.clone();
        let previous = previous_access_token.map(str::to_owned);
        self.0.update_value(GOOGLE_CONNECTIONS, move |bytes| {
            let mut connections: Vec<GoogleConnection> = bytes.map(serde_json::from_slice).transpose()?.unwrap_or_default();
            let current = connections.iter().position(|current| current.account.id == connection.account.id);
            if let Some(previous) = &previous {
                if !current.is_some_and(|index| connections[index].tokens.access_token == *previous) { return Ok(None); }
            }
            match current { Some(index) => connections[index] = connection, None => connections.push(connection) }
            Ok(Some(serde_json::to_vec(&connections)?))
        }).map_err(persistence_error)
    }
}

pub(super) fn connections(registry: LibraryRegistry) -> Result<CloudConnections, crate::BackendError> {
    let Some(client_id) = configured_client_id() else { return Ok(CloudConnections::default()) };
    let oauth = GoogleOAuth::new(GoogleOAuthConfig { client_id, client_secret: configured_client_secret() }).map_err(crate::BackendError::operation)?;
    Ok(CloudConnections::new(oauth, Arc::new(RegistryConnections(registry))))
}

fn configured_client_id() -> Option<String> {
    #[cfg(not(target_arch = "wasm32"))]
    if let Ok(value) = std::env::var("BOKHEIM_GOOGLE_DRIVE_CLIENT_ID") { if !value.trim().is_empty() { return Some(value); } }
    option_env!("BOKHEIM_GOOGLE_DRIVE_CLIENT_ID").filter(|value| !value.trim().is_empty()).map(str::to_owned)
}
fn configured_client_secret() -> Option<String> {
    #[cfg(not(target_arch = "wasm32"))]
    if let Ok(value) = std::env::var("BOKHEIM_GOOGLE_DRIVE_CLIENT_SECRET") { if !value.trim().is_empty() { return Some(value); } }
    option_env!("BOKHEIM_GOOGLE_DRIVE_CLIENT_SECRET").filter(|value| !value.trim().is_empty()).map(str::to_owned)
}
fn persistence_error(_: impl std::fmt::Display) -> CloudError { CloudError::Retryable("Could not read or save this device's cloud connection".into()) }

impl AppBackend {
    pub(crate) fn google_cloud_accounts(&self) -> Result<Vec<GoogleAccount>, BackendError> {
        let bytes = self.state.registry.read_value(GOOGLE_CONNECTIONS).map_err(BackendError::operation)?;
        let connections: Vec<GoogleConnection> = bytes.map(|bytes| serde_json::from_slice(&bytes)).transpose().map_err(BackendError::operation)?.unwrap_or_default();
        Ok(connections.into_iter().map(|connection| connection.account).collect())
    }

    pub(crate) fn disconnect_google_cloud_account(&self, id: ProviderId) -> Result<(), BackendError> {
        self.state.registry.update_value(GOOGLE_CONNECTIONS, move |bytes| {
            let mut connections: Vec<GoogleConnection> = bytes.map(serde_json::from_slice).transpose()?.unwrap_or_default();
            connections.retain(|connection| connection.account.id != id);
            Ok(Some(serde_json::to_vec(&connections)?))
        }).map_err(BackendError::operation)?;
        Ok(())
    }

    #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
    pub(crate) async fn begin_google_cloud_login(&self) -> Result<CloudLoginRequest, BackendError> {
        let pending = cloud_storage::native_login::NativeGoogleLogin::begin(self.state.cloud.clone()).await.map_err(BackendError::operation)?;
        let request = CloudLoginRequest { attempt_id: Uuid::new_v4(), authorization_url: pending.authorization_url().to_string() };
        let (sender, receiver) = tokio::sync::oneshot::channel();
        self.state.cloud_logins.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(request.attempt_id, receiver);
        let weak = Arc::downgrade(&self.state);
        let attempt = request.attempt_id;
        self.state.executor.spawn_detached(async move {
            let result = pending.finish().await.map_err(BackendError::operation);
            let _ = sender.send(result);
            let _ = crate::executor::sleep(std::time::Duration::from_secs(60)).await;
            if let Some(state) = weak.upgrade() { state.cloud_logins.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(&attempt); }
        });
        Ok(request)
    }

    #[cfg(any(target_arch = "wasm32", target_os = "android"))]
    pub(crate) async fn begin_google_cloud_login(&self) -> Result<CloudLoginRequest, BackendError> {
        Err(BackendError::message("Google Drive sign-in is not configured for this platform yet"))
    }

    pub(crate) async fn wait_google_cloud_login(&self, attempt: Uuid) -> Result<GoogleAccount, BackendError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let receiver = self.state.cloud_logins.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(&attempt).ok_or_else(|| BackendError::message("Google sign-in is no longer pending"))?;
            receiver.await.map_err(|_| BackendError::message("Google sign-in was interrupted"))?
        }
        #[cfg(target_arch = "wasm32")]
        { let _ = attempt; Err(BackendError::message("Google Drive sign-in is not configured for this platform yet")) }
    }

    pub(crate) async fn cloud_folders(&self, scope: ProviderScope, folder: Option<ProviderId>, page: Option<String>) -> Result<CloudFolderListing, BackendError> {
        let connection = self.state.cloud.google_drive(scope).map_err(BackendError::operation)?;
        let id = folder.unwrap_or(ProviderId::new("root").expect("static root alias"));
        let CloudEntry::Folder(folder) = connection.describe(&id).await.map_err(BackendError::operation)? else { return Err(BackendError::message("selected cloud item is not a folder")); };
        let listing = connection.list_folder(&folder.id, page.as_deref(), true).await.map_err(BackendError::operation)?;
        Ok(CloudFolderListing { folder, page: listing })
    }

    pub(crate) async fn create_cloud_folder(&self, scope: ProviderScope, parent: ProviderId, name: String) -> Result<CloudFolder, BackendError> {
        self.state.cloud.google_drive(scope).map_err(BackendError::operation)?.create_folder(&parent, &name).await.map_err(BackendError::operation)
    }

    pub(crate) async fn create_cloud_library(&self, name: String, root: CloudLibraryRoot) -> Result<LibraryEntry, BackendError> {
        let connection = self.state.cloud.google_drive(root.scope.clone()).map_err(BackendError::operation)?;
        let selected = cloud_ancestors(&connection, &root.folder_id).await?;
        for entry in self.libraries()? {
            let storage = self.state.registry.storage(entry.library_id()).map_err(BackendError::operation)?;
            if let Some(existing) = storage.external().filter(|existing| existing.scope == root.scope) {
                if selected.contains(&existing.folder_id) || cloud_ancestors(&connection, &existing.folder_id).await?.contains(&root.folder_id) {
                    return Err(BackendError::message(format!("This folder overlaps the storage folder for {}", entry.library_name())));
                }
            }
        }
        let entry = self.state.registry.create_with_storage(&name, None, LibraryStorage::External(root)).map_err(BackendError::operation)?;
        self.finish_created_library(entry)
    }
}

async fn cloud_ancestors(connection: &ConnectedDrive, folder: &ProviderId) -> Result<HashSet<ProviderId>, BackendError> {
    let mut pending = vec![folder.clone()];
    let mut seen = HashSet::new();
    while let Some(id) = pending.pop() {
        if !seen.insert(id.clone()) { continue; }
        if seen.len() > 1024 { return Err(BackendError::message("cloud folder ancestry exceeds the supported depth")); }
        let CloudEntry::Folder(folder) = connection.describe(&id).await.map_err(BackendError::operation)? else { return Err(BackendError::message("selected cloud item is not a folder")); };
        pending.extend(folder.parent_ids);
    }
    Ok(seen)
}
