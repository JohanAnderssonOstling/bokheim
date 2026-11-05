//! Database policy for native file replay and interrupted directory moves.

use super::{AssetStore, BackendError, BookId, DatabaseClient, DirId};
use crate::{crosses_devices, LibraryState};
use library_database::{executor::run_blocking, DatabaseError, NativeFileWork, NativeFileWorkCompletion, NativeFileWorkSnapshot};
use library_files::asset_store::PreparedPlacement;
use library_files::library_move::{checked_path, cleanup_root_matches};
use library_model::RelativeBookPath;
use std::collections::{HashMap, HashSet};
use std::io::ErrorKind;
use std::path::Path;
use std::sync::Arc;
use sync_common::ROOT_DIR_ID;
use tokio::sync::{OwnedRwLockReadGuard, RwLock};

/// Owned inputs for byte preparation; no database connection leaves its owner.
struct FileWorkPlan {
    snapshot: NativeFileWorkSnapshot,
    occupied: Vec<String>,
    movable: Vec<NativeFileWork>,
}

struct PreparedFileWork {
    plan: FileWorkPlan,
    output: Option<PreparedPlacement>,
    retained: Option<PreparedPlacement>,
    path: RelativeBookPath,
    renamed: Option<(String, String)>,
    cleanup_assets: Option<AssetStore>,
    root_access: Arc<OwnedRwLockReadGuard<()>>,
}

/// Reader repairs offload byte preparation, durable publication and cleanup.
/// The caller retains its snapshot while each dispatched task pins the root.
pub(super) async fn reconcile_file_jobs(assets: &AssetStore, database: &DatabaseClient, jobs: Vec<NativeFileWork>) -> Result<(), BackendError> {
    let movable = movable_sources(&jobs);
    let mut failure = None;
    for work in jobs {
        let result = async {
            let Some(plan) = plan_file_work(database, &movable, work).await? else { return Ok(()) };
            let Some(mut prepared) = prepare_file_work(assets.clone(), plan, false).await? else { return Ok(()) };
            let result = publish_file_work(assets, database, &mut prepared, false).await;
            run_blocking(move || drop(prepared)).await.map_err(BackendError::operation)?;
            result
        }
        .await;
        if let Err(error) = result {
            failure.get_or_insert(error);
        }
    }
    failure.map_or(Ok(()), Err)
}

/// The gate covers snapshots and publication. Hashing, copying and flushing
/// happen on the blocking executor with only owned filesystem inputs.
pub(super) async fn reconcile_background_file_jobs(state: &RwLock<LibraryState>, jobs: Vec<NativeFileWork>) -> Result<(), BackendError> {
    let movable = movable_sources(&jobs);
    let mut failure = None;
    for work in jobs {
        if let Err(error) = apply_background_file_work(state, &movable, work).await {
            failure.get_or_insert(error);
        }
        tokio::task::yield_now().await;
    }
    failure.map_or(Ok(()), Err)
}

async fn apply_background_file_work(state: &RwLock<LibraryState>, movable: &HashMap<BookId, Vec<NativeFileWork>>, work: NativeFileWork) -> Result<(), BackendError> {
    for copy_move in [false, true] {
        let plan = {
            let storage = state.write().await;
            plan_file_work(storage.database(), movable, work.clone()).await?
        };
        let Some(plan) = plan else { return Ok(()) };
        let assets = state.read().await.assets().clone();
        let Some(mut prepared) = prepare_file_work(assets, plan, copy_move).await? else { return Ok(()) };
        // Preparation drops its file leases before returning. Never wait for
        // the library gate while retaining a lease a foreground caller needs.
        let result = {
            let storage = state.write().await;
            publish_file_work(storage.assets(), storage.database(), &mut prepared, true).await
        };
        // A stale copy can contain an entire audiobook folder. Its temporary
        // files are removed on the blocking executor after releasing the gate.
        run_blocking(move || drop(prepared)).await.map_err(BackendError::operation)?;
        match result {
            // A short rename cannot cross filesystems. Prepare a complete copy
            // outside the gate, retaining the old source for its cleanup job.
            Err(error) if !copy_move && crosses_devices(&error) => continue,
            result => return result,
        }
    }
    unreachable!("the copy attempt returns its result")
}

