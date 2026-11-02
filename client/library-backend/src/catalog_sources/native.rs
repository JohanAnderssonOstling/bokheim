use super::*;
use crate::executor::{BackendExecutor, BackendOutput};
use crate::integration::{LibraryAccount, NotificationInterest};
use crate::library::handle::{LibraryActorCommand, LibraryControl};
use crate::library::{LibraryDirectory, LibraryOpenConfig, LibraryRuntimeSpec};
use crate::{LibraryClient, LibraryHandle};
use account_client::Url;
use client_platform_runtime::storage_queue::{self, Priority};
use library_registry::LibraryRegistry;
use std::collections::HashSet;
use std::sync::{Arc, Mutex, PoisonError};
use tokio::sync::OnceCell;

const CATALOG_FEEDS: &str = "catalog-opds-feeds";
fn credentials_key(id: OpdsSourceId) -> String {
    format!("opds-credentials/{id}")
}

pub struct CatalogServiceConfig {
    pub registry: LibraryRegistry,
    pub libraries: Arc<LibraryDirectory>,
    pub executor: BackendExecutor,
    pub account: LibraryAccount,
    pub notification_interest: NotificationInterest,
    pub sync_server: Url,
    pub metadata_server: String,
}
struct Inner {
    config: CatalogServiceConfig,
    owned: Mutex<HashSet<OpdsSourceId>>,
}
impl Drop for Inner {
    fn drop(&mut self) {
        let libraries = self.config.libraries.clone();
        let owned = std::mem::take(self.owned.get_mut().unwrap_or_else(PoisonError::into_inner));
        let _ = storage_queue::dispatch_cleanup(move || {
            for id in owned {
                let _ = libraries.retire(&id);
            }
        });
    }
}
#[derive(Clone)]
pub struct CatalogService(Arc<Inner>);
impl CatalogService {
    /// The host supplies capabilities once; the service owns catalog operations and runtimes.
    pub fn start(config: CatalogServiceConfig) -> Self {
        Self(Arc::new(Inner { config, owned: Mutex::new(HashSet::new()) }))
    }

    fn with_storage<T: BackendOutput>(&self, operation: impl FnOnce(&Self) -> Result<T, BackendError> + Send + 'static) -> Result<T, BackendError> {
        let service = self.clone();
        storage_queue::run_sync(Priority::Interactive, move || operation(&service)).map_err(BackendError::message)?
    }
    fn stored_catalog_opds_feeds(&self) -> Result<Vec<OpdsSource>, BackendError> {
        self.0.config.registry.read_value(CATALOG_FEEDS).map_err(BackendError::operation)?.map(|bytes| serde_json::from_slice(&bytes).map_err(BackendError::operation)).unwrap_or_else(|| Ok(Vec::new()))
    }

    fn save_catalog_opds_feeds(&self, feeds: &[OpdsSource]) -> Result<(), BackendError> {
        self.0.config.registry.write_value(CATALOG_FEEDS, &serde_json::to_vec(feeds).map_err(BackendError::operation)?).map_err(BackendError::operation)
    }

    pub fn credentials(&self, id: OpdsSourceId) -> Result<Option<OpdsCredentials>, BackendError> {
        self.0.config.registry.read_value(&credentials_key(id)).map_err(BackendError::operation)?.map(|bytes| serde_json::from_slice(&bytes)).transpose().map_err(BackendError::operation)
    }

    /// Only this device's catalog subscriptions, sorted by name.
    pub fn list(&self) -> Result<Vec<OpdsSource>, BackendError> {
        self.open()?;
        let mut feeds = self.stored_catalog_opds_feeds()?;
        feeds.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        Ok(feeds)
    }

    /// Catalog browsing owns disposable runtimes, without registered libraries.
    pub fn open(&self) -> Result<(), BackendError> {
        self.with_storage(|this| {
            for feed in this.stored_catalog_opds_feeds()? {
                let id = feed.id;
                if this.0.config.libraries.try_get(&id)?.is_none() {
                    if let Err(error) = this.open_catalog_opds_feed(feed) {
                        log::warn!("Could not open local catalog feed {id}: {error}");
                    }
                }
            }
            Ok(())
        })
    }

    fn open_catalog_opds_feed(&self, feed: OpdsSource) -> Result<(), BackendError> {
        let database_locator = self.0.config.registry.library_database_locator(&feed.id, "");
        let credentials = self.credentials(feed.id)?;
        let feed_id = feed.id;
        let spec = LibraryRuntimeSpec {
            config: LibraryOpenConfig::catalog_feed(feed, database_locator, credentials, self.0.config.sync_server.clone(), self.0.config.metadata_server.clone())?,
            account: self.0.config.account.clone(),
            notification_interest: self.0.config.notification_interest.clone(),
            root_failures: None,
        };
        self.0.config.libraries.open_catalog(spec, &self.0.config.executor)?;
        self.0.owned.lock().unwrap_or_else(PoisonError::into_inner).insert(feed_id);
        Ok(())
    }

