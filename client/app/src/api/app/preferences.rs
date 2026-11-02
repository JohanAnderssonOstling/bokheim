use super::AppClient;
use crate::runtime::AppCommand;
use crate::BackendError;

impl AppClient {
    pub async fn update_browsing_preference(&self, patch: app_preferences::BrowsingPreferencePatch) -> Result<app_preferences::BrowsingPreferences, BackendError> {
        self.request(AppCommand::UpdateBrowsingPreference { patch }).await
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "../../../tests/app/preferences/annotation_export_tests.rs"]
mod annotation_export_tests;
