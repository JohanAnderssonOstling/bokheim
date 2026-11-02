use super::AppClient;
use crate::runtime::AppCommand;
use crate::BackendError;

impl AppClient {
    pub async fn text_search_index_bytes(&self) -> Result<u64, BackendError> {
        self.request(AppCommand::TextSearchIndexBytes).await
    }
    pub async fn default_annotation_export_folder(&self) -> Result<Option<String>,BackendError> {
        self.request(AppCommand::DefaultAnnotationExportFolder).await
    }
    pub async fn set_default_annotation_export_folder(&self,path:String) -> Result<String,BackendError> {
        self.request(AppCommand::SetDefaultAnnotationExportFolder {path}).await
    }

    pub async fn default_library_save_location(&self) -> Result<Option<String>, BackendError> {
        self.request(AppCommand::DefaultLibrarySaveLocation).await
    }
    pub async fn set_default_library_save_location(&self, path: String) -> Result<String, BackendError> {
        self.request(AppCommand::SetDefaultLibrarySaveLocation { path }).await
    }

    pub async fn update_browsing_preference(&self, patch: app_preferences::BrowsingPreferencePatch) -> Result<app_preferences::BrowsingPreferences, BackendError> {
        self.request(AppCommand::UpdateBrowsingPreference { patch }).await
    }

    pub async fn update_reader_preferences(&self, patches: Vec<app_preferences::ReaderPreferencePatch>) -> Result<(), BackendError> {
        self.request(AppCommand::UpdateReaderPreferences { patches }).await
    }
}