fn movable_sources(jobs: &[NativeFileWork]) -> HashMap<BookId, Vec<NativeFileWork>> {
    let mut movable = HashMap::<BookId, Vec<NativeFileWork>>::new();
    for job in jobs.iter().filter(|job| job.operation == "remove") {
        if job.source_root.is_none() && job.cleanup_path.is_some() {
            movable.entry(job.book_id).or_default().push(job.clone());
        }
    }
    movable
}

/// Called with the library gate held. Capture only placement facts: reading
/// progress and other unrelated edits do not invalidate a prepared copy.
async fn plan_file_work(database: &DatabaseClient, movable: &HashMap<BookId, Vec<NativeFileWork>>, work: NativeFileWork) -> Result<Option<FileWorkPlan>, BackendError> {
    let Some(snapshot) = database.native_file_work_snapshot(work).await? else { return Ok(None) };
    if snapshot.staged_source {
        return Ok(None);
    }
    let mut occupied = Vec::new();
    let mut sources = Vec::new();
    if snapshot.work.operation == "materialize" {
        if let Some((dir, _)) = &snapshot.placement {
            occupied = database.shared_occupied_file_names(&DirId::parse_str(dir).map_err(BackendError::operation)?).await?;
        }
        for removal in movable.get(&snapshot.work.book_id).into_iter().flatten() {
            if let Some(source) = &removal.cleanup_path {
                if !snapshot.live_book_paths.contains(source) && database.native_file_work_is_current(removal).await? {
                    sources.push(removal.clone());
                }
            }
        }
    }
    occupied.sort();
    Ok(Some(FileWorkPlan { snapshot, occupied, movable: sources }))
}

async fn prepare_file_work(assets: AssetStore, plan: FileWorkPlan, copy_move: bool) -> Result<Option<PreparedFileWork>, BackendError> {
    let root_access = Arc::new(assets.lock_root_shared().await);
    run_blocking(move || prepare_file_work_blocking(&assets, plan, copy_move, root_access)).await.map_err(BackendError::operation)?
}

