use super::*;

/// Applies one remote change. Returns the placement to delete locally when the
/// change was a removal that won the LWW gate; `None` otherwise.
pub fn apply_remote_change_in_tx(tx: &Transaction, change: &ServerChange) -> Result<Option<RemovedBookDir>, Box<dyn std::error::Error>> {
    observe_remote_hlc(tx, change.hlc)?;
    if should_skip_singleton_apply(tx, change)? {
        return Ok(None);
    }

    struct TxOriginGuard<'a> {
        tx: &'a Transaction<'a>,
        active: bool,
    }
    impl<'a> TxOriginGuard<'a> {
        fn activate(tx: &'a Transaction<'a>) -> Result<Self, rusqlite::Error> {
            tx.set_sync_metadata("change_origin", "remote")?;
            Ok(Self { tx, active: true })
        }
    }
    impl Drop for TxOriginGuard<'_> {
        fn drop(&mut self) {
            if self.active {
                let _ = self.tx.set_sync_metadata("change_origin", "local");
            }
        }
    }

    let _guard = TxOriginGuard::activate(tx)?;
    let mut removed = None;
    match &change.op {
        SyncOp::DirState { dir_id, directory } => {
            apply_remote_dir_state(tx, change, dir_id, directory)?;
        }
        SyncOp::BookAdded { book_id, title, added_at } => {
            apply_remote_book_added(tx, change, *book_id, title, *added_at)?;
        }
        SyncOp::BookDeleted { book_id } => {
            apply_remote_book_deleted(tx, change, *book_id)?;
        }
        SyncOp::BookDirAdded { book_id, dir_id, file_name, content_hash } => match claim_physical_book_path(tx, change, *book_id, dir_id, file_name)? {
            PathClaim::Lost => {}
            PathClaim::Won(displaced) => {
                apply_remote_book_dir(tx, change, *book_id, dir_id, file_name, RemotePlacementState::Present(content_hash))?;
                removed = displaced;
            }
        },
        SyncOp::BookDirRemoved { book_id, dir_id, file_name } => {
            if apply_remote_book_dir(tx, change, *book_id, dir_id, file_name, RemotePlacementState::Removed)? {
                removed = Some(RemovedBookDir { dir_id: dir_id.clone(), book_id: *book_id, file_name: file_name.clone() });
            }
        }
        SyncOp::ReadingPositionUpdated { book_id, cfi } => {
            if !sync_common::valid_reading_position(cfi) {
                return Err("invalid synchronized reading position".into());
            }
            tx.first_book_insert(&book_id.canonical(), storage_i64(change.changed_at, "change timestamp")?)?;
            tx.update_reading_position(&book_id.canonical(), cfi)?;
        }
        SyncOp::BookOpened { book_id, last_read } => {
            tx.first_book_insert(&book_id.canonical(), storage_i64(change.changed_at, "change timestamp")?)?;
            tx.update_last_read(&book_id.canonical(), *last_read)?;
        }
        SyncOp::BookMetadataUpdated { book_id, title, author } => {
            tx.first_book_insert(&book_id.canonical(), storage_i64(change.changed_at, "change timestamp")?)?;
            update_book_metadata(tx, *book_id, title, author)?;
        }
    }
    Ok(removed)
}

fn should_skip_singleton_apply(tx: &Transaction, change: &ServerChange) -> Result<bool, rusqlite::Error> {
    if !change.op.is_singleton() {
        return Ok(false);
    }

    let last_pushed_local_seq = match get_sync_metadata_tx(tx, "last_pushed_local_seq")? {
        Some(value) => value.parse::<u64>().map_err(|error| invalid_text_column(0, error.to_string()))?,
        None => 0,
    };
    let raw_local_actor = get_sync_metadata_tx(tx, "client_id")?.ok_or_else(|| invalid_text_column(0, "missing client id"))?;
    let local_actor = ActorId::parse(&raw_local_actor).map_err(|error| invalid_text_column(0, error.to_string()))?;
    let mut local_version: Option<VersionKey> = None;
    let entity_id = change.op.entity_id();
    tx.get_max_unpushed_hlc(last_pushed_local_seq, entity_id.as_ref(), change.op.event_type(), |row| {
        let conflict_rank = bounded_u8(row.get(4)?, 4, "conflict rank")?;
        local_version = StoredVersion::decode(row.get(0)?, row.get(1)?, local_actor.as_str().to_owned(), row.get(2)?, row.get(3)?, conflict_rank, [0, 1, 0, 2, 3])?.map(StoredVersion::into_key);
        Ok(())
    })?;

    let Some(local_version) = local_version else {
        return Ok(false);
    };

    Ok(local_version > change.version_key())
}

