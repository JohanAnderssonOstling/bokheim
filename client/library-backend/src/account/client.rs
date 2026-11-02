use super::AccountSnapshot;
#[cfg(not(target_arch = "wasm32"))]
use super::Service;
use crate::executor::BoxedBackendFuture;
#[cfg(not(target_arch = "wasm32"))]
use crate::management::ManagementClient;
use crate::BackendError;
#[cfg(target_arch = "wasm32")]
use std::rc::Rc as Shared;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc as Shared;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum Command {
    Snapshot,
    Login { email: String, password: String },
    VerifyEmail { email: String, pin: String, password: String },
    Logout,
    DeleteAccount { email: String, password: String },
}

pub trait Transport {
    fn request(&self, command: Command) -> BoxedBackendFuture<'_, Result<AccountSnapshot, BackendError>>;
}

#[derive(Clone)]
pub struct Client {
    #[cfg(not(target_arch = "wasm32"))]
    transport: Shared<dyn Transport + Send + Sync>,
    #[cfg(target_arch = "wasm32")]
    transport: Shared<dyn Transport>,
}

impl Client {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn local(service: Service, management: ManagementClient) -> Self {
        Self { transport: Shared::new(Local { service, management }) }
    }
    #[cfg(target_arch = "wasm32")]
    pub fn new(transport: impl Transport + 'static) -> Self {
        Self { transport: Shared::new(transport) }
    }
    pub async fn account_snapshot(&self) -> Result<AccountSnapshot, BackendError> {
        self.transport.request(Command::Snapshot).await
    }
    pub async fn login(&self, email: String, password: String) -> Result<AccountSnapshot, BackendError> {
        self.transport.request(Command::Login { email, password }).await
    }
    pub async fn verify_email(&self, email: String, pin: String, password: String) -> Result<AccountSnapshot, BackendError> {
        self.transport.request(Command::VerifyEmail { email, pin, password }).await
    }
    pub async fn logout(&self) -> Result<AccountSnapshot, BackendError> {
        self.transport.request(Command::Logout).await
    }
    pub async fn delete_account(&self, email: String, password: String) -> Result<AccountSnapshot, BackendError> {
        self.transport.request(Command::DeleteAccount { email, password }).await
    }
}

#[cfg(not(target_arch = "wasm32"))]
struct Local {
    service: Service,
    management: ManagementClient,
}
#[cfg(not(target_arch = "wasm32"))]
impl Transport for Local {
    fn request(&self, command: Command) -> BoxedBackendFuture<'_, Result<AccountSnapshot, BackendError>> {
        Box::pin(async move {
            self.management.ready().await?;
            self.service.dispatch(command).await
        })
    }
}
