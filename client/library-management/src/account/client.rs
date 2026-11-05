use super::{AccountSnapshot, AccountStatus, Service};
use crate::BackendError;
use crate::executor::BackendSend;
use std::future::Future;
use std::sync::Arc;
use tokio::sync::watch::{self, error::RecvError};

/// Keeps the account service alive while its UI observes identity changes.
pub struct AccountUpdates {
    changes: watch::Receiver<AccountStatus>,
    _client: Client,
}

impl AccountUpdates {
    pub fn current(&mut self) -> AccountStatus {
        self.changes.borrow_and_update().clone()
    }
    pub async fn changed(&mut self) -> Result<AccountStatus, RecvError> {
        self.changes.changed().await?;
        Ok(self.current())
    }
}

#[derive(Clone)]
pub struct Client {
    service: Arc<Service>,
    _host: Arc<dyn Send + Sync>,
}
impl Client {
    pub fn local(service: Arc<Service>, host: Arc<dyn Send + Sync>) -> Self {
        Self { service, _host: host }
    }
    pub fn subscribe_status(&self) -> AccountUpdates {
        AccountUpdates { changes: self.service.subscribe_status(), _client: self.clone() }
    }
    pub async fn account_snapshot(&self) -> Result<AccountSnapshot, BackendError> {
        self.service.executor.with_io_context(self.service.account_snapshot()).await
    }
    async fn change<F: Future<Output = Result<(), BackendError>> + BackendSend>(&self, operation: impl FnOnce(Arc<Service>) -> F + BackendSend) -> Result<AccountSnapshot, BackendError> {
        self.service.executor.with_io_context(async {
            operation(self.service.clone()).await?;
            self.service.account_snapshot().await
        }).await
    }

    pub async fn login(&self, email: String, password: String) -> Result<AccountSnapshot, BackendError> {
        self.change(move |service| async move { service.login(&email, &password).await }).await
    }
    pub async fn verify_email(&self, email: String, pin: String, password: String) -> Result<AccountSnapshot, BackendError> {
        self.change(move |service| async move { service.verify_email(&email, &pin, &password).await }).await
    }
    pub async fn logout(&self) -> Result<AccountSnapshot, BackendError> {
        self.change(|service| async move { service.logout().await }).await
    }
    pub async fn delete_account(&self, email: String, password: String) -> Result<AccountSnapshot, BackendError> {
        self.change(move |service| async move { service.delete_account(&email, &password).await }).await
    }
}
