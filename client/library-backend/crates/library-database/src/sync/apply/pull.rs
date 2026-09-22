//! Applying a pull page: preparation is pure; all database decisions share one transaction.
use super::*;
use crate::books::audiobook::AudiobookSql;
use crate::shared_sql::SharedSql;

type PendingMetadata<'a> = (&'a sync_common::ContentHash, &'a library_replica::SyncBookMetadata);
const BATCH_SIZE: usize = 100;

struct ApplyState<'a, 'scope> {
    origin: &'scope mut crate::transactions::RemoteOrigin,
    reproject: bool,
    pending: Vec<PendingMetadata<'a>>,
}

enum ApplyPlan {
    Additions(Vec<String>),
    Metadata(Vec<String>),
    Ordered,
}

impl ApplyPlan {
    // Only page contents are inspected here. Version and lifecycle snapshots must
    // still be read after reserving the writer, immediately before each chunk.
    fn prepare(prepared: &PreparedRemoteChanges<'_>) -> Result<Self, DatabaseError> {
        let additions = independent_additions(prepared);
        if !additions && !independent_metadata(prepared) {
            return Ok(Self::Ordered);
        }
        let keys = prepared.changes.chunks(BATCH_SIZE).map(|chunk| serde_json::to_string(&chunk.iter().map(|(_, _, identity)| identity).collect::<Vec<_>>()).map_err(DatabaseError::operation)).collect::<Result<Vec<_>, _>>()?;
        Ok(if additions { Self::Additions(keys) } else { Self::Metadata(keys) })
    }
}

fn read_cursor(conn: &rusqlite::Connection) -> Result<sync_common::SyncCursor, DatabaseError> {
    Ok(sync_common::SyncCursor { state_revision: parse_revision(read_cursor_revision(conn, PullCursorKey::State)?)?, reading_revision: parse_revision(read_cursor_revision(conn, PullCursorKey::Reading)?)? })
}

impl Database {
    /// Acknowledges sent mutations and applies a page atomically. Wire decoding,
    /// batch planning and input-only serialization run before reserving the writer.
    /// Returns whether the page carried remote changes, including stale changes.
    pub fn sync_commit_pull_response(&self, ids: &[MutationId], page: &PullStateResponse) -> Result<bool, DatabaseError> {
        if page.mutations.is_empty() && ids.is_empty() && read_cursor(&self.connection)? == page.next_cursor {
            return Ok(false);
        }
        let prepared = prepare_remote_changes(&page.mutations).map_err(DatabaseError::operation)?;
        let plan = ApplyPlan::prepare(&prepared)?;
        let acknowledgements = if ids.is_empty() { None } else { Some(serde_json::to_string(&ids.iter().map(ToString::to_string).collect::<Vec<_>>()).map_err(DatabaseError::operation)?) };
        self.with_write_transaction(|tx| {
            tx.pragma_update(None, "defer_foreign_keys", "ON").map_err(DatabaseError::operation)?;
            if let Some(payload) = acknowledgements {
                tx.outbox_delete_for_ids(&payload).map_err(DatabaseError::operation)?;
            }
            if page.mutations.is_empty() && read_cursor(tx)? == page.next_cursor {
                return Ok(crate::transactions::WriteOutcome::Commit(false));
            }
            crate::transactions::with_remote_origin(tx, |origin| {
                let mut state = ApplyState { origin, reproject: false, pending: Vec::new() };
                match plan {
                    ApplyPlan::Additions(keys) => apply_additions(tx, &prepared, &keys, &mut state)?,
                    ApplyPlan::Metadata(keys) => apply_metadata_batch(tx, &prepared, &keys, &mut state)?,
                    ApplyPlan::Ordered => apply_ordered_changes(tx, &prepared, &mut state)?,
                }
                for (hash, metadata) in state.pending {
                    project_remote_metadata(tx, hash, metadata)?;
                }
                if state.reproject {
                    rebuild_remote_directories(tx)?;
                }
                write_cursor_revision(tx, PullCursorKey::State, page.next_cursor.state_revision)?;
                write_cursor_revision(tx, PullCursorKey::Reading, page.next_cursor.reading_revision)?;
                Ok(())
            })?;
            Ok(crate::transactions::WriteOutcome::Commit(!page.mutations.is_empty()))
        })
    }
}

