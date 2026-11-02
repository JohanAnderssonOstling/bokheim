//! Foreground library management; native calls go directly to the owning service.
use super::{LibraryService, LocalLibraryStorageUsage};
use crate::catalog_sources::CatalogClient;
use crate::executor::BoxedBackendFuture;
#[cfg(not(target_arch = "wasm32"))]
use crate::{client::ClientTransport, LibraryHandle};
use crate::{BackendError, LibraryClient, LibraryId};
#[cfg(not(target_arch = "wasm32"))]
use client_platform_runtime::startup_profile::StartupProfile;
#[cfg(target_arch = "wasm32")]
use client_runtime::reply::{Reply, WireReply};
#[cfg(target_arch = "wasm32")]
use client_runtime::wire::{decode_worker_payload, WorkerPayload};
use library_model::{AddOpdsFeedResult, OpdsCredentials};
use library_registry::LibraryEntry;
#[cfg(target_arch = "wasm32")]
use std::rc::Rc;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;
use tokio::sync::watch;
#[cfg(not(target_arch = "wasm32"))]
use tokio::sync::OnceCell;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum ManagementCommand {
    List,
    Create { name: String, asset_storage_enabled: bool },
    EnsureImportLibrary,
    AddOpdsLibrary { url: String, name: Option<String>, credentials: Option<OpdsCredentials> },
    LocalStorageUsage,
    TextSearchIndexBytes,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum ManagementSubscription {
    LibraryList,
    Notifications { browsing: bool },
}

/// Dropping the foreground view releases its interest immediately on native,
/// or cancels its browser worker subscription.
pub struct NotificationLease {
    #[cfg(not(target_arch = "wasm32"))]
    _lease: crate::integration::InterestLease,
    #[cfg(target_arch = "wasm32")]
    _receiver: async_channel::Receiver<()>,
}

#[derive(Clone)]
pub struct ManagementClient {
    #[cfg(not(target_arch = "wasm32"))]
    service: LibraryService,
    #[cfg(not(target_arch = "wasm32"))]
    startup: watch::Receiver<Option<Result<(), BackendError>>>,
    #[cfg(not(target_arch = "wasm32"))]
    host: Arc<dyn Send + Sync>,
    #[cfg(target_arch = "wasm32")]
    transport: Rc<dyn ManagementTransport>,
}

#[cfg(target_arch = "wasm32")]
pub trait ManagementTransport {
    fn library(&self, id: LibraryId) -> LibraryClient;
    fn request(&self, command: ManagementCommand) -> BoxedBackendFuture<'_, Result<WorkerPayload, BackendError>>;
    fn list_changes(&self) -> BoxedBackendFuture<'_, Result<watch::Receiver<()>, BackendError>>;
    fn notification_interest(&self, browsing: bool) -> BoxedBackendFuture<'_, Result<async_channel::Receiver<()>, BackendError>>;
}

impl ManagementClient {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(service: LibraryService, startup: watch::Receiver<Option<Result<(), BackendError>>>, host: impl Send + Sync + 'static) -> Self {
        Self { service, startup, host: Arc::new(host) }
    }

    #[cfg(target_arch = "wasm32")]
    pub fn new(transport: impl ManagementTransport + 'static) -> Self {
        Self { transport: Rc::new(transport) }
    }

    pub fn library(&self, id: LibraryId) -> LibraryClient {
        #[cfg(not(target_arch = "wasm32"))]
        {
            LibraryClient::new(id, Connection { client: self.clone(), id, handle: OnceCell::new() })
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.transport.library(id)
        }
    }

    pub fn catalog(&self) -> CatalogClient {
        #[cfg(not(target_arch = "wasm32"))]
        {
            CatalogClient::new(self.service.state.catalog.clone(), self.host.clone())
        }
        #[cfg(target_arch = "wasm32")]
        {
            CatalogClient::new()
        }
    }

    /// Resolve a destination only when the user imports or downloads a book.
    pub async fn library_for_import(&self, selected: Option<LibraryId>) -> Result<LibraryClient, BackendError> {
        let explicitly_selected = selected.is_some();
        let id = match selected {
            Some(id) => id,
            None => *self.ensure_import_library().await?.library_id(),
        };
        let library = self.library(id);
        if explicitly_selected {
            library.book_count().await.map_err(|error| error.context("selected library is unavailable"))?;
        }
        Ok(library)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) async fn ready(&self) -> Result<&LibraryService, BackendError> {
        let mut startup = self.startup.clone();
        loop {
            if let Some(result) = startup.borrow_and_update().clone() {
                result?;
                return Ok(&self.service);
            }
            startup.changed().await.map_err(|_| BackendError::message("backend stopped during library startup"))?;
        }
    }

    #[cfg(target_arch = "wasm32")]
    async fn request<T: WireReply>(&self, command: ManagementCommand) -> Result<T, BackendError> {
        decode_worker_payload(self.transport.request(command).await?)
    }

    pub async fn libraries(&self) -> Result<Vec<LibraryEntry>, BackendError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.ready().await?.run(|service| service.list()).await
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.request(ManagementCommand::List).await
        }
    }
    pub async fn create_library_with_asset_storage(&self, name: String, asset_storage_enabled: bool) -> Result<LibraryEntry, BackendError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.ready().await?.run(move |service| service.create_with_asset_storage(name, asset_storage_enabled)).await
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.request(ManagementCommand::Create { name, asset_storage_enabled }).await
        }
    }
    pub async fn ensure_import_library(&self) -> Result<LibraryEntry, BackendError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.ready().await?.run(|service| service.ensure_import_library()).await
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.request(ManagementCommand::EnsureImportLibrary).await
        }
    }
    pub async fn add_opds_library(&self, url: String, name: Option<String>, credentials: Option<OpdsCredentials>) -> Result<AddOpdsFeedResult, BackendError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.ready().await?.add_opds_library(url, name, credentials).await
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.request(ManagementCommand::AddOpdsLibrary { url, name, credentials }).await
        }
    }
    pub async fn local_storage_usage(&self) -> Result<Vec<LocalLibraryStorageUsage>, BackendError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.ready().await?.local_storage_usage().await
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.request(ManagementCommand::LocalStorageUsage).await
        }
    }
    pub async fn text_search_index_bytes(&self) -> Result<u64, BackendError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.ready().await?.text_search_index_bytes().await
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.request(ManagementCommand::TextSearchIndexBytes).await
        }
    }
    pub async fn library_list_changes(&self) -> Result<watch::Receiver<()>, BackendError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            Ok(self.service.list_changes())
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.transport.list_changes().await
        }
    }
    pub async fn notification_interest(&self, browsing: bool) -> Result<NotificationLease, BackendError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            Ok(NotificationLease { _lease: self.service.notification_interest(browsing) })
        }
        #[cfg(target_arch = "wasm32")]
        {
            Ok(NotificationLease { _receiver: self.transport.notification_interest(browsing).await? })
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl LibraryService {
    /// Browser hosts decode the domain command and invoke the same service as native clients.
    pub async fn dispatch(&self, command: ManagementCommand) -> Result<Reply, BackendError> {
        match command {
            ManagementCommand::List => self.run(|service| service.list().map(Reply::new)).await,
            ManagementCommand::Create { name, asset_storage_enabled } => self.run(move |service| service.create_with_asset_storage(name, asset_storage_enabled).map(Reply::new)).await,
            ManagementCommand::EnsureImportLibrary => self.run(|service| service.ensure_import_library().map(Reply::new)).await,
            ManagementCommand::AddOpdsLibrary { .. } => Err(BackendError::message("OPDS feeds are not available in the browser")),
            ManagementCommand::LocalStorageUsage => self.local_storage_usage().await.map(Reply::new),
            ManagementCommand::TextSearchIndexBytes => self.text_search_index_bytes().await.map(Reply::new),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
struct Connection {
    client: ManagementClient,
    id: LibraryId,
    handle: OnceCell<LibraryHandle>,
}

#[cfg(not(target_arch = "wasm32"))]
impl ClientTransport for Connection {
    fn handle(&self) -> BoxedBackendFuture<'_, Result<&LibraryHandle, BackendError>> {
        Box::pin(self.handle.get_or_try_init(|| async {
            let mut profile = StartupProfile::new("book_lookup_transport");
            let service = self.client.ready().await?;
            profile.mark("startup_wait");
            let id = self.id;
            let (handle, mut profile) = service
                .run(move |service| {
                    profile.mark("storage_dispatch");
                    let handle = service.state.library_directory.get(&id);
                    profile.mark("directory_get");
                    Ok((handle, profile))
                })
                .await?;
            profile.mark("storage_return");
            handle
        }))
    }
}
