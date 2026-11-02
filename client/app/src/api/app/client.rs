use crate::runtime::{AppCommand, Transport, WireReply};
use crate::services::Services;
use crate::BackendError;
use library_backend::account::Client as AccountClient;
use library_backend::management::ManagementClient;
use std::sync::Arc;

/// The frontend entry point for application services and backend operations.
/// Backend implementation objects are deliberately not part of this API.
///
/// ```compile_fail
/// use app::app::{AppBackend, BackendContext};
/// ```
#[derive(Clone)]
pub struct AppClient {
    pub management: ManagementClient,
    pub account: AccountClient,
    pub(crate) transport: Transport,
    pub(crate) services: Arc<Services>,
}

impl AppClient {
    pub(super) async fn request<T: WireReply>(&self, command: AppCommand) -> Result<T, BackendError> {
        self.transport.request(command).await
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "../../../tests/library_client/tests.rs"]
mod library_client_tests;
