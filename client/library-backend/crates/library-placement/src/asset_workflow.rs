//! Database-backed placement policy.
//!
//! `library-files` owns concrete storage operations; this module owns the
//! SQLite state transitions that decide which operations are required.
use client_runtime::BackendError;
use library_database::Database;
use library_files::asset_store::{CopiedBook, ImportOptions, PublishedStagedBook, StagedBook};
use library_files::AssetStore;
#[cfg(target_arch = "wasm32")]
use library_model::BlobKind;
use sync_common::BookId;
use sync_common::DirId;

/// Remove local bytes before acknowledging eviction, so storage failures leave
/// availability intact and the same operation can be retried.
pub async fn evict_local_book(assets: &AssetStore, database: &Database, hash: &BookId) -> Result<bool, BackendError> {
    // Refuse eviction before touching the files that an unfinished upload needs.
    if !database.check_local_book_eviction(hash)? {
        return Ok(false);
    }
    assets.evict_book_files(*hash, database.book_paths(hash)?).await?;
    database.evict_local_book(hash).map_err(BackendError::operation)
}

#[cfg(not(target_arch = "wasm32"))]
mod native;

/// Replay physical placement work, then finish permanent-purge cleanup.
pub async fn run_file_jobs(assets: &AssetStore, database: &Database) -> Result<(), BackendError> {
    let mut failure = None;
    #[cfg(not(target_arch = "wasm32"))]
    if let Err(error) = (|| {
        let books = database.native_book_replay_work_ids()?;
        replay_directories(database, assets)?;
        native::reconcile_file_jobs(assets, database, database.native_file_work()?)?;
        database.complete_book_replay(&books)?;
        Ok::<_, BackendError>(())
    })() {
        failure = Some(error);
    }
    let mut after = None;
    loop {
        let jobs = database.requested_purge_books(after.as_ref())?;
        let Some(last) = jobs.last() else { break };
        after = Some(*last);
        for hash in jobs {
            if let Err(error) = purge_book(assets, database, &hash) {
                failure.get_or_insert(error);
            }
        }
    }
    failure.map_or(Ok(()), Err)
}

/// Removes a purged book's bytes and thumbnails, then acknowledges the purge.
/// Does nothing while another operation holds the book.
fn purge_book(assets: &AssetStore, database: &Database, hash: &BookId) -> Result<(), BackendError> {
    #[cfg(not(target_arch = "wasm32"))]
    if !assets.root_available() {
        return Err(BackendError::message("library storage is unavailable"));
    }
    let Some(_lease) = assets.try_exclusive_lease(hash)? else { return Ok(()) };
    let Some(snapshot) = database.purge_book_snapshot(hash)? else { return Ok(()) };
    if snapshot.remove_bytes {
        #[cfg(target_arch = "wasm32")]
        assets.remove(BlobKind::Book, hash)?;
        assets.remove(library_model::BlobKind::Thumbnail, hash)?;
    }
    database.acknowledge_purge_book(snapshot)?;
    Ok(())
}

pub async fn run_file_jobs_for(assets: &AssetStore, database: &Database, hash: &BookId) -> Result<(), BackendError> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let jobs = database.native_file_work()?.into_iter().filter(|work| &work.book_id == hash).collect();
        native::reconcile_file_jobs(assets, database, jobs)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (assets, database, hash);
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn prepare_directory(assets: &AssetStore, database: &Database, id: DirId) -> Result<(), BackendError> {
    prepare_directories(assets, database, &[id]).await
}

/// Recover a set of destinations together, visiting shared ancestors once.
#[cfg(not(target_arch = "wasm32"))]
pub async fn prepare_directories(assets: &AssetStore, database: &Database, ids: &[DirId]) -> Result<(), BackendError> {
    if ids.is_empty() {
        return Ok(());
    }
    native::replay_directories(assets, database, Some(ids))
}

/// Replay all pending folder changes after sync. Each step re-reads the
/// database, so a folder changed meanwhile is retried or skipped.
#[cfg(not(target_arch = "wasm32"))]
pub fn replay_directories(database: &Database, assets: &AssetStore) -> Result<(), BackendError> {
    native::replay_directories(assets, database, None)
}

#[cfg(not(target_arch = "wasm32"))]
async fn import_path(assets: &AssetStore, database: &Database, parent: DirId, name: &str) -> Result<library_model::RelativeBookPath, BackendError> {
    prepare_directory(assets, database, parent).await?;
    database.native_book_path(parent, name)?.ok_or_else(|| BackendError::message("prepared directory no longer exists"))
}

/// Resolve placement policy before handing raw import bytes to storage.
pub async fn stage_import(assets: &AssetStore, database: &Database, parent: DirId, name: &str, source: Box<dyn std::io::Read + Send>, options: ImportOptions) -> Result<(String, CopiedBook), BackendError> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let relative = import_path(assets, database, parent, name).await?;
        let occupied = database.shared_occupied_file_names(&parent)?;
        Ok(assets.copy_import(relative, source, occupied, options).await?)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (database, parent);
        Ok(assets.copy_import(library_model::RelativeBookPath::parse(name).map_err(BackendError::operation)?, source, Vec::new(), options).await?)
    }
}

/// Resolve the destination again after inspection, then publish the bytes.
pub async fn publish_import(assets: &AssetStore, database: &Database, parent: DirId, name: &str, staged: StagedBook, audiobook_archive: bool) -> Result<PublishedStagedBook, BackendError> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let relative = import_path(assets, database, parent, name).await?;
        let occupied = database.shared_occupied_file_names(&parent)?;
        Ok(staged.publish_snapshot(assets, &relative, &occupied, audiobook_archive)?)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (assets, database, parent, audiobook_archive);
        staged.commit().await?;
        Ok(PublishedStagedBook { name: name.to_owned(), relative_path: String::new(), published: true, fingerprint: None })
    }
}