fn apply_additions<'a>(tx: &rusqlite::Transaction<'_>, prepared: &'a PreparedRemoteChanges<'_>, keys: &[String], state: &mut ApplyState<'a, '_>) -> Result<(), DatabaseError> {
    for (chunk, keys) in prepared.changes.chunks(BATCH_SIZE).zip(keys) {
        let snapshots = read_book_snapshots(tx, keys)?;
        let mut books: Vec<(&str, &str, i64, &str, Option<&str>)> = Vec::new();
        let mut versions: Vec<VersionRow<'_>> = Vec::new();
        let mut accepted: Vec<(&sync_common::ContentHash, &library_replica::SyncBookMetadata, bool)> = Vec::new();
        let mut queue: Vec<&str> = Vec::new();
        for ((change, body, identity), snapshot) in chunk.iter().zip(&snapshots) {
            if snapshot.version.as_ref().is_some_and(|stored| incoming_version(change) <= *stored) {
                continue;
            }
            let MutationBody::BookLifecycle { content_hash, value: library_replica::BookLifecycleState::Present { metadata, added_at, format } } = body else { unreachable!("independent additions checked by caller") };
            state.origin.mark(tx)?;
            books.push((content_hash.as_str(), metadata.title.as_str(), storage_i64(*added_at, "book addition timestamp").map_err(DatabaseError::operation)?, format.canonical_extension(), metadata.subtitle.as_deref()));
            versions.push(version_row(change, identity)?);
            accepted.push((content_hash, metadata, snapshot.has_metadata));
            queue.push(content_hash.as_str());
        }
        if accepted.is_empty() {
            continue;
        }
        tx.apply_remote_books_added_batch(&serde_json::to_string(&books).map_err(DatabaseError::operation)?).map_err(DatabaseError::operation)?;
        let projecting = accepted.iter().filter(|(_, _, has_metadata)| !has_metadata).map(|(hash, metadata, _)| (*hash, *metadata)).collect::<Vec<_>>();
        state.pending.extend(projecting);
        tx.record_versions_batch(&serde_json::to_string(&versions).map_err(DatabaseError::operation)?).map_err(DatabaseError::operation)?;
        tx.queue_book_work_batch(&serde_json::to_string(&queue).map_err(DatabaseError::operation)?).map_err(DatabaseError::operation)?;
    }
    Ok(())
}

fn apply_metadata_batch<'a>(tx: &rusqlite::Transaction<'_>, prepared: &'a PreparedRemoteChanges<'_>, keys: &[String], state: &mut ApplyState<'a, '_>) -> Result<(), DatabaseError> {
    for (chunk, keys) in prepared.changes.chunks(BATCH_SIZE).zip(keys) {
        let snapshots = read_book_snapshots(tx, keys)?;
        let mut missing: Vec<(&str, i64)> = Vec::new();
        let mut metadata: Vec<(&sync_common::ContentHash, &library_replica::SyncBookMetadata)> = Vec::new();
        let mut versions: Vec<VersionRow<'_>> = Vec::new();
        for ((change, body, identity), snapshot) in chunk.iter().zip(&snapshots) {
            if snapshot.version.as_ref().is_some_and(|stored| incoming_version(change) <= *stored) {
                continue;
            }
            let MutationBody::Metadata { content_hash, value } = body else { unreachable!("independent metadata checked by caller") };
            state.origin.mark(tx)?;
            versions.push(version_row(change, identity)?);
            if !snapshot.is_purged {
                if !snapshot.exists {
                    missing.push((content_hash.as_str(), storage_i64(change.mutation.changed_at, "change timestamp").map_err(DatabaseError::operation)?));
                }
                metadata.push((content_hash, value));
            }
        }
        if versions.is_empty() {
            continue;
        }
        tx.ensure_remote_books_batch(&serde_json::to_string(&missing).map_err(DatabaseError::operation)?).map_err(DatabaseError::operation)?;
        state.pending.extend(metadata.drain(..));
        tx.record_versions_batch(&serde_json::to_string(&versions).map_err(DatabaseError::operation)?).map_err(DatabaseError::operation)?;
    }
    Ok(())
}

