//! Adding a book downloaded over the network: a public-domain catalog book,
//! checked against the catalog's hash and with the catalog's record as its
//! enrichment answer, or a book from an OPDS feed, enriched like any import.

use client_runtime::BackendError;
use library_placement::stage_import;
use sync_common::{ContentHash, DirId};

use crate::LibraryImport;

impl LibraryImport {
    pub async fn import_catalog_book(&self, parent_id: DirId, hash: ContentHash) -> Result<ContentHash, BackendError> {
        let book = library_database::catalog::books().iter().find(|book| book.hash == hash).ok_or("the book is not in the catalog")?;
        let bytes = library_catalog::download_book(hash).await.map_err(BackendError::message)?;
        let name = if book.author.is_empty() { book.title.clone() } else { format!("{} - {}", book.author, book.title) };
        let file_name = format!("{}.epub", name.replace(['/', '\\', ':'], " ").trim());
        let imported = self.import_downloaded_book(parent_id, &file_name, bytes).await?;
        self.database.record_catalog_answer(&imported, book.record.clone()).map_err(BackendError::operation)?;
        Ok(imported)
    }

    /// Imports a downloaded file as if it were a local one.
    pub async fn import_downloaded_book(&self, parent_id: DirId, file_name: &str, bytes: Vec<u8>) -> Result<ContentHash, BackendError> {
        let mut import = self.validate_import(&parent_id, file_name)?;
        let (name, staged) = stage_import(&self.assets, &self.database, parent_id, import.file_name.clone(), Box::new(std::io::Cursor::new(bytes)), Default::default()).await.map_err(BackendError::operation)?;
        import.file_name = name;
        self.finish_import(import, staged).await
    }
}