fn apply_remote_dir_state(tx: &Transaction, change: &ServerChange, dir_id: &DirId, directory: &DirectoryState) -> Result<(), Box<dyn std::error::Error>> {
    if dir_id.as_str() == "__root__" {
        return Ok(());
    }
    if let Some(stored) = read_dir_version(tx, dir_id)? {
        if change.version_key() < stored {
            return Ok(());
        }
    }
    tx.apply_remote_dir_state(
        dir_id.as_str(),
        directory.parent_id().as_str(),
        directory.name().as_str(),
        directory.is_deleted(),
        storage_i64(change.hlc.pt, "HLC physical time")?,
        storage_i64(change.hlc.l, "HLC logical counter")?,
        change.actor_id.as_str(),
        storage_i64(change.local_seq.get(), "actor sequence")?,
        &change.event_id.to_string(),
    )?;
    reproject_directories(tx)?;
    Ok(())
}

#[derive(Clone, Debug)]
struct DirectoryIntent {
    id: DirId,
    parent_id: DirId,
    name: FileName,
    deleted: bool,
    version: Option<StoredVersion>,
}

fn read_directory_intents(tx: &Transaction) -> Result<HashMap<DirId, DirectoryIntent>, rusqlite::Error> {
    let mut intents = HashMap::new();
    let mut statement = tx.prepare(
        "SELECT id, COALESCE(intent_parent_id, parent_id), COALESCE(intent_name, name),
                COALESCE(intent_deleted, deleted_at IS NOT NULL),
                hlc_pt, hlc_l, hlc_actor, hlc_seq, hlc_event
         FROM dir WHERE id != '__root__'",
    )?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let id = DirId::parse(&row.get::<_, String>(0)?).map_err(|error| invalid_text_column(0, error.to_string()))?;
        let parent_id = DirId::parse(&row.get::<_, String>(1)?).map_err(|error| invalid_text_column(1, error.to_string()))?;
        let name = FileName::parse(&row.get::<_, String>(2)?).map_err(|error| invalid_text_column(2, error.to_string()))?;
        let deleted = row.get(3)?;
        let version = StoredVersion::decode(row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?, u8::from(deleted), [4, 5, 6, 7, 8])?;
        intents.insert(id.clone(), DirectoryIntent { id, parent_id, name, deleted, version });
    }
    Ok(intents)
}

fn cascade_unreachable(intents: &HashMap<DirId, DirectoryIntent>, suppressed: &mut HashSet<DirId>) {
    loop {
        let newly_suppressed: Vec<_> = intents
            .values()
            .filter(|intent| !suppressed.contains(&intent.id))
            .filter(|intent| intent.parent_id.as_str() != "__root__" && (!intents.contains_key(&intent.parent_id) || suppressed.contains(&intent.parent_id)))
            .map(|intent| intent.id.clone())
            .collect();
        if newly_suppressed.is_empty() {
            break;
        }
        suppressed.extend(newly_suppressed);
    }
}

fn suppress_cycles(intents: &HashMap<DirId, DirectoryIntent>, suppressed: &mut HashSet<DirId>) {
    let mut ids: Vec<_> = intents.keys().cloned().collect();
    ids.sort();
    for start in ids {
        if suppressed.contains(&start) {
            continue;
        }
        let mut path = Vec::new();
        let mut positions = HashMap::new();
        let mut current = start;
        loop {
            if suppressed.contains(&current) || current.as_str() == "__root__" {
                break;
            }
            if let Some(position) = positions.get(&current).copied() {
                if let Some(loser) = path[position..].iter().max().cloned() {
                    suppressed.insert(loser);
                }
                break;
            }
            let Some(intent) = intents.get(&current) else {
                break;
            };
            positions.insert(current.clone(), path.len());
            path.push(current);
            current = intent.parent_id.clone();
        }
    }
}