fn apply_ordered_changes<'a>(tx: &rusqlite::Transaction<'_>, prepared: &'a PreparedRemoteChanges<'_>, state: &mut ApplyState<'a, '_>) -> Result<(), DatabaseError> {
    for (change, body, identity) in &prepared.changes {
        let incoming = incoming_version(change);
        let stored = read_version(tx, identity.0, &identity.1, &identity.2)?;
        if stored.is_some_and(|stored| incoming <= stored) {
            continue;
        }
        state.origin.mark(tx)?;
        apply_mutation(tx, change, body, state)?;
        let (kind, key, subkey, changed_at, rank, actor, sequence, mutation_id) = version_row(change, identity)?;
        tx.record_sync_state_version_insert(kind, key, subkey, changed_at, rank, &actor, sequence, &mutation_id)?;
        #[cfg(not(target_arch = "wasm32"))]
        match body {
            MutationBody::Placement { content_hash, .. } | MutationBody::BookLifecycle { content_hash, .. } => {
                tx.queue_book_work_insert(content_hash.as_str()).map_err(DatabaseError::operation)?;
            }
            _ => {}
        }
        if matches!(body, MutationBody::DirectoryName { .. } | MutationBody::DirectoryParent { .. } | MutationBody::DirectoryLifecycle { .. }) {
            state.reproject = true;
        }
    }
    Ok(())
}

