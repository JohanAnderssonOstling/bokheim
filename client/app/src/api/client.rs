use library_management::account::Client as AccountClient;
use library_management::management::ManagementClient;

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
    pub preferences: app_preferences::Client,
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "../../tests/library_client/tests.rs"]
mod library_client_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "../../tests/app/preferences/annotation_export_tests.rs"]
mod annotation_export_tests;
