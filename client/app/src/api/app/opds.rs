use super::AppClient;
use crate::runtime::AppCommand;
use library_backend::LibraryBrowseData;
use library_model::{CardKey, LibraryBrowseQuery, OpdsCredentials, OpdsSource, OpdsSourceId};

/// The account's OPDS feeds. None in the browser.
impl AppClient {
    pub async fn opds_feeds(&self) -> Result<Vec<OpdsSource>, String> {
        self.request(AppCommand::OpdsFeeds).await
    }

    /// Adds the feed at `url`, named by its own title unless `name` is given.
    pub async fn add_opds_feed(&self, url: String, name: Option<String>, credentials: Option<OpdsCredentials>) -> Result<(), String> {
        self.request(AppCommand::AddOpdsFeed { url, name, credentials }).await
    }

    pub async fn remove_opds_feed(&self, id: OpdsSourceId) -> Result<(), String> {
        self.request(AppCommand::RemoveOpdsFeed { id }).await
    }

    /// Keeps `credentials` for a feed on this device; they are not synced.
    pub async fn set_opds_credentials(&self, id: OpdsSourceId, credentials: OpdsCredentials) -> Result<(), String> {
        self.request(AppCommand::SetOpdsCredentials { id, credentials }).await
    }

    pub async fn browse_opds(&self, query: LibraryBrowseQuery) -> Result<LibraryBrowseData, String> {
        self.request(AppCommand::BrowseOpds { query }).await
    }

    /// A listed feed book's cover, as a JPEG `width` pixels wide.
    pub async fn opds_cover(&self, source: OpdsSourceId, entry: CardKey, width: u32) -> Result<Option<Vec<u8>>, String> {
        let bytes: Option<serde_bytes::ByteBuf> = self.request(AppCommand::OpdsCover { source, entry, width }).await?;
        Ok(bytes.map(serde_bytes::ByteBuf::into_vec))
    }

    /// The feed's description of a listed book, as HTML.
    pub async fn opds_description(&self, entry: CardKey) -> Result<String, String> {
        self.request(AppCommand::OpdsDescription { entry }).await
    }

    /// A listed feed book's file and the name to import it under.
    pub async fn download_opds_book(&self, source: OpdsSourceId, entry: CardKey) -> Result<(String, Vec<u8>), String> {
        let (file_name, bytes): (String, serde_bytes::ByteBuf) = self.request(AppCommand::DownloadOpdsBook { source, entry }).await?;
        Ok((file_name, bytes.into_vec()))
    }
}