fn apply_mutation<'a>(tx: &rusqlite::Transaction<'_>, change: &sync_common::ServerMutation, body: &'a MutationBody, state: &mut ApplyState<'a, '_>) -> Result<(), DatabaseError> {
    let mut pending_metadata = None;
    match body {
        MutationBody::DirectoryName { dir_id, name } => {
            if *dir_id != sync_common::ROOT_DIR_ID {
                tx.ensure_remote_directory_placeholder(&dir_id.to_string()).map_err(DatabaseError::operation)?;
            }
            tx.set_remote_directory_name(&dir_id.to_string(), name.as_str()).map_err(DatabaseError::operation)?;
            state.reproject = true;
        }
        MutationBody::DirectoryParent { dir_id, parent_id } => {
            if *parent_id != sync_common::ROOT_DIR_ID {
                tx.ensure_remote_directory_placeholder(&parent_id.to_string()).map_err(DatabaseError::operation)?;
            }
            if *dir_id != sync_common::ROOT_DIR_ID {
                tx.ensure_remote_directory_placeholder(&dir_id.to_string()).map_err(DatabaseError::operation)?;
            }
            tx.set_remote_directory_parent(&dir_id.to_string(), &parent_id.to_string()).map_err(DatabaseError::operation)?;
            state.reproject = true;
        }
        MutationBody::DirectoryLifecycle { dir_id, value } => {
            if *dir_id != sync_common::ROOT_DIR_ID {
                tx.ensure_remote_directory_placeholder(&dir_id.to_string()).map_err(DatabaseError::operation)?;
            }
            let value = match value {
                DirectoryLifecycleState::Present => 0,
                DirectoryLifecycleState::Deleted => 1,
                DirectoryLifecycleState::Purged => 2,
            };
            tx.set_remote_directory_lifecycle(&dir_id.to_string(), value).map_err(DatabaseError::operation)?;
            state.reproject = true;
        }
        MutationBody::BookLifecycle { content_hash, value } => {
            pending_metadata = apply_book_lifecycle(tx, change, content_hash, value)?;
        }
        MutationBody::Placement { dir_id, content_hash, present, origin_folder_id } => {
            if !book_is_purged(tx, content_hash.as_str())? {
                if *dir_id != sync_common::ROOT_DIR_ID {
                    tx.ensure_remote_directory_placeholder(&dir_id.to_string()).map_err(DatabaseError::operation)?;
                }
                tx.first_book_insert(content_hash.as_str(), storage_i64(change.mutation.changed_at, "placement timestamp").map_err(DatabaseError::operation)?).map_err(DatabaseError::operation)?;
                if *present {
                    tx.apply_remote_book_dir_added(&dir_id.to_string(), content_hash.as_str()).map_err(DatabaseError::operation)?;
                } else {
                    tx.apply_remote_book_dir_removed(&dir_id.to_string(), content_hash.as_str(), origin_folder_id.as_deref()).map_err(DatabaseError::operation)?;
                }
            }
        }
        MutationBody::ReadingPosition { content_hash, value } => {
            if !book_is_purged(tx, content_hash.as_str())? {
                let progress = book_model::ReadProgress::new(value.progress).map_err(|error| DatabaseError::operation(error.to_string()))?;
                tx.first_book_insert(content_hash.as_str(), storage_i64(change.mutation.changed_at, "change timestamp").map_err(DatabaseError::operation)?).map_err(DatabaseError::operation)?;
                tx.read_position_update(&value.location.as_str(), progress.value(), content_hash.as_str()).map_err(DatabaseError::operation)?;
            }
        }
        MutationBody::Annotation { annotation_id, value } => {
            if !book_is_purged(tx, value.content_hash.as_str())? {
                tx.first_book_insert(value.content_hash.as_str(), storage_i64(change.mutation.changed_at, "change timestamp").map_err(DatabaseError::operation)?).map_err(DatabaseError::operation)?;
                let modified_at = storage_i64(value.modified_at, "annotation modification timestamp").map_err(DatabaseError::operation)?;
                let detail = value.detail_json().map_err(DatabaseError::operation)?;
                tx.upsert_annotation(annotation_id.as_str(), value.content_hash.as_str(), value.toc_ordinal, value.progress, detail.as_slice(), modified_at, value.deleted.then_some(modified_at)).map_err(DatabaseError::operation)?;
            }
        }
        MutationBody::PdfReaderMetadata { content_hash, value } => {
            if !book_is_purged(tx, content_hash.as_str())? {
                tx.first_book_insert(content_hash.as_str(), storage_i64(change.mutation.changed_at, "change timestamp").map_err(DatabaseError::operation)?).map_err(DatabaseError::operation)?;
                let checksum = sync_common::ContentHash::new(&value.checksum);
                if !value.valid_for(checksum.as_str()) {
                    return Err(DatabaseError::message("PDF ingestion checksum or metadata is invalid"));
                }
                let bytes = serde_json::to_vec(&value).map_err(DatabaseError::operation)?;
                tx.pdf_reader_metadata_upsert(content_hash.as_str(), checksum.as_str(), &bytes).map_err(DatabaseError::operation)?;
            }
        }
        MutationBody::BookToc { content_hash, value } => {
            if !book_is_purged(tx, content_hash.as_str())? {
                tx.first_book_insert(content_hash.as_str(), storage_i64(change.mutation.changed_at, "change timestamp").map_err(DatabaseError::operation)?).map_err(DatabaseError::operation)?;
                // Duration first, mirroring the local write order: it is what
                // turns the last chapter's start into its end.
                if let Some(duration_ms) = value.duration_ms {
                    let duration_ms = i64::try_from(duration_ms).map_err(|_| DatabaseError::message("audiobook duration exceeds SQLite integer range"))?;
                    tx.audiobook_upsert_metadata(content_hash.as_str(), duration_ms).map_err(DatabaseError::operation)?;
                }
                let entry_count = i64::try_from(toc_entry_count(&value.entries)).map_err(|_| DatabaseError::message("table of contents entry count exceeds SQLite integer range"))?;
                let toc_json = serde_json::to_string(&value.entries).map_err(DatabaseError::operation)?;
                tx.shared_audiobook_upsert_toc(content_hash.as_str(), entry_count, &toc_json).map_err(DatabaseError::operation)?;
            }
        }
        MutationBody::Description { content_hash, value } => {
            if !book_is_purged(tx, content_hash.as_str())? {
                tx.first_book_insert(content_hash.as_str(), storage_i64(change.mutation.changed_at, "change timestamp").map_err(DatabaseError::operation)?).map_err(DatabaseError::operation)?;
                let normalized = normalized_description(value);
                tx.set_description(content_hash.as_str(), &normalized).map_err(DatabaseError::operation)?;
                tx.shared_enrichment_mark_description_scanned(content_hash.as_str()).map_err(DatabaseError::operation)?;
            }
        }
        MutationBody::Metadata { content_hash, value } => {
            if !book_is_purged(tx, content_hash.as_str())? {
                tx.first_book_insert(content_hash.as_str(), storage_i64(change.mutation.changed_at, "change timestamp").map_err(DatabaseError::operation)?).map_err(DatabaseError::operation)?;
                pending_metadata = Some((content_hash, value));
            }
        }
    }
    if let Some(metadata) = pending_metadata {
        state.pending.push(metadata);
    }
    Ok(())
}

fn book_is_purged(tx: &rusqlite::Transaction<'_>, hash: &str) -> Result<bool, DatabaseError> {
    let mut purged = false;
    tx.book_is_purged_select(hash, |row| {
        purged = row.get(0)?;
        Ok(())
    })
    .map_err(DatabaseError::operation)?;
    Ok(purged)
}