fn suppress_path_collisions(intents: &HashMap<DirId, DirectoryIntent>, suppressed: &mut HashSet<DirId>) {
    let mut groups: HashMap<(DirId, FileName), Vec<&DirectoryIntent>> = HashMap::new();
    for intent in intents.values().filter(|intent| !suppressed.contains(&intent.id)) {
        groups.entry((intent.parent_id.clone(), intent.name.clone())).or_default().push(intent);
    }
    for group in groups.values().filter(|group| group.len() > 1) {
        let winner = group.iter().max_by(|left, right| left.version.as_ref().map(StoredVersion::key).cmp(&right.version.as_ref().map(StoredVersion::key)).then_with(|| left.id.cmp(&right.id))).expect("collision group is non-empty");
        suppressed.extend(group.iter().filter(|intent| intent.id != winner.id).map(|intent| intent.id.clone()));
    }
}

fn reproject_directories(tx: &Transaction) -> Result<(), Box<dyn std::error::Error>> {
    let before = get_live_dir_paths_tx(tx)?;
    let intents = read_directory_intents(tx)?;
    let mut suppressed: HashSet<_> = intents.values().filter(|intent| intent.deleted).map(|intent| intent.id.clone()).collect();
    suppress_cycles(&intents, &mut suppressed);
    cascade_unreachable(&intents, &mut suppressed);
    suppress_path_collisions(&intents, &mut suppressed);
    cascade_unreachable(&intents, &mut suppressed);

    tx.execute("UPDATE dir SET deleted_at=1 WHERE id != '__root__'", [])?;
    let mut ids: Vec<_> = intents.keys().cloned().collect();
    ids.sort();
    for id in &ids {
        let intent = &intents[id];
        let projected_parent = if suppressed.contains(id) { "__root__" } else { intent.parent_id.as_str() };
        tx.execute("UPDATE dir SET parent_id=?1, name=?2 WHERE id=?3", rusqlite::params![projected_parent, intent.name.as_str(), intent.id.as_str()])?;
    }

    let mut active = HashSet::new();
    let mut remaining: Vec<_> = ids.into_iter().filter(|id| !suppressed.contains(id)).collect();
    while !remaining.is_empty() {
        let Some(position) = remaining.iter().position(|id| {
            let parent = &intents[id].parent_id;
            parent.as_str() == "__root__" || active.contains(parent)
        }) else {
            return Err("directory projection contains an unreachable cycle".into());
        };
        let id = remaining.remove(position);
        tx.execute("UPDATE dir SET deleted_at=NULL WHERE id=?1", [id.as_str()])?;
        active.insert(id);
    }

    let after = get_live_dir_paths_tx(tx)?;
    let now = now_ts_secs()?;
    for (id, old_path) in &before {
        if !after.contains_key(id) {
            tx.enqueue_dir_removal(id.as_str(), old_path.as_str(), now)?;
        }
    }
    for id in after.keys() {
        tx.cancel_dir_removal(id.as_str())?;
    }
    Ok(())
}

fn read_dir_version(tx: &Transaction, dir_id: &DirId) -> Result<Option<VersionKey>, rusqlite::Error> {
    let mut version = None;
    tx.get_dir_version(dir_id.as_str(), |row| {
        version = StoredVersion::decode(row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(5)?, u8::from(row.get::<_, bool>(4)?), [0, 1, 2, 3, 5])?.map(StoredVersion::into_key);
        Ok(())
    })?;
    Ok(version)
}

fn apply_remote_book_added(tx: &Transaction, change: &ServerChange, book_id: BookId, title: &str, added_at: i64) -> Result<(), Box<dyn std::error::Error>> {
    if !book_version_wins(tx, change, book_id)? {
        return Ok(());
    }
    tx.apply_remote_book_added(
        &book_id.canonical(),
        title,
        added_at,
        storage_i64(change.hlc.pt, "HLC physical time")?,
        storage_i64(change.hlc.l, "HLC logical counter")?,
        change.actor_id.as_str(),
        storage_i64(change.local_seq.get(), "actor sequence")?,
        &change.event_id.to_string(),
    )?;
    Ok(())
}

fn apply_remote_book_deleted(tx: &Transaction, change: &ServerChange, book_id: BookId) -> Result<(), Box<dyn std::error::Error>> {
    if !book_version_wins(tx, change, book_id)? {
        return Ok(());
    }
    tx.apply_remote_book_deleted(
        &book_id.canonical(),
        storage_i64(change.changed_at, "change timestamp")?,
        storage_i64(change.hlc.pt, "HLC physical time")?,
        storage_i64(change.hlc.l, "HLC logical counter")?,
        change.actor_id.as_str(),
        storage_i64(change.local_seq.get(), "actor sequence")?,
        &change.event_id.to_string(),
    )?;
    Ok(())
}

