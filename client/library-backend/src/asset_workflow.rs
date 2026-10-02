//! Database-backed placement policy.
//!
//! `library-files` owns concrete storage operations; this module owns the
//! SQLite state transitions that decide which operations are required.
use crate::{BackendError, ContentHash};
use library_database::Database;
use library_files::asset_store::{CopiedBook, ImportOptions, PublishedStagedBook, StagedBook};
use library_files::AssetStore;
#[cfg(target_arch = "wasm32")]
use library_model::BlobKind;
use sync_common::DirId;

pub(crate) fn evict_local_book(assets: &AssetStore, database: &Database, hash: &ContentHash) -> Result<(), BackendError> {
    #[cfg(not(target_arch = "wasm32"))]
    if database.is_book_downloaded(hash)? {
        let paths = database.book_paths(hash)?;
        assets.evict_book_files(&paths, hash)?;
    }
    if !database.evict_local_book(hash)? {
        return Ok(());
    }
    #[cfg(target_arch = "wasm32")]
    assets.remove(BlobKind::Book, hash)?;
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
mod native;

/// Replay physical placement work, then finish permanent-purge cleanup.
pub async fn run_file_jobs(assets: &AssetStore, database: &Database) -> Result<(), BackendError> {
    let mut failure = None;
    #[cfg(not(target_arch = "wasm32"))]
    if let Err(error) = async {
        let books = database.native_book_replay_work_ids()?;
        native::replay_directories(assets, database, None).await?;
        native::reconcile_file_jobs(assets, database, database.native_file_work()?)?;
        use library_database::native::NativeFileWorkSql;
        database.connection.native_complete_book_replay(&serde_json::to_string(&books).map_err(BackendError::operation)?)?;
        Ok::<_, BackendError>(())
    }
    .await
    {
        failure = Some(error);
    }
    let mut after = String::new();
    loop {
        let jobs = database.requested_purge_books(&after)?;
        let Some(last) = jobs.last() else { break };
        after = last.to_string();
        for hash in jobs {
            let result: Result<(), BackendError> = (|| {
                #[cfg(not(target_arch = "wasm32"))]
                if !assets.root_available() {
                    return Err(BackendError::message("library storage is unavailable"));
                }
                let Some(_lease) = assets.try_exclusive_lease(&hash)? else { return Ok(()) };
                let Some(snapshot) = database.purge_book_snapshot(&hash)? else { return Ok(()) };
                if snapshot.remove_bytes {
                    #[cfg(target_arch = "wasm32")]
                    assets.remove(BlobKind::Book, &hash)?;
                    assets.remove(library_model::BlobKind::Thumbnail, &hash)?;
                }
                database.acknowledge_purge_book(snapshot)?;
                Ok(())
            })();
            if let Err(error) = result {
                failure.get_or_insert(error);
            }
        }
    }
    failure.map_or(Ok(()), Err)
}

pub async fn run_directory_work(assets: &AssetStore, database: &Database) -> Result<(), BackendError> {
    #[cfg(not(target_arch = "wasm32"))]
    return native::replay_directories(assets, database, None).await;
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (assets, database);
        Ok(())
    }
}

pub async fn run_file_jobs_for(assets: &AssetStore, database: &Database, hash: &ContentHash) -> Result<(), BackendError> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let jobs = database.native_file_work()?.into_iter().filter(|work| &work.content_hash == hash).collect();
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
    native::replay_directories(assets, database, Some(id)).await
}

#[cfg(not(target_arch = "wasm32"))]
async fn import_path(assets: &AssetStore, database: &Database, parent: DirId, name: &str) -> Result<library_model::RelativeBookPath, BackendError> {
    prepare_directory(assets, database, parent).await?;
    database.native_book_path(parent, name)?.ok_or_else(|| BackendError::message("prepared directory no longer exists"))
}

/// Resolve placement policy before handing raw import bytes to storage.
pub async fn stage_import(assets: &AssetStore, database: &Database, parent: DirId, name: String, source: Box<dyn std::io::Read + Send>, options: ImportOptions) -> Result<(String, CopiedBook), BackendError> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let relative = import_path(assets, database, parent, &name).await?;
        let occupied = database.shared_occupied_file_names(&parent)?;
        Ok(assets.copy_import(relative, source, occupied, options).await?)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (database, parent);
        Ok(assets.copy_import(library_model::RelativeBookPath::parse(&name).map_err(BackendError::operation)?, source, Vec::new(), options).await?)
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