fn apply_book_lifecycle<'a>(
    tx: &rusqlite::Transaction<'_>, change: &sync_common::ServerMutation, content_hash: &'a sync_common::ContentHash, value: &'a library_replica::BookLifecycleState,
) -> Result<Option<PendingMetadata<'a>>, DatabaseError> {
    let mut pending_metadata = None;
    match value {
        library_replica::BookLifecycleState::Present { metadata, added_at, format } => {
            let mut has_metadata = false;
            tx.remote_book_has_metadata(content_hash.as_str(), |row| {
                has_metadata = row.get(0)?;
                Ok(())
            })
            .map_err(DatabaseError::operation)?;
            tx.apply_remote_book_added(content_hash.as_str(), metadata.title.as_str(), metadata.subtitle.as_deref(), storage_i64(*added_at, "book addition timestamp").map_err(DatabaseError::operation)?, format.canonical_extension())
                .map_err(DatabaseError::operation)?;
            if !has_metadata {
                pending_metadata = Some((content_hash, metadata));
            }
        }
        library_replica::BookLifecycleState::Deleted { origin_folder_id } => {
            tx.apply_remote_book_deleted(content_hash.as_str(), storage_i64(change.mutation.changed_at, "book deletion timestamp").map_err(DatabaseError::operation)?, origin_folder_id.as_deref()).map_err(DatabaseError::operation)?;
        }
        library_replica::BookLifecycleState::Purged => {
            tx.apply_remote_book_purged(content_hash.as_str(), storage_i64(change.mutation.changed_at, "book purge timestamp").map_err(DatabaseError::operation)?).map_err(DatabaseError::operation)?;
            let mut row_id: Option<i64> = None;
            tx.shared_apply_remote_book_purged_select(content_hash.as_str(), |row| {
                row_id = row.get(0)?;
                Ok(())
            })
            .map_err(DatabaseError::operation)?;
            let row_id = row_id.ok_or(rusqlite::Error::QueryReturnedNoRows).map_err(DatabaseError::operation)?;
            crate::purge::clear_book_metadata(tx, row_id)?;
            tx.shared_purge_book_record_delete_3(row_id).map_err(DatabaseError::operation)?;
            tx.shared_purge_book_record_delete_4(content_hash.as_str()).map_err(DatabaseError::operation)?;
            tx.apply_remote_book_purged_update(row_id).map_err(DatabaseError::operation)?;
            tx.shared_purge_book_record_delete_7(row_id).map_err(DatabaseError::operation)?;
            tx.shared_purge_book_record_insert(content_hash.as_str()).map_err(DatabaseError::operation)?;
        }
    }
    Ok(pending_metadata)
}

// Recheck lifecycle after the whole page so queued metadata cannot revive a purge.
fn project_remote_metadata(tx: &rusqlite::Transaction<'_>, meta_hash: &sync_common::ContentHash, meta: &library_replica::SyncBookMetadata) -> Result<(), DatabaseError> {
    let mut purged = false;
    tx.book_is_purged_select(meta_hash.as_str(), |row| {
        purged = row.get(0)?;
        Ok(())
    })
    .map_err(DatabaseError::operation)?;
    if purged {
        return Ok(());
    }
    // Every book always gets a full, unconditional re-resolve and
    // re-projection: idempotent regardless of whether this exact
    // metadata was already stored, so no prior-state read or diff
    // decides what to skip.
    let stored_contributors = crate::contributors::resolve_credits(tx, &meta.contributors)?;
    let prepared_publishers = crate::contributors::resolve_publishers(tx, &meta.book.publishers, crate::contributors::IdentityPolicy::StableOnly)?;
    let mut prepared_book = meta.book.clone();
    prepared_book.publishers = prepared_publishers;
    let encoded = sync_common::wire::encode(&prepared_book).map_err(DatabaseError::operation)?;
    tx.meta_update_book_metadata_state_preserving_ids_update_2(meta_hash.as_str(), &meta.title, meta.subtitle.as_deref(), &encoded).map_err(DatabaseError::operation)?;
    crate::contributors::write_credits(tx, meta_hash.as_str(), &stored_contributors)?;
    project_prepared_book(tx, meta_hash.as_str(), &prepared_book, encoded)?;
    Ok(())
}