fn book_version_wins(tx: &Transaction, change: &ServerChange, book_id: BookId) -> Result<bool, rusqlite::Error> {
    let mut stored: Option<VersionKey> = None;
    tx.get_book_version(&book_id.canonical(), |row| {
        let deleted: bool = row.get(4)?;
        stored = StoredVersion::decode(row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(5)?, u8::from(deleted), [0, 1, 2, 3, 5])?.map(StoredVersion::into_key);
        Ok(())
    })?;
    Ok(stored.is_none_or(|stored| change.version_key() > stored))
}

/// Returns whether the event was applied (won the LWW gate). The caller uses a
/// `true` from a removal to delete the local file, propagating the deletion.
#[derive(Clone, Copy)]
pub(super) enum RemotePlacementState<'a> {
    Present(&'a ContentHash),
    Removed,
}

enum PathClaim {
    Lost,
    Won(Option<RemovedBookDir>),
}

/// Resolves the path register before applying an add. Every historical
/// occupant participates in the version comparison, while at most one live
/// occupant is retained. A replacement is therefore one final path-state add;
/// a later scanner removal for the displaced book cannot erase the replacement.
fn claim_physical_book_path(tx: &Transaction, change: &ServerChange, book_id: BookId, dir_id: &DirId, file_name: &FileName) -> Result<PathClaim, Box<dyn std::error::Error>> {
    let mut greatest: Option<VersionKey> = None;
    let mut live_other: Option<BookId> = None;
    {
        let mut statement = tx.prepare(
            "SELECT book_id, hlc_pt, hlc_l, hlc_actor, hlc_seq,
                    COALESCE(intent_deleted, deleted_at IS NOT NULL), deleted_at IS NOT NULL, hlc_event
             FROM book_dir WHERE dir_id=?1 AND file_name=?2",
        )?;
        let mut rows = statement.query(rusqlite::params![dir_id.as_str(), file_name.as_str()])?;
        while let Some(row) = rows.next()? {
            let stored_book = BookId::parse(&row.get::<_, String>(0)?).map_err(|error| invalid_text_column(0, error.to_string()))?;
            if let Some(version) = StoredVersion::decode(row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(7)?, u8::from(row.get::<_, bool>(5)?), [1, 2, 3, 4, 7])? {
                let version = version.into_key();
                greatest = Some(greatest.map_or(version.clone(), |current| current.max(version)));
            }
            if stored_book != book_id && !row.get::<_, bool>(6)? {
                live_other = Some(stored_book);
            }
        }
    }

    if greatest.as_ref().is_some_and(|stored| change.version_key() <= *stored) {
        // An idempotent retry for the already-live winning occupant remains a
        // successful no-op; only a different occupant loses the claim.
        let same_is_live: bool =
            tx.query_row("SELECT EXISTS(SELECT 1 FROM book_dir WHERE dir_id=?1 AND file_name=?2 AND book_id=?3 AND deleted_at IS NULL)", rusqlite::params![dir_id.as_str(), file_name.as_str(), book_id.canonical()], |row| row.get(0))?;
        return Ok(if same_is_live { PathClaim::Won(None) } else { PathClaim::Lost });
    }

    let displaced = if let Some(old_book_id) = live_other {
        let pending_rel_path = get_dir_rel_path_tx(tx, dir_id.as_str())?.and_then(|path| path.join_file(file_name).ok()).map(|path| path.as_str().to_owned()).unwrap_or_default();
        tx.execute("UPDATE book_dir SET deleted_at=1 WHERE dir_id=?1 AND file_name=?2 AND book_id=?3 AND deleted_at IS NULL", rusqlite::params![dir_id.as_str(), file_name.as_str(), old_book_id.canonical()])?;
        tx.enqueue_book_file_removal(dir_id.as_str(), &old_book_id.canonical(), file_name.as_str(), &pending_rel_path, now_ts_secs()?)?;
        Some(RemovedBookDir { dir_id: dir_id.clone(), book_id: old_book_id, file_name: file_name.clone() })
    } else {
        None
    };
    Ok(PathClaim::Won(displaced))
}