    fn commit_add(&self, feed: OpdsSource, credentials: Option<OpdsCredentials>) -> Result<(), BackendError> {
        self.with_storage(move |this| {
            if let Some(credentials) = credentials {
                this.set_credentials(feed.id, credentials)?;
            }
            let mut feeds = this.stored_catalog_opds_feeds()?;
            feeds.push(feed);
            this.save_catalog_opds_feeds(&feeds)?;
            this.open()
        })
    }

    pub fn remove(&self, id: OpdsSourceId) -> Result<(), BackendError> {
        self.with_storage(move |this| {
            let mut feeds = this.stored_catalog_opds_feeds()?;
            if !feeds.iter().any(|feed| feed.id == id) {
                return Ok(());
            }
            feeds.retain(|feed| feed.id != id);
            this.save_catalog_opds_feeds(&feeds)?;
            this.forget_credentials(id)?;
            this.0.config.libraries.retire(&id)?;
            this.0.owned.lock().unwrap_or_else(PoisonError::into_inner).remove(&id);
            Ok(())
        })
    }

    pub fn forget_credentials(&self, id: OpdsSourceId) -> Result<(), BackendError> {
        self.0.config.registry.remove_value(&credentials_key(id)).map_err(BackendError::operation)
    }

    pub fn set_credentials(&self, id: OpdsSourceId, credentials: OpdsCredentials) -> Result<(), BackendError> {
        self.with_storage(move |this| {
            this.0.config.registry.write_value(&credentials_key(id), &serde_json::to_vec(&credentials).map_err(BackendError::operation)?).map_err(BackendError::operation)?;
            if let Some(library) = this.0.config.libraries.try_get(&id)? {
                library.commands.try_send(LibraryActorCommand::Control(LibraryControl::SetOpdsCredentials(Some(credentials)))).map_err(|_| BackendError::message("OPDS library stopped"))?;
            }
            Ok(())
        })
    }

    fn handle(&self, id: OpdsSourceId) -> Result<LibraryHandle, BackendError> {
        self.with_storage(move |service| {
            if let Some(handle) = service.0.config.libraries.try_get(&id)? {
                return Ok(handle);
            }
            let feed = service.stored_catalog_opds_feeds()?.into_iter().find(|feed| feed.id == id).ok_or_else(|| BackendError::message("this catalog feed is no longer available"))?;
            service.open_catalog_opds_feed(feed)?;
            service.0.config.libraries.get(&id)
        })
    }
}
impl CatalogClient {
    pub(crate) fn new(service: CatalogService, host: Arc<dyn Send + Sync>) -> Self {
        Self { service, _host: host }
    }
    async fn run<T: BackendOutput>(&self, operation: impl FnOnce(CatalogService) -> Result<T, BackendError> + Send + 'static) -> Result<T, BackendError> {
        let service = self.service.clone();
        storage_queue::run(Priority::Interactive, move || operation(service)).await.map_err(BackendError::message)?
    }
    pub async fn list(&self) -> Result<Vec<OpdsSource>, BackendError> {
        self.run(|service| service.list()).await
    }
    pub async fn add(&self, url: String, name: Option<String>, credentials: Option<OpdsCredentials>) -> Result<AddOpdsFeedResult, BackendError> {
        let Some(feed) = library_opds::validated_source(url, name, credentials.as_ref()).await? else { return Ok(AddOpdsFeedResult::SignInRequired) };
        self.run(move |service| service.commit_add(feed, credentials)).await?;
        Ok(AddOpdsFeedResult::Added)
    }
    pub async fn remove(&self, id: OpdsSourceId) -> Result<(), BackendError> {
        self.run(move |service| service.remove(id)).await
    }
    pub async fn set_credentials(&self, id: OpdsSourceId, credentials: OpdsCredentials) -> Result<(), BackendError> {
        self.run(move |service| service.set_credentials(id, credentials)).await
    }
    pub fn library(&self, id: OpdsSourceId) -> LibraryClient {
        LibraryClient::new(id, Connection { catalog: self.clone(), id, handle: OnceCell::new() })
    }
}
struct Connection {
    catalog: CatalogClient,
    id: OpdsSourceId,
    handle: OnceCell<LibraryHandle>,
}
impl crate::client::ClientTransport for Connection {
    fn handle(&self) -> crate::executor::BoxedBackendFuture<'_, Result<&LibraryHandle, BackendError>> {
        Box::pin(async move {
            self.handle
                .get_or_try_init(|| {
                    self.catalog.run({
                        let id = self.id;
                        move |service| service.handle(id)
                    })
                })
                .await
        })
    }
}
