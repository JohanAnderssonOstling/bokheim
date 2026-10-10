//! Replays logical folder renames and moves onto the library folder.
use client_runtime::BackendError;
use library_files::AssetStore;
use library_database::{Database, NativeDirectoryAcknowledgement, NativeDirectoryCompletion};

/// Runs after a successful sync pass, on the library thread. Every step
/// re-reads the database, so a folder changed meanwhile is retried or skipped.
pub fn replay(database: &Database, assets: &AssetStore) -> Result<(), BackendError> {
    // Plan: every changed folder plus the folders of pending book work, each
    // preceded by its ancestors.
    let work = database.native_directory_replay_work()?;
    let mut directories: Vec<_> = work.iter().map(|(_, id)| *id).collect();
    directories.extend(database.native_book_work_directory_ids()?);
    directories.sort();
    directories.dedup();
    let mut ordered = Vec::new();
    for id in directories {
        for component in database.dir_path_components(&id)? {
            if !ordered.contains(&component.id) {
                ordered.push(component.id);
            }
        }
    }
    let mut recovery = ordered.clone();
    for (_, id) in &work {
        if !recovery.contains(id) {
            recovery.push(*id);
        }
    }

    // Stage all changing folders before materializing destinations (including swaps).
    for id in recovery {
        let snapshot = database.native_directory_snapshot(&id)?;
        let Some(old) = database.native_directory_projected_path(&id)? else { continue };
        if let Some(staged) = assets.stage_directory(id, &old, snapshot.as_ref().map(|s| s.relative_path.as_str()))? {
            database.acknowledge_native_directory_staging(&id, &old, &staged)?;
        }
    }
    for id in ordered {
        if id == sync_common::ROOT_DIR_ID {
            continue;
        }
        if !assets.root_available() {
            return Err(BackendError::message("library storage is unavailable"));
        }
        loop {
            let Some(snapshot) = database.native_directory_snapshot(&id)? else { break };
            let prepared = assets.inspect_directory(snapshot.id, &snapshot.relative_path, snapshot.projected_path.as_deref(), &snapshot.name, &snapshot.sibling_names)?;
            let Some(result) = prepared.publish()? else { continue };
            let completion = NativeDirectoryCompletion { snapshot, renamed_to: result.renamed_to, materialized: result.materialized };
            match database.acknowledge_native_directory(completion)? {
                NativeDirectoryAcknowledgement::Applied | NativeDirectoryAcknowledgement::Obsolete => break,
                NativeDirectoryAcknowledgement::Retry => continue,
            }
        }
    }
    let ids: Vec<i64> = work.into_iter().map(|(id, _)| id).collect();
    database.complete_directory_replay(&ids)?;
    Ok(())
}
