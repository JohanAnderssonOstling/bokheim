use crate::LibraryId;
use crate::api::app::AppClient;

#[derive(Clone)]
pub struct LibraryClient {
    pub(crate) app: AppClient,
    pub(crate) library_id: LibraryId,
}

impl LibraryClient {
    pub(crate) fn new(app: AppClient, library_id: LibraryId) -> Self {
        Self { app, library_id }
    }

    pub fn id(&self) -> &LibraryId {
        &self.library_id
    }

    pub fn app(&self) -> &AppClient {
        &self.app
    }

    pub(crate) async fn request<T: crate::runtime::LibraryReply, C: crate::runtime::LibraryRequest>(&self, command: C) -> Result<T, String> {
        self.app.transport.library_request(self.library_id, command).await
    }

    pub async fn updates(&self) -> Result<(async_channel::Receiver<library_model::LibraryUpdate>, bool), String> {
        self.app.transport.library_updates_transport(self.library_id).await
    }

    pub async fn download_changes(&self, content_hash: crate::ContentHash) -> Result<(async_channel::Receiver<crate::DownloadState>, crate::DownloadState), String> {
        self.app.transport.book_download_changes(self.library_id, content_hash).await
    }
}