pub(crate) fn rebuild_remote_directories(tx: &rusqlite::Transaction<'_>) -> Result<(), DatabaseError> {
    // Directory writes invalidate folder membership; the next browse refreshes
    // it once from the final tree, just as for local directory batches.
    #[cfg(not(target_arch = "wasm32"))]
    let mut before: std::collections::HashMap<String, (String, String, bool)> = std::collections::HashMap::new();
    #[cfg(not(target_arch = "wasm32"))]
    tx.directory_projection_rows_select(|row| {
        before.insert(row.get(0)?, (row.get(1)?, row.get(2)?, row.get(3)?));
        Ok(())
    })
    .map_err(DatabaseError::operation)?;
    let intents = read_directory_intents(tx)?;
    let projection = library_replica::project_directories(&intents);
    tx.hide_remote_directories().map_err(DatabaseError::operation)?;
    let mut ids: Vec<_> = intents.keys().copied().collect();
    ids.sort();
    // Detach the old tree before installing new edges. Even hidden rows are
    // traversed by asset invalidation triggers, so intermediate cycles are unsafe.
    tx.execute("UPDATE dir SET parent_id='00000000-0000-0000-0000-000000000000' WHERE id!='00000000-0000-0000-0000-000000000000' AND parent_id!='00000000-0000-0000-0000-000000000000'", [])?;
    for id in &ids {
        let intent = &intents[id];
        let projected_parent = if projection.suppressed.contains(id) { sync_common::ROOT_DIR_ID } else { projection.parents[id] };
        let projected_name = projection.names.get(id).unwrap_or(&intent.name);
        tx.project_remote_directory(&intent.id.to_string(), &projected_parent.to_string(), projected_name.as_str()).map_err(DatabaseError::operation)?;
        if intent.lifecycle == DirectoryLifecycleState::Purged {
            tx.mark_remote_directory_purged(&intent.id.to_string()).map_err(DatabaseError::operation)?;
        }
    }
    for id in projection.ordered_active {
        tx.activate_remote_directory(&id.to_string()).map_err(DatabaseError::operation)?;
    }
    #[cfg(not(target_arch = "wasm32"))]
    let mut after: Vec<(String, (String, String, bool))> = Vec::new();
    #[cfg(not(target_arch = "wasm32"))]
    tx.directory_projection_rows_select(|row| {
        after.push((row.get::<_, String>(0)?, (row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, bool>(3)?)));
        Ok(())
    })
    .map_err(DatabaseError::operation)?;
    #[cfg(not(target_arch = "wasm32"))]
    for (id, state) in after {
        if before.get(&id) != Some(&state) {
            let dir = sync_common::DirId::parse_str(&id).map_err(DatabaseError::operation)?;
            tx.shared_placements_queue_directory_work(&dir.to_string()).map_err(DatabaseError::operation)?;
        }
    }
    Ok(())
}

fn read_directory_intents(tx: &rusqlite::Transaction<'_>) -> Result<std::collections::HashMap<sync_common::DirId, DirectoryIntent>, DatabaseError> {
    let mut intents: std::collections::HashMap<sync_common::DirId, library_replica::DirectoryIntent> = std::collections::HashMap::new();
    let mut rows: Vec<(String, String, String, i64)> = Vec::new();
    tx.get_remote_directory_intents(|row| {
        rows.push((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?));
        Ok(())
    })
    .map_err(DatabaseError::operation)?;
    for (id_text, parent_text, name, lifecycle) in rows {
        let id = parse_uuid_column(id_text.clone(), 0).map_err(DatabaseError::operation)?;
        let parent_id = parse_uuid_column(parent_text, 1).map_err(DatabaseError::operation)?;
        let lifecycle = match lifecycle {
            0 => DirectoryLifecycleState::Present,
            1 => DirectoryLifecycleState::Deleted,
            2 => DirectoryLifecycleState::Purged,
            _ => return Err(DatabaseError::message("invalid directory lifecycle state")),
        };
        let name_version = read_version(tx, "directory_name", &id_text, "")?;
        let parent_version = read_version(tx, "directory_parent", &id_text, "")?;
        let lifecycle_version = read_version(tx, "directory_lifecycle", &id_text, "")?;
        if let (Some(name_version), Some(parent_version), Some(_)) = (name_version, parent_version, lifecycle_version) {
            let version = name_version.max(parent_version);
            intents.insert(id, DirectoryIntent { id, parent_id, name, lifecycle, version });
        }
    }
    Ok(intents)
}
