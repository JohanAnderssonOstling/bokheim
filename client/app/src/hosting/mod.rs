//! Shared typed application clients and their host lifetime.

use crate::app::AppBackend;
use crate::{AccountStatus, AppClient};
use library_management::account::Client as AccountClient;
use library_management::management::ManagementClient;
use std::sync::Arc;
use tokio::sync::oneshot;

pub use library_management::account::AccountSnapshot;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod native;
#[cfg(target_arch = "wasm32")]
pub(crate) mod web;

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct AppStartupState {
    pub default_library_root: String,
    pub dictionary_storage_root: String,
    pub preference_storage_root: String,
    pub libraries: Vec<crate::LibraryEntry>,
    pub browsing_preferences: app_preferences::BrowsingPreferences,
    pub account_status: AccountStatus,
    pub reader_preferences: app_preferences::ReaderPreferences,
}

pub(crate) fn client(backend: Arc<AppBackend>, stopped: Option<oneshot::Sender<()>>) -> AppClient {
    if let Some(stopped) = stopped {
        assert!(backend.host_shutdown.set(stopped).is_ok(), "application host is already connected");
    }
    let management = ManagementClient::local(backend.library_service.clone(), backend.clone());
    let account = AccountClient::local(backend.account_service.clone(), backend.clone());
    let preferences = app_preferences::Client::local(backend.preferences.clone(), backend.executor.clone(), backend);
    AppClient { account, management, preferences }
}
