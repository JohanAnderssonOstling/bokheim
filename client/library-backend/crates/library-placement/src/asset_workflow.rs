//! Database-backed placement policy.
//!
//! `library-files` owns concrete storage operations; this module owns the
//! SQLite state transitions that decide which operations are required.
use crate::LibraryState;
use client_protocol::BackendError;
use library_database::DatabaseClient;
use library_files::asset_store::{CopiedBook, ImportOptions, PublishedStagedBook, StagedBook};
use library_files::AssetStore;
use library_model::RelativeBookPath;
use sync_common::BookId;
use sync_common::DirId;
use tokio::sync::RwLock;

/// Remove local bytes before acknowledging eviction, so storage failures leave
/// availability intact and the same operation can be retried.
pub async fn evict_local_book(assets: &AssetStore, database: &DatabaseClient, hash: &BookId) -> Result<bool, BackendError> {
    // Refuse eviction before touching the files that an unfinished upload needs.
    if !database.check_local_book_eviction(hash).await? {
        return Ok(false);
    }
    let lease = assets.try_exclusive_lease(hash)?.ok_or("book is currently in use; close it and retry removing the download")?;
    // Deletion returns the lease so it also covers the metadata update.
    let _lease = assets.evict_book_files(*hash, database.book_paths(hash).await?, lease).await?;
    database.evict_local_book(hash).await.map_err(BackendError::operation)
}

#[cfg(not(target_arch = "wasm32"))]
mod native;

/// Replay queued work, releasing the library gate between file changes.
/// Acquires the gate internally; the caller must not already hold it.
pub async fn run_file_jobs(state: &RwLock<LibraryState>) -> Result<(), BackendError> {
    let mut failure = None;
    let database = state.read().await.database().clone();
    #[cfg(not(target_arch = "wasm32"))]
    {
        native::replay_background_directories(state).await?;
        let books = database.native_book_replay_work_ids().await?;
        let jobs = database.native_file_work().await?;
        if let Err(error) = native::reconcile_background_file_jobs(state, jobs).await {
            failure.get_or_insert(error);
        }
        database.complete_book_replay(&books).await?;
    }
    let mut after = None;
    loop {
        let jobs = database.requested_purge_books(after.as_ref()).await?;
        let Some(last) = jobs.last() else { break };
        after = Some(*last);
        for hash in jobs {
            let storage = state.read().await.clone();
            if let Err(error) = purge_book(storage.assets(), storage.database(), &hash).await {
                failure.get_or_insert(error);
            }
            drop(storage);
            tokio::task::yield_now().await;
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    if let Err(error) = native::purge_background_directories(state).await {
        failure.get_or_insert(error);
    }
    failure.map_or(Ok(()), Err)
}

/// Removes a purged book's bytes and thumbnails, then acknowledges the purge.
/// Does nothing while another operation holds the book.
async fn purge_book(assets: &AssetStore, database: &DatabaseClient, hash: &BookId) -> Result<(), BackendError> {
    #[cfg(not(target_arch = "wasm32"))]
    if !assets.root_available() {
        return Err(BackendError::message("library storage is unavailable"));
    }
    let Some(lease) = assets.try_exclusive_lease(hash)? else { return Ok(()) };
    let Some(snapshot) = database.purge_book_snapshot(hash).await? else { return Ok(()) };
    let _lease = if snapshot.remove_bytes { assets.purge_book_assets(*hash, lease).await? } else { lease };
    database.acknowledge_purge_book(snapshot).await?;
    Ok(())
}

/// Repair one book against the caller's storage snapshot, for example before opening it.
pub async fn run_file_jobs_for(assets: &AssetStore, database: &DatabaseClient, hash: &BookId) -> Result<(), BackendError> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let jobs = database.native_file_work().await?.into_iter().filter(|work| &work.book_id == hash).collect();
        native::reconcile_file_jobs(assets, database, jobs).await
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (assets, database, hash);
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn prepare_directory(assets: &AssetStore, database: &DatabaseClient, id: DirId) -> Result<(), BackendError> {
    prepare_directories(assets, database, &[id]).await
}

/// Recover a set of destinations together, visiting shared ancestors once.
#[cfg(not(target_arch = "wasm32"))]
pub async fn prepare_directories(assets: &AssetStore, database: &DatabaseClient, ids: &[DirId]) -> Result<(), BackendError> {
    if ids.is_empty() {
        return Ok(());
    }
    native::replay_directories(assets, database, Some(ids)).await
}

/// Replay all pending folder changes after sync. Each step re-reads the
/// database, so a folder changed meanwhile is retried or skipped.
#[cfg(not(target_arch = "wasm32"))]
pub async fn replay_directories(database: &DatabaseClient, assets: &AssetStore) -> Result<(), BackendError> {
    native::replay_directories(assets, database, None).await
}

#[cfg(not(target_arch = "wasm32"))]
async fn import_path(assets: &AssetStore, database: &DatabaseClient, parent: DirId, name: &str) -> Result<RelativeBookPath, BackendError> {
    prepare_directory(assets, database, parent).await?;
    database.native_book_path(parent, name).await?.ok_or_else(|| BackendError::message("prepared directory no longer exists"))
}

/// Resolve placement policy before handing raw import bytes to storage.
pub async fn stage_import(assets: &AssetStore, database: &DatabaseClient, parent: DirId, name: &str, source: Box<dyn std::io::Read + Send>, options: ImportOptions) -> Result<(String, CopiedBook), BackendError> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let relative = import_path(assets, database, parent, name).await?;
        let occupied = database.shared_occupied_file_names(&parent).await?;
        Ok(assets.copy_import(relative, source, occupied, options).await?)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (database, parent);
        Ok(assets.copy_import(RelativeBookPath::parse(name).map_err(BackendError::operation)?, source, Vec::new(), options).await?)
    }
}

/// Resolve the destination again after inspection, then publish the bytes.
pub async fn publish_import(assets: &AssetStore, database: &DatabaseClient, parent: DirId, name: &str, staged: StagedBook, audiobook_archive: bool) -> Result<PublishedStagedBook, BackendError> {
    #[cfg(not(target_arch = "wasm32"))]
    let (relative, occupied) = (import_path(assets, database, parent, name).await?, database.shared_occupied_file_names(&parent).await?);
    #[cfg(target_arch = "wasm32")]
    let (relative, occupied) = {
        let _ = (database, parent);
        (RelativeBookPath::parse(name).map_err(BackendError::operation)?, Vec::new())
    };
    Ok(assets.publish_import(staged, relative, occupied, audiobook_archive).await?)
}
