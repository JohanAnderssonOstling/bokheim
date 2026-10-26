use super::AppClient;
use crate::runtime::AppCommand;
use crate::BackendError;
use library_backend::LibraryBrowseData;
use library_model::{AddOpdsFeedResult, CardKey, LibraryBrowseQuery, OpdsCredentials, OpdsSource, OpdsSourceId, Store, StoreId};
use sync_common::LibraryId;

/// The account's OPDS feeds. None in the browser.
impl AppClient {
    /// Release the backend's retained catalog/feed cover JPEGs before reading.
    pub async fn evict_cover_caches(&self) -> Result<(), BackendError> {
        self.request(AppCommand::EvictCoverCaches).await
    }

    pub async fn opds_feeds(&self) -> Result<Vec<OpdsSource>, BackendError> {
        self.request(AppCommand::OpdsFeeds).await
    }

    /// Adds the feed at `url`, named by its own title unless `name` is given.
    /// An anonymous feed that requires authentication asks for credentials first.
    pub async fn add_opds_feed(&self, url: String, name: Option<String>, credentials: Option<OpdsCredentials>) -> Result<AddOpdsFeedResult, BackendError> {
        self.request(AppCommand::AddOpdsFeed { url, name, credentials }).await
    }

    pub async fn remove_opds_feed(&self, id: OpdsSourceId) -> Result<(), BackendError> {
        self.request(AppCommand::RemoveOpdsFeed { id }).await
    }

    /// Keeps `credentials` for a feed on this device; they are not synced.
    pub async fn set_opds_credentials(&self, id: OpdsSourceId, credentials: OpdsCredentials) -> Result<(), BackendError> {
        self.request(AppCommand::SetOpdsCredentials { id, credentials }).await
    }

    pub async fn browse_opds(&self, query: LibraryBrowseQuery) -> Result<LibraryBrowseData, BackendError> {
        self.request(AppCommand::BrowseOpds { query }).await
    }

    /// A listed feed book's cover, as a JPEG `width` pixels wide.
    pub async fn opds_cover(&self, source: OpdsSourceId, entry: CardKey, width: u32) -> Result<Option<Vec<u8>>, BackendError> {
        let bytes: Option<serde_bytes::ByteBuf> = self.request(AppCommand::OpdsCover { source, entry, width, card: false }).await?;
        Ok(bytes.map(serde_bytes::ByteBuf::into_vec))
    }

    pub async fn opds_card_cover(&self, source: OpdsSourceId, entry: CardKey, width: u32) -> Result<Option<Vec<u8>>, BackendError> {
        let bytes: Option<serde_bytes::ByteBuf> = self.request(AppCommand::OpdsCover { source, entry, width, card: true }).await?;
        Ok(bytes.map(serde_bytes::ByteBuf::into_vec))
    }

    /// The feed's description of a listed book, as HTML.
    pub async fn opds_description(&self, entry: CardKey) -> Result<String, BackendError> {
        self.request(AppCommand::OpdsDescription { entry }).await
    }

    /// A listed feed book's file and the name to import it under.
    pub async fn download_opds_book(&self, source: OpdsSourceId, entry: CardKey) -> Result<(String, Vec<u8>), BackendError> {
        let (file_name, bytes): (String, serde_bytes::ByteBuf) = self.request(AppCommand::DownloadOpdsBook { source, entry }).await?;
        Ok((file_name, bytes.into_vec()))
    }

    /// Opens the store page that sells a listed feed book; what is bought
    /// there goes into `library_id`'s root.
    pub async fn buy_opds_book(&self, entry: CardKey, library_id: LibraryId) -> Result<(), BackendError> {
        self.request(AppCommand::BuyOpdsBook { entry, library_id }).await
    }
}

/// The stores added on this device. None in the browser.
impl AppClient {
    /// Whether a store window can open here: the system has the webview it
    /// needs, such as WebKitGTK on Linux or WebView2 on Windows.
    pub async fn stores_available(&self) -> Result<bool, BackendError> {
        self.request(AppCommand::StoresAvailable).await
    }

    pub async fn stores(&self) -> Result<Vec<Store>, BackendError> {
        self.request(AppCommand::Stores).await
    }

    /// Adds the store at `url`, named by its host unless `name` is given.
    pub async fn add_store(&self, url: String, name: Option<String>) -> Result<(), BackendError> {
        self.request(AppCommand::AddStore { url, name }).await
    }

    pub async fn remove_store(&self, id: StoreId) -> Result<(), BackendError> {
        self.request(AppCommand::RemoveStore { id }).await
    }

    /// Opens the store's window; what is bought there goes into
    /// `library_id`'s root.
    pub async fn open_store(&self, id: StoreId, library_id: LibraryId) -> Result<(), BackendError> {
        self.request(AppCommand::OpenStore { id, library_id }).await
    }

    /// Opens a browser with its address bar ready to type into.
    pub async fn open_browser(&self, library_id: LibraryId) -> Result<(), BackendError> {
        self.request(AppCommand::OpenBrowser { library_id }).await
    }
}
