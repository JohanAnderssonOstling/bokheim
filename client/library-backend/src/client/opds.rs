use super::LibraryClient;
#[cfg(not(target_arch = "wasm32"))]
use crate::library::{LibraryCommand, PendingCommand, ReadyCommand};
#[cfg(not(target_arch = "wasm32"))]
use crate::BookId;
use crate::{BackendError, LibraryBrowseData, ThumbnailResolution};
use library_model::{CardKey, LibraryBrowseQuery};

impl LibraryClient {
    /// Fetch a page from this library's OPDS feed and retain its book metadata.
    pub async fn browse_opds(&self, query: LibraryBrowseQuery) -> Result<LibraryBrowseData, BackendError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.request(LibraryCommand::Pending(PendingCommand::BrowseOpds { query })).await
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = query;
            Err("OPDS feeds are not available in the browser".into())
        }
    }

    /// Resolution selects cropped card art or the unmodified detail cover.
    pub async fn opds_cover(&self, entry: CardKey, resolution: ThumbnailResolution) -> Result<Option<Vec<u8>>, BackendError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let bytes: Option<serde_bytes::ByteBuf> = self.request(LibraryCommand::Pending(PendingCommand::OpdsCover { entry, width: resolution.width(), card: resolution != ThumbnailResolution::HighDensity })).await?;
            Ok(bytes.map(serde_bytes::ByteBuf::into_vec))
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (entry, resolution);
            Err("OPDS feeds are not available in the browser".into())
        }
    }

    /// Download a listed book from this feed for import into another library.
    pub async fn download_opds_book(&self, entry: CardKey) -> Result<(String, Vec<u8>), BackendError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (name, bytes): (String, serde_bytes::ByteBuf) = self.request(LibraryCommand::Pending(PendingCommand::DownloadOpdsBook { entry })).await?;
            Ok((name, bytes.into_vec()))
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = entry;
            Err("OPDS feeds are not available in the browser".into())
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn opds_book_states(&self, books: Vec<BookId>) -> Result<Vec<(BookId, f32, bool)>, BackendError> {
        self.request(LibraryCommand::Ready(ReadyCommand::OpdsBookStates { books })).await
    }
}
