//! Import storage lifecycle. Browser objects are committed asynchronously before
//! the metadata transaction; native files are published against its placements.
use super::{AssetStore, AssetStoreError, StagedBook};
use crate::{ContentHash, DirId};
use crate::database::FileDatabase;

impl AssetStore {
    pub async fn stage_import<D: FileDatabase>(&self, db: &D, parent: DirId, name: String, source: Box<dyn std::io::Read + Send>) -> Result<StagedBook, AssetStoreError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let relative = self.prepare_import_path(db, parent, name).await?;
            self.stage_publication(relative, source).await
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (db, parent, name);
            self.stage_reader(source).await
        }
    }
}

/// Verified storage ready to participate in the caller's metadata transaction.
/// The caller must hold its content lease through preparation and publication.
pub struct PreparedImport {
    #[cfg(not(target_arch = "wasm32"))]
    staged: StagedBook,
}

impl StagedBook {
    pub async fn prepare_import(self) -> Result<PreparedImport, AssetStoreError> {
        #[cfg(not(target_arch = "wasm32"))]
        { Ok(PreparedImport { staged: self }) }
        #[cfg(target_arch = "wasm32")]
        {
            self.commit().await?;
            Ok(PreparedImport {})
        }
    }
}

impl PreparedImport {
    pub fn prepare_destination(&self, store: &AssetStore, conn: &mut rusqlite::Connection, parent: DirId) -> Result<(), AssetStoreError> {
        #[cfg(not(target_arch = "wasm32"))]
        { store.placement().prepare_directory(conn, parent) }
        #[cfg(target_arch = "wasm32")]
        { let _ = (store, conn, parent); Ok(()) }
    }

    /// Returns whether imported bytes became the placement's readable content.
    pub fn publish(self, store: &AssetStore, conn: &rusqlite::Connection, parent: &DirId) -> Result<bool, AssetStoreError> {
        #[cfg(not(target_arch = "wasm32"))]
        { self.staged.publish(store, conn, parent) }
        #[cfg(target_arch = "wasm32")]
        { let _ = (store, conn, parent); Ok(true) }
    }
}

impl AssetStore {
    pub fn reconcile_import_placements(&self, conn: &rusqlite::Connection, hash: &ContentHash) -> Result<(), AssetStoreError> {
        #[cfg(not(target_arch = "wasm32"))]
        { self.placement().queue_book_placements(conn, hash) }
        #[cfg(target_arch = "wasm32")]
        { let _ = (conn, hash); Ok(()) }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl AssetStore {
    pub async fn prepare_import_path<D: FileDatabase>(&self, db: &D, parent: DirId, name: String) -> Result<String, AssetStoreError> {
        let store = self.clone();
        let db = db.clone();
        crate::executor::run_blocking(move || {
            let mut conn = db.get().map_err(|error| AssetStoreError(error.to_string()))?;
            store.placement().prepare_directory(&mut conn, parent)?;
            client_library::file_work::placement_path(&conn, &parent, &name).map_err(AssetStoreError::from)
        }).await.map_err(|error| AssetStoreError(error.to_string()))?
    }
}