/// Byte preparation owns its leases locally and never reads library state.
fn prepare_file_work_blocking(assets: &AssetStore, plan: FileWorkPlan, copy_move: bool, root_access: Arc<OwnedRwLockReadGuard<()>>) -> Result<Option<PreparedFileWork>, BackendError> {
    if !assets.root_available() {
        return Err(BackendError::message("library storage is unavailable"));
    }
    let snapshot = &plan.snapshot;
    let hash = snapshot.work.book_id;
    let Some(_lease) = assets.try_exclusive_lease(&hash)? else { return Ok(None) };
    let historical_root = snapshot.work.source_root.as_deref().filter(|root| Path::new(root) != assets.native_library_root());
    let cleanup_absent = if let Some(root) = historical_root {
        match checked_path(Path::new(root), Path::new(snapshot.resolved_path.as_str().trim_start_matches('/'))) {
            Ok(path) => !path.try_exists()?,
            Err(error) if error.kind() == ErrorKind::NotFound => true,
            Err(error) => return Err(error.into()),
        }
    } else {
        false
    };
    let cleanup_obsolete = !cleanup_absent && !cleanup_owner_valid(&snapshot.work)?;
    let cleanup_assets = historical_root.filter(|_| !cleanup_absent && !cleanup_obsolete).map(AssetStore::open).transpose()?;
    let cleanup_store = cleanup_assets.as_ref().unwrap_or(assets);
    let _cleanup_lease = if cleanup_assets.is_some() {
        let Some(lease) = cleanup_store.try_exclusive_lease(&hash)? else { return Ok(None) };
        Some(lease)
    } else {
        None
    };
    let mut retained = None;
    let mut path = snapshot.resolved_path.clone();
    let mut renamed = None;
    let output = match snapshot.work.operation.as_str() {
        "remove" if cleanup_absent || cleanup_obsolete || snapshot.placement.is_some() || (historical_root.is_none() && snapshot.live_book_paths.contains(&path)) => None,
        "remove" => {
            if !snapshot.live_book_paths.is_empty() {
                retained = assets.prepare_retained_placement(&hash, &snapshot.live_book_paths)?;
                if retained.is_none() {
                    return Err(BackendError::message("destination copy is not ready; the source file has been kept"));
                }
            }
            Some(cleanup_store.prepare_removal(&hash, &path)?)
        }
        "materialize" => {
            if let Some((dir, name)) = &snapshot.placement {
                let mut alternatives = library_replica::file_name_candidates(name, plan.occupied.iter().map(String::as_str).chain(std::iter::once(name.as_str())));
                while assets.placement_conflicts(&hash, &path)? {
                    let chosen = alternatives.next().ok_or_else(|| BackendError::message("no collision-free file name remains"))?;
                    path = path.with_file_name(&chosen).map_err(BackendError::operation)?;
                    renamed = Some((dir.clone(), chosen));
                }
                let sources = plan.movable.iter().filter(|_| !copy_move).filter_map(|work| work.cleanup_path.as_ref());
                let Some(prepared) = assets.prepare_placement(&hash, &path, &snapshot.book_paths, sources)? else { return Ok(None) };
                Some(prepared)
            } else {
                None
            }
        }
        operation => return Err(BackendError::message(format!("unknown file operation: {operation}"))),
    };
    Ok(Some(PreparedFileWork { plan, output, retained, path, renamed, cleanup_assets, root_access }))
}

/// Called under the gate. A superseded job keeps its durable work for a fresh
/// pass, and dropping its prepared output removes any abandoned temporary copy.
async fn publish_file_work(assets: &AssetStore, database: &DatabaseClient, prepared: &mut PreparedFileWork, short: bool) -> Result<(), BackendError> {
    if !assets.root_available() {
        return Err(BackendError::message("library storage is unavailable"));
    }
    let snapshot = &prepared.plan.snapshot;
    let hash = snapshot.work.book_id;
    let Some(_lease) = assets.try_exclusive_lease(&hash)? else { return Ok(()) };
    let Some(current) = database.native_file_work_snapshot(snapshot.work.clone()).await? else { return Ok(()) };
    if &current != snapshot {
        return Ok(());
    }
    if snapshot.work.operation == "materialize" {
        if let Some((dir, _)) = &snapshot.placement {
            let mut occupied = database.shared_occupied_file_names(&DirId::parse_str(dir).map_err(BackendError::operation)?).await?;
            occupied.sort();
            if occupied != prepared.plan.occupied {
                return Ok(());
            }
        }
        if prepared.output.as_ref().is_some_and(PreparedPlacement::renames_source) {
            for removal in &prepared.plan.movable {
                if !database.native_file_work_is_current(removal).await? {
                    return Ok(());
                }
            }
        }
    }
    if snapshot.work.operation == "remove" && !cleanup_owner_valid(&snapshot.work)? {
        prepared.output = None;
    }
    let _cleanup_lease = if prepared.output.is_some() {
        if let Some(cleanup_assets) = &prepared.cleanup_assets {
            let Some(lease) = cleanup_assets.try_exclusive_lease(&hash)? else { return Ok(()) };
            Some(lease)
        } else {
            None
        }
    } else {
        None
    };
    let published = prepared.output.is_some();
    let _leases = if short {
        if prepared.output.is_some() {
            if let Some(retained) = prepared.retained.take() {
                // Preserve the source if its retained destination changed
                // while the gate was released.
                retained.publish_short()?;
            }
        }
        if let Some(output) = prepared.output.take() {
            output.publish_short()?;
        }
        (_lease, _cleanup_lease)
    } else {
        let retained = if prepared.output.is_some() { prepared.retained.take() } else { None };
        let output = prepared.output.take();
        let root_access = prepared.root_access.clone();
        run_blocking(move || {
            let _root = root_access;
            let leases = (_lease, _cleanup_lease);
            if let Some(retained) = retained {
                retained.publish_short()?;
            }
            if let Some(output) = output {
                output.publish()?;
            }
            Ok::<_, BackendError>(leases)
        })
        .await
        .map_err(BackendError::operation)??
    };
    database
        .acknowledge_native_file_work(NativeFileWorkCompletion { snapshot: current, final_path: prepared.path.to_string(), renamed: prepared.renamed.take(), published, thumbnail_set_exists: assets.thumbnail_set_exists(&hash).await? })
        .await?;
    Ok(())
}

