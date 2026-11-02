//! Database policy for native file replay and interrupted directory moves.

use super::{AssetStore, BackendError, BookId, Database, DirId};
use library_database::{DatabaseError, NativeFileWork};
use library_files::library_move::{checked_path, cleanup_root_matches};
use std::collections::{HashMap, HashSet};
use std::io::ErrorKind;
use std::path::Path;
use sync_common::ROOT_DIR_ID;

pub(super) fn reconcile_file_jobs(assets: &AssetStore, database: &Database, jobs: Vec<NativeFileWork>) -> Result<(), BackendError> {
    let mut movable = HashMap::<BookId, Vec<library_model::RelativeBookPath>>::new();
    for job in jobs.iter().filter(|job| job.operation == "remove") {
        if job.source_root.is_none() {
            if let Some(path) = &job.cleanup_path {
                movable.entry(job.book_id).or_default().push(path.clone());
            }
        }
    }
    let mut failure = None;
    for work in jobs {
        if let Err(error) = apply_file_work(assets, database, &movable, work) {
            failure.get_or_insert(error);
        }
    }
    failure.map_or(Ok(()), Err)
}

/// One placement job: removes a file, or materializes it under a free name.
/// Does nothing when the job is obsolete or another operation holds the book.
fn apply_file_work(assets: &AssetStore, database: &Database, movable: &HashMap<BookId, Vec<library_model::RelativeBookPath>>, work: NativeFileWork) -> Result<(), BackendError> {
    let Some(snapshot) = database.native_file_work_snapshot(work)? else { return Ok(()) };
    // Keep a staged source's cleanup pending until its live placement has
    // been restored. A failed restore must never delete the remaining bytes.
    if snapshot.staged_source {
        return Ok(());
    }
    let hash = snapshot.work.book_id;
    let Some(_lease) = assets.try_exclusive_lease(&hash)? else { return Ok(()) };
    let historical_root = snapshot.work.source_root.as_deref().filter(|root| Path::new(root) != assets.native_library_root());
    let cleanup_absent = if let Some(root) = historical_root {
        // A completed root move often already removed this file. Do not reopen
        // the old store or recreate its metadata just to acknowledge absent bytes.
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
    let cleanup_assets = cleanup_assets.as_ref().unwrap_or(assets);
    let _cleanup_lease = if historical_root.is_some() && !cleanup_absent && !cleanup_obsolete {
        let Some(lease) = cleanup_assets.try_exclusive_lease(&hash)? else { return Ok(()) };
        Some(lease)
    } else {
        None
    };
    let mut path = snapshot.resolved_path.clone();
    let mut renamed = None;
    let prepared = match snapshot.work.operation.as_str() {
        "remove" if cleanup_absent || cleanup_obsolete || snapshot.placement.is_some() || (historical_root.is_none() && snapshot.live_book_paths.contains(&path)) => None,
        "remove" => {
            if !snapshot.live_book_paths.is_empty() && !assets.verified_placement_exists(&hash, &snapshot.live_book_paths)? {
                return Err(BackendError::message("destination copy is not ready; the source file has been kept"));
            }
            Some(cleanup_assets.prepare_removal(&hash, &path)?)
        }
        "materialize" => {
            if let Some((dir, name)) = &snapshot.placement {
                let directory = DirId::parse_str(dir).map_err(BackendError::operation)?;
                let occupied = database.shared_occupied_file_names(&directory)?;
                let mut alternatives = library_replica::file_name_candidates(name, occupied.iter().map(String::as_str).chain(std::iter::once(name.as_str())));
                while assets.placement_conflicts(&hash, &path)? {
                    let chosen = alternatives.next().ok_or_else(|| BackendError::message("no collision-free file name remains"))?;
                    path = path.with_file_name(&chosen).map_err(BackendError::operation)?;
                    renamed = Some((dir.clone(), chosen));
                }
                let movable = movable.get(&hash).map(Vec::as_slice).unwrap_or_default();
                let Some(prepared) = assets.prepare_placement(&hash, &path, &snapshot.book_paths, movable)? else {
                    return Ok(());
                };
                Some(prepared)
            } else {
                None
            }
        }
        operation => return Err(BackendError::message(format!("unknown file operation: {operation}"))),
    };
    // Hashing a prepared removal may take time. Check ownership again before
    // publishing so reusing the old folder cannot authorize a stale deletion.
    let prepared = if snapshot.work.operation == "remove" && !cleanup_owner_valid(&snapshot.work)? { None } else { prepared };
    let published = prepared.is_some();
    if let Some(prepared) = prepared {
        prepared.publish()?;
    }
    database.acknowledge_native_file_work(library_database::NativeFileWorkCompletion { snapshot, final_path: path.to_string(), renamed, published, thumbnail_set_exists: assets.thumbnail_set_exists(&hash)? })?;
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

fn materialize_directory(assets: &AssetStore, database: &Database, id: DirId) -> Result<(), BackendError> {
    if id == ROOT_DIR_ID {
        return Ok(());
    }
    if !assets.root_available() {
        return Err(BackendError::message("library storage is unavailable"));
    }
    loop {
        let Some(snapshot) = database.native_directory_snapshot(&id)? else { return Ok(()) };
        let prepared = assets.inspect_directory(snapshot.id, &snapshot.relative_path, snapshot.projected_path.as_deref(), &snapshot.name, &snapshot.sibling_names)?;
        let Some(result) = prepared.publish()? else { continue };
        let completion = library_database::NativeDirectoryCompletion { snapshot, renamed_to: result.renamed_to, materialized: result.materialized };
        match database.acknowledge_native_directory(completion)? {
            library_database::NativeDirectoryAcknowledgement::Applied | library_database::NativeDirectoryAcknowledgement::Obsolete => return Ok(()),
            library_database::NativeDirectoryAcknowledgement::Retry => continue,
        }
    }
}

/// Live snapshots include both paths; hidden folders retain only their physical location.
fn directory_paths(database: &Database, id: &DirId) -> Result<(Option<String>, Option<String>), DatabaseError> {
    match database.native_directory_snapshot(id)? {
        Some(snapshot) => Ok((Some(snapshot.relative_path), snapshot.projected_path)),
        None => Ok((None, database.native_directory_projected_path(id)?)),
    }
}

pub(super) fn replay_directories(assets: &AssetStore, database: &Database, requested: Option<&[DirId]>) -> Result<(), BackendError> {
    let work = database.native_directory_replay_work().map_err(BackendError::operation)?;
    let mut directories: Vec<_> = work.iter().map(|(_, directory)| *directory).collect();
    directories.extend(database.native_book_work_directory_ids().map_err(BackendError::operation)?);
    directories.extend(requested.into_iter().flatten().copied());
    directories.sort();
    directories.dedup();
    if let Some(requested) = requested {
        // Restrict interactive work to ancestors and intersecting physical
        // moves. A broken move in another subtree must not block imports.
        let mut paths = HashMap::new();
        for id in &directories {
            paths.insert(*id, directory_paths(database, id).map_err(BackendError::operation)?);
        }
        let overlaps = |a: &str, b: &str| a == b || a.strip_prefix(b).is_some_and(|suffix| suffix.starts_with('/')) || b.strip_prefix(a).is_some_and(|suffix| suffix.starts_with('/'));
        let mut selected = requested.iter().copied().collect::<HashSet<_>>();
        let mut pending = requested.to_vec();
        while let Some(id) = pending.pop() {
            for component in database.dir_path_components(&id).map_err(BackendError::operation)? {
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
        for component in database.dir_path_components(&directory).map_err(BackendError::operation)? {
            if seen.insert(component.id) {
                ordered.push(component.id);
            }
        }
    }
    // Hidden directories have no live ancestry, but interrupted physical
    // staging must still be recovered before permanent removal is replayed.
    let recovery = ordered.iter().copied().chain(work.iter().map(|(_, id)| *id).filter(|id| selected.contains(id) && seen.insert(*id)));
    // Stage changing managed folders first, including name swaps. Otherwise
    // a destination still occupied by its previous owner looks like a foreign
    // collision and causes a new, device-dependent synchronized rename.
    for directory in recovery {
        let (desired, old) = directory_paths(database, &directory)?;
        let Some(old) = old else { continue };
        if let Some(staged) = assets.stage_directory(directory, &old, desired.as_deref())? {
            database.acknowledge_native_directory_staging(&directory, &old, &staged)?;
        }
    }
    for directory in ordered {
        materialize_directory(assets, database, directory)?;
    }
    let ids = work.into_iter().filter(|(_, directory)| selected.contains(directory)).map(|(id, _)| id).collect::<Vec<_>>();
    database.complete_directory_replay(&ids)?;
    Ok(())
}