pub(super) fn apply_remote_book_dir(tx: &Transaction, change: &ServerChange, book_id: BookId, dir_id: &DirId, file_name: &FileName, state: RemotePlacementState<'_>) -> Result<bool, Box<dyn std::error::Error>> {
    // The complete version key makes every add/remove pair totally ordered,
    // including equal-HLC add/add conflicts and idempotent retries.
    if let Some(stored) = read_book_dir_version(tx, dir_id, book_id, file_name)? {
        if change.version_key() <= stored {
            return Ok(false);
        }
    }

    let pending_rel_path = match state {
        RemotePlacementState::Present(_) => None,
        RemotePlacementState::Removed => get_dir_rel_path_tx(tx, dir_id.as_str())?.and_then(|path| path.join_file(file_name).ok()).map(|path| path.as_str().to_owned()),
    };
    let stored_book_id = book_id.canonical();
    tx.first_book_insert(&stored_book_id, storage_i64(change.changed_at, "change timestamp")?)?;
    match state {
        RemotePlacementState::Present(content_hash) => {
            tx.apply_remote_book_dir_added(
                dir_id.as_str(),
                &stored_book_id,
                file_name.as_str(),
                content_hash.as_str(),
                storage_i64(change.hlc.pt, "HLC physical time")?,
                storage_i64(change.hlc.l, "HLC logical counter")?,
                change.actor_id.as_str(),
                storage_i64(change.local_seq.get(), "actor sequence")?,
                &change.event_id.to_string(),
            )?;
            tx.cancel_book_file_removal(dir_id.as_str(), &stored_book_id, file_name.as_str())?;
        }
        RemotePlacementState::Removed => {
            // Tombstone the placement (created if absent) so a later lower-HLC add loses.
            let tombstone_key = remote_book_dir_tombstone_key(book_id, dir_id.as_str(), file_name.as_str());
            tx.apply_remote_book_dir_removed(
                dir_id.as_str(),
                &stored_book_id,
                file_name.as_str(),
                &tombstone_key,
                storage_i64(change.hlc.pt, "HLC physical time")?,
                storage_i64(change.hlc.l, "HLC logical counter")?,
                change.actor_id.as_str(),
                storage_i64(change.local_seq.get(), "actor sequence")?,
                &change.event_id.to_string(),
            )?;
            tx.enqueue_book_file_removal(dir_id.as_str(), &stored_book_id, file_name.as_str(), pending_rel_path.as_deref().unwrap_or(""), now_ts_secs()?)?;
        }
    }
    Ok(true)
}

fn read_book_dir_version(tx: &Transaction, dir_id: &DirId, book_id: BookId, file_name: &FileName) -> Result<Option<VersionKey>, rusqlite::Error> {
    let mut version: Option<VersionKey> = None;
    tx.get_book_dir_hlc(dir_id.as_str(), &book_id.canonical(), file_name.as_str(), |row| {
        let deleted: bool = row.get(4)?;
        version = StoredVersion::decode(row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(5)?, u8::from(deleted), [0, 1, 2, 3, 5])?.map(StoredVersion::into_key);
        Ok(())
    })?;
    Ok(version)
}

fn get_sync_metadata_tx(tx: &Transaction, key: &str) -> Result<Option<String>, rusqlite::Error> {
    let mut value: Option<String> = None;
    tx.get_sync_metadata_value(key, |row| {
        value = Some(row.get(0)?);
        Ok(())
    })?;
    Ok(value)
}

fn read_local_hlc(tx: &Transaction) -> Result<Hlc, rusqlite::Error> {
    let pt = get_sync_metadata_tx(tx, "hlc_pt")?.unwrap_or_else(|| "0".to_owned()).parse::<u64>().map_err(|error| invalid_text_column(0, error.to_string()))?;
    let l = get_sync_metadata_tx(tx, "hlc_l")?.unwrap_or_else(|| "0".to_owned()).parse::<u64>().map_err(|error| invalid_text_column(0, error.to_string()))?;
    Ok(Hlc { pt, l })
}

fn write_local_hlc(tx: &Transaction, hlc: Hlc) -> Result<(), rusqlite::Error> {
    tx.set_sync_metadata("hlc_pt", &hlc.pt.to_string())?;
    tx.set_sync_metadata("hlc_l", &hlc.l.to_string())?;
    Ok(())
}

fn observe_remote_hlc(tx: &Transaction, remote_hlc: Hlc) -> Result<(), Box<dyn std::error::Error>> {
    let prev = read_local_hlc(tx)?;
    let next = Hlc::tick_recv(prev, remote_hlc, unix_millis()?);
    write_local_hlc(tx, next)?;
    Ok(())
}

fn remote_book_dir_tombstone_key(book_id: BookId, dir_id: &str, file_name: &str) -> String {
    format!("remote:{dir_id}:{book_id}:{file_name}")
}