fn cleanup_owner_valid(work: &NativeFileWork) -> Result<bool, BackendError> {
    let Some(root) = work.source_root.as_deref() else { return Ok(true) };
    let (Some(identity), Some(library)) = (work.source_identity.as_deref(), work.source_library) else { return Ok(false) };
    match cleanup_root_matches(Path::new(root), identity, library) {
        Ok(matches) => Ok(matches),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

async fn materialize_directory(assets: &AssetStore, database: &DatabaseClient, id: DirId) -> Result<(), BackendError> {
    if id == ROOT_DIR_ID {
        return Ok(());
    }
    if !assets.root_available() {
        return Err(BackendError::message("library storage is unavailable"));
    }
    loop {
        let Some(snapshot) = database.native_directory_snapshot(&id).await? else { return Ok(()) };
        let prepared = assets.inspect_directory(snapshot.id, &snapshot.relative_path, snapshot.projected_path.as_deref(), &snapshot.name, &snapshot.sibling_names)?;
        let Some(result) = prepared.publish()? else { continue };
        let completion = library_database::NativeDirectoryCompletion { snapshot, renamed_to: result.renamed_to, materialized: result.materialized };
        match database.acknowledge_native_directory(completion).await? {
            library_database::NativeDirectoryAcknowledgement::Applied | library_database::NativeDirectoryAcknowledgement::Obsolete => return Ok(()),
            library_database::NativeDirectoryAcknowledgement::Retry => continue,
        }
    }
}

/// Live snapshots include both paths; hidden folders retain only their physical location.
async fn directory_paths(database: &DatabaseClient, id: &DirId) -> Result<(Option<String>, Option<String>), DatabaseError> {
    match database.native_directory_snapshot(id).await? {
        Some(snapshot) => Ok((Some(snapshot.relative_path), snapshot.projected_path)),
        None => Ok((None, database.native_directory_projected_path(id).await?)),
    }
}

struct DirectoryReplay {
    recovery: Vec<DirId>,
    ordered: Vec<DirId>,
    work_ids: Vec<i64>,
}

async fn plan_directory_replay(database: &DatabaseClient, requested: Option<&[DirId]>) -> Result<DirectoryReplay, BackendError> {
    let work = database.native_directory_replay_work().await.map_err(BackendError::operation)?;
    let mut directories: Vec<_> = work.iter().map(|(_, directory)| *directory).collect();
    directories.extend(database.native_book_work_directory_ids().await.map_err(BackendError::operation)?);
    directories.extend(requested.into_iter().flatten().copied());
    directories.sort();
    directories.dedup();
    if let Some(requested) = requested {
        // Restrict interactive work to ancestors and intersecting physical
        // moves. A broken move in another subtree must not block imports.
        let mut paths = HashMap::new();
        for id in &directories {
            paths.insert(*id, directory_paths(database, id).await.map_err(BackendError::operation)?);
        }
        let overlaps = |a: &str, b: &str| a == b || a.strip_prefix(b).is_some_and(|suffix| suffix.starts_with('/')) || b.strip_prefix(a).is_some_and(|suffix| suffix.starts_with('/'));
        let mut selected = requested.iter().copied().collect::<HashSet<_>>();
        let mut pending = requested.to_vec();
        while let Some(id) = pending.pop() {
            for component in database.dir_path_components(&id).await.map_err(BackendError::operation)? {
                if selected.insert(component.id) {
                    pending.push(component.id);
                }
            }
            let Some((desired, old)) = paths.get(&id) else { continue };
            for (candidate, (candidate_desired, candidate_old)) in &paths {
                if [desired, old].into_iter().flatten().any(|path| [candidate_desired, candidate_old].into_iter().flatten().any(|other| overlaps(path, other))) && selected.insert(*candidate) {
                    pending.push(*candidate);
                }
            }
        }
        directories = selected.into_iter().collect();
        directories.sort();
    }
    // Materialize effective parents first. In particular, lift a repaired
    // cycle root out of its old ancestor before moving that ancestor below it.
    let mut ordered = Vec::new();
    let mut seen = HashSet::new();
    let selected: HashSet<_> = directories.iter().copied().collect();
    for directory in directories {
        for component in database.dir_path_components(&directory).await.map_err(BackendError::operation)? {
            if seen.insert(component.id) {
                ordered.push(component.id);
            }
        }
    }
    // Hidden directories have no live ancestry, but interrupted physical
    // staging must still be recovered before permanent removal is replayed.
    let recovery = ordered.iter().copied().chain(work.iter().map(|(_, id)| *id).filter(|id| selected.contains(id) && seen.insert(*id))).collect();
    let work_ids = work.into_iter().filter(|(_, directory)| selected.contains(directory)).map(|(id, _)| id).collect();
    Ok(DirectoryReplay { recovery, ordered, work_ids })
}

async fn stage_directory(assets: &AssetStore, database: &DatabaseClient, directory: DirId) -> Result<(), BackendError> {
    let (desired, old) = directory_paths(database, &directory).await?;
    if let Some(old) = old {
        if let Some(staged) = assets.stage_directory(directory, &old, desired.as_deref())? {
            database.acknowledge_native_directory_staging(&directory, &old, &staged).await?;
        }
    }
    Ok(())
}

pub(super) async fn replay_directories(assets: &AssetStore, database: &DatabaseClient, requested: Option<&[DirId]>) -> Result<(), BackendError> {
    let plan = plan_directory_replay(database, requested).await?;
    // Stage all changing folders before materializing any, including swaps.
    for directory in plan.recovery {
        stage_directory(assets, database, directory).await?;
    }
    for directory in plan.ordered {
        materialize_directory(assets, database, directory).await?;
    }
    database.complete_directory_replay(&plan.work_ids).await?;
    Ok(())
}

/// Release the gate after each directory. If a foreground edit or sync adds
/// directory work, restart staging before publishing further destinations:
/// otherwise a newly queued name swap could be mistaken for a foreign folder.
pub(super) async fn replay_background_directories(state: &RwLock<LibraryState>) -> Result<(), BackendError> {
    'replan: loop {
        let (plan, revision) = {
            let storage = state.write().await;
            (plan_directory_replay(storage.database(), None).await?, storage.database().native_directory_work_revision().await?)
        };
        for directory in plan.recovery {
            {
                let storage = state.write().await;
                if storage.database().native_directory_work_revision().await? != revision {
                    continue 'replan;
                }
                stage_directory(storage.assets(), storage.database(), directory).await?;
            }
            tokio::task::yield_now().await;
        }
        for directory in plan.ordered {
            {
                let storage = state.write().await;
                if storage.database().native_directory_work_revision().await? != revision {
                    continue 'replan;
                }
                materialize_directory(storage.assets(), storage.database(), directory).await?;
            }
            tokio::task::yield_now().await;
        }
        let storage = state.write().await;
        if storage.database().native_directory_work_revision().await? != revision {
            continue;
        }
        storage.database().complete_directory_replay(&plan.work_ids).await?;
        return Ok(());
    }
}
