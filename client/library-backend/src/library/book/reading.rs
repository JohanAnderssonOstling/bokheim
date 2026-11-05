//! Reader preparation, book identity, playback metadata, and thumbnail access.

use crate::asset_store::AssetStore;
use crate::library::LibraryHandle;
#[cfg(target_arch = "wasm32")]
use crate::library::{browser_audiobook, PlaybackSource};
use crate::{BackendError, BookId, ThumbnailResolution};
use library_model::BlobKind;

use super::ResolvedBook;

impl LibraryHandle {
    pub async fn book_format(&self, book_id: BookId) -> Result<Option<book_model::BookFormat>, BackendError> {
        let database = self.database();
        Ok(database.book_format(&book_id).await?)
    }

    pub async fn audiobook_playback_metadata(&self, book_id: BookId) -> Result<Option<library_model::PlaybackMetadata>, BackendError> {
        let database = self.database();
        Ok(database.audiobook_playback_metadata(&book_id).await?)
    }

    pub async fn resolve_book_identity(&self, book_id: BookId) -> Result<BookId, BackendError> {
        if book_id.content_hash().is_none() {
            return Ok(book_id);
        }
        let database = self.database();
        Ok(database.resolve_book_identity(book_id).await?)
    }

    pub async fn thumbnail(&self, book_id: BookId, resolution: ThumbnailResolution) -> Result<Option<Vec<u8>>, BackendError> {
        self.run_io(async {
            let storage = self.storage().await?;
            thumbnail(storage.assets(), book_id, resolution).await
        })
        .await
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn open_remote_audio(&self, book_id: BookId) -> Result<Option<PlaybackSource>, BackendError> {
        self.run_io(async {
            let library = &self.owner.resources;
            let storage = self.storage().await?;
            let client = library.sync.http_client();
            let account = &library.account;
            let source = remote_book_access::direct_audio_source(client, account, library.id, book_id);
            browser_audiobook::open_remote_audio(storage.database(), storage.assets(), book_id, source).await
        })
        .await
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn resolve_book(&self, book_id: BookId) -> Result<ResolvedBook, BackendError> {
        self.run_io(async {
            let library = &self.owner.resources;
            let storage = self.storage().await?;
            library.prepare_book_for_reader(&storage, book_id).await
        })
        .await
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn resolve_book(&self, book_id: BookId) -> Result<ResolvedBook, BackendError> {
        super::web_reader::resolve_book(self, book_id).await
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn resolve_audiobook_track(&self, book_id: BookId, index: usize) -> Result<ResolvedBook, BackendError> {
        self.run_io(async {
            let library = &self.owner.resources;
            let storage = self.storage().await?;
            library.prepare_audiobook_track(&storage, book_id, index).await
        })
        .await
    }
}

pub(in crate::library) async fn thumbnail(assets: &AssetStore, book_id: BookId, resolution: ThumbnailResolution) -> Result<Option<Vec<u8>>, BackendError> {
    if matches!(resolution, ThumbnailResolution::Browse | ThumbnailResolution::CardHighDensity) {
        if let Some(bytes) = assets.read_named(library_files::asset_store::thumbnail_variant_name(&book_id, resolution.width())).await.map_err(BackendError::operation)? {
            return Ok(Some(bytes));
        }
    }
    // Existing libraries may only have the hash-addressed master. Load it
    // directly rather than hiding the cover until a card variant is rebuilt.
    if let Some(bytes) = assets.read_bytes(BlobKind::Thumbnail, &book_id).await.map_err(BackendError::operation)? {
        return Ok(Some(bytes));
    }

    // The origin also addresses cover images directly by hash.
    Ok(match resolution {
        ThumbnailResolution::HighDensity => library_catalog::cover(book_id, true).await,
        ThumbnailResolution::Browse | ThumbnailResolution::CardHighDensity => library_catalog::card_cover(library_model::CardKey::from(book_id), resolution.width()).await,
    })
}
