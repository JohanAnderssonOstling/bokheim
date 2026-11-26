//! Domain projections of canonical registers, shared by local commands and remote sync.
//! Only register admission publishes edits; projection writes disposable domain rows.

use crate::DatabaseError;
use crate::SqlParam;
use crate::annotations::AnnotationsSql;
use crate::books::audiobook::AudiobookSql;
use crate::books::toc::TocSql;
use crate::contributors::ContributorResolver;
use crate::import::ImportCommitSql;
use crate::purge::remove_book;
use crate::query_methods::{parse_uuid_column, query_rows, text_column};
#[cfg(not(target_arch = "wasm32"))]
use crate::shared_sql::SharedSql;
use crate::stored_document;
use crate::sync::SyncApplySql;
use crate::sync::{nonnegative_u64, normalized_description, storage_i64, toc_entry_count};
use client_platform_runtime::profiling::ProfileSpan;
use content_address::BookId;
use library_replica::{BookAddition, BookLifecycleState, DirectoryIntent, DirectoryLifecycleState, EnrichmentResult, LibraryStateEntry};
use rusqlite::Transaction;
use std::collections::{BTreeMap, HashMap, HashSet};
use sync_common::{DirId, ROOT_DIR_ID, StateKey, wire};

mod metadata;

/// Project canonical winners inside the same transaction as their admission.
pub(crate) fn project_dirty(tx: &Transaction<'_>) -> Result<(), DatabaseError> {
    project_dirty_with_contributors(tx, &mut ContributorResolver::default())
}

pub(crate) fn project_dirty_with_contributors(tx: &Transaction<'_>, contributors: &mut ContributorResolver) -> Result<(), DatabaseError> {
    let _profile = ProfileSpan::new("metadata_projection", "transaction");
    let dirty = query_rows(|callback| tx.projection_dirty_addresses(callback), |r| Ok((r.get::<_, String>(0)?, r.get::<_, StateKey>(1)?, r.get::<_, String>(2)?)))?;
    if dirty.is_empty() {
        return Ok(());
    }
    // Determine queue membership once, rather than scanning every dirty
    // address again for every projected book in a large sync response.
    #[cfg(not(target_arch = "wasm32"))]
    let queued_books = dirty
        .iter()
        .filter_map(|(kind, key, _)| match key {
            StateKey::Book(hash) if matches!(kind.as_str(), "book_add" | "book_lifecycle" | "placement") => Some(*hash),
            _ => None,
        })
        .collect::<HashSet<_>>();
    let directories = dirty.iter().any(|(kind, _, _)| kind.starts_with("directory_"));
    let mut books = BTreeMap::<BookId, HashSet<String>>::new();
    for (hash, kind) in query_rows(|callback| tx.projection_dirty_books(callback), |row| Ok((row.get::<_, BookId>(0)?, row.get::<_, String>(1)?)))? {
        books.entry(hash).or_default().insert(kind);
    }
    let directory_scope = if directories { Some(refresh_directory_intents(tx, &dirty)?) } else { None };
    let purged = if directories || dirty.iter().any(|(kind, _, _)| matches!(kind.as_str(), "placement" | "book_projection")) {
        let cached: bool = tx.query_row("SELECT valid FROM local_directory_cache WHERE singleton=1", [], |row| row.get(0))?;
        if directories || !cached {
            if !directories {
                refresh_directory_intents(tx, &dirty)?;
            }
            tx.execute("DELETE FROM local_purged_directory", [])?;
            tx.execute(
                "WITH RECURSIVE purged(id) AS (
                    SELECT id FROM local_directory_intent WHERE lifecycle=2 AND id!=?1
                    UNION SELECT d.id FROM local_directory_intent d JOIN purged p ON d.parent_id=p.id WHERE d.id!=?1
                ) INSERT INTO local_purged_directory SELECT id FROM purged",
                [ROOT_DIR_ID.to_string()],
            )?;
            tx.execute("UPDATE local_directory_cache SET valid=1 WHERE singleton=1", [])?;
        }
        tx.prepare_cached("SELECT id FROM local_purged_directory")?.query_map([], |row| row.get::<_, String>(0))?.map(|id| DirId::parse_str(&id?).map_err(DatabaseError::operation)).collect::<Result<HashSet<_>, _>>()?
    } else {
        HashSet::new()
    };
    if let Some(scope) = &directory_scope {
        // Only affected folders can restore memberships lost to a purge.
        let mut statement = tx.prepare_cached(
            "SELECT DISTINCT v.state_key FROM json_each(?1) affected
            CROSS JOIN sync_state_version v INDEXED BY sync_placement_directory ON v.state_subkey=affected.value AND v.state_kind='placement' AND v.body IS NOT NULL
            LEFT JOIN dir d ON d.id=v.state_subkey WHERE d.id IS NULL",
        )?;
        for hash in statement.query_map([scope], |row| row.get::<_, BookId>(0))? {
            books.entry(hash?).or_default().insert("placement".into());
        }
    }
    let loading = ProfileSpan::new("metadata_state_loading", "projection");
    let mut pending = Vec::with_capacity(books.len());
    for (hash, changed) in books {
        let full = changed.contains("book_projection");
        let kinds = serde_json::to_string(&changed).map_err(DatabaseError::operation)?;
        let rows = query_rows(|callback| tx.projection_book_state(&(&hash), full, &kinds, callback), |r| Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, i64>(1)?)))?;
        let values = rows
            .into_iter()
            .map(|(bytes, time)| Ok((wire::decode::<LibraryStateEntry>(&bytes, wire::MAX_DECODED_REQUEST_BYTES).map_err(DatabaseError::operation)?, nonnegative_u64(time, 1, "register timestamp")?)))
            .collect::<Result<Vec<_>, DatabaseError>>()?;
        pending.push((hash, changed, values));
    }
    // Bulk metadata projection updates the format too. Capture its old value
    // before that write so metadata-only additions can avoid file replay while
    // actual format changes still request it.
    #[cfg(not(target_arch = "wasm32"))]
    let previous_formats = pending
        .iter()
        .filter(|(hash, changed, _)| queued_books.contains(hash) && !changed.contains("book_lifecycle") && !changed.contains("placement"))
        .map(|(hash, _, _)| tx.projection_book_format(hash, |row| row.get::<_, Option<i64>>(0)).map(|format| (*hash, format)))
        .collect::<Result<HashMap<_, _>, _>>()?;
    drop(loading);
    // Canonical winners have already been admitted under this writer. Build
    // complete metadata sets, then write them together before reader documents.
    let tables = ProfileSpan::new("metadata_tables", "projection");
    metadata::project(
        tx,
        contributors,
        pending.iter().filter_map(|(hash, changed, values)| {
            let needs_metadata = changed.contains("book_projection") || changed.contains("book_add");
            (needs_metadata && !book_is_purged(values)).then(|| (hash, book_addition(values)))
        }),
    )?;
    drop(tables);
    let documents = ProfileSpan::new("metadata_documents", "projection");
    for (hash, changed, values) in pending {
        project_book(tx, &hash, &values, &changed, &purged)?;
        // OPFS stores one object per book and has no placement files to replay.
        // A native work row would otherwise block browser upload preparation.
        #[cfg(not(target_arch = "wasm32"))]
        // Enrichment can replace a book's addition without changing any file
        // facts. Only a changed format needs replay in that case; lifecycle
        // and placement changes retain their normal reconciliation policy.
        if queued_books.contains(&hash) {
            let requires_replay = match previous_formats.get(&hash) {
                Some(previous) => *previous != tx.projection_book_format(&hash, |row| row.get::<_, Option<i64>>(0))?,
                None => true,
            };
            if requires_replay {
                tx.queue_book_work_insert(&hash)?;
            }
        }
    }
    drop(documents);
    if directories {
        // Cache the canonical directory winners before deriving the display tree.
        for bytes in query_rows(|callback| tx.projection_directory_state(callback), |row| row.get::<_, Vec<u8>>(0))? {
            let body = wire::decode::<LibraryStateEntry>(&bytes, wire::MAX_DECODED_REQUEST_BYTES).map_err(DatabaseError::operation)?;
            project_value(tx, &body, &purged)?;
        }
        project_directory_tree(tx, &purged, directory_scope.as_deref().unwrap())?;
    }
    tx.projection_clear_dirty()?;
    Ok(())
}

/// Decode only admitted winners. The cache retains purged and incomplete
/// intents, which cannot be recovered from the disposable display table.
fn refresh_directory_intents(tx: &Transaction<'_>, dirty: &[(String, StateKey, String)]) -> Result<String, DatabaseError> {
    let ready: bool = tx.query_row("SELECT intents_valid FROM local_directory_cache WHERE singleton=1", [], |row| row.get(0))?;
    let seeds = dirty.iter().filter(|(kind, _, _)| kind.starts_with("directory_")).filter_map(|(_, key, _)| key.text()).collect::<Vec<_>>();
    let seeds = serde_json::to_string(&seeds).map_err(DatabaseError::operation)?;
    let descendants = |seeds: &str| -> Result<Vec<String>, DatabaseError> {
        Ok(tx
            .prepare_cached(
                "WITH RECURSIVE affected(id) AS (
            SELECT value FROM json_each(?1)
            UNION SELECT d.id FROM local_directory_intent d JOIN affected a ON d.parent_id=a.id WHERE d.id!=?2
            UNION SELECT d.id FROM dir d JOIN affected a ON d.parent_id=a.id WHERE d.id!=?2
        ) SELECT id FROM affected",
            )?
            .query_map((seeds, ROOT_DIR_ID.to_string()), |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?)
    };
    let mut affected = descendants(&seeds)?;
    // Old parents still participate in collision repair after a move.
    let old_parents =
        tx.prepare_cached("SELECT parent_id FROM local_directory_intent WHERE id IN (SELECT value FROM json_each(?1)) AND parent_id IS NOT NULL")?.query_map([&seeds], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
    let sql = if ready {
        "SELECT v.body FROM sync_projection_dirty d JOIN sync_state_version v USING(state_kind,state_key,state_subkey)
         WHERE d.state_kind IN ('directory_name','directory_parent','directory_lifecycle') AND v.body IS NOT NULL"
    } else {
        "SELECT body FROM sync_state_version WHERE state_kind IN ('directory_name','directory_parent','directory_lifecycle') AND body IS NOT NULL"
    };
    let values = tx.prepare_cached(sql)?.query_map([], |row| row.get::<_, Vec<u8>>(0))?.collect::<Result<Vec<_>, _>>()?;
    for bytes in values {
        let entry = wire::decode::<LibraryStateEntry>(&bytes, wire::MAX_DECODED_REQUEST_BYTES).map_err(DatabaseError::operation)?;
        match entry {
            LibraryStateEntry::DirectoryName { dir_id, name } => {
                tx.execute("INSERT INTO local_directory_intent(id,name) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET name=excluded.name", (dir_id.to_string(), name))?;
            }
            LibraryStateEntry::DirectoryParent { dir_id, parent_id } => {
                tx.execute("INSERT INTO local_directory_intent(id,parent_id) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET parent_id=excluded.parent_id", (dir_id.to_string(), parent_id.to_string()))?;
            }
            LibraryStateEntry::DirectoryLifecycle { dir_id, value } => {
                let value = match value {
                    DirectoryLifecycleState::Present => 0,
                    DirectoryLifecycleState::Deleted => 1,
                    DirectoryLifecycleState::Purged => 2,
                };
                tx.execute("INSERT INTO local_directory_intent(id,lifecycle) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET lifecycle=excluded.lifecycle", (dir_id.to_string(), value))?;
            }
            _ => return Err(DatabaseError::message("invalid canonical directory register")),
        }
    }
    tx.execute("UPDATE local_directory_cache SET intents_valid=1 WHERE singleton=1", [])?;
    if !ready {
        affected.extend(tx.prepare_cached("SELECT id FROM local_directory_intent UNION SELECT id FROM dir")?.query_map([], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?);
    } else {
        affected.extend(descendants(&seeds)?);
    }
    affected.extend(old_parents);
    let affected = serde_json::to_string(&affected).map_err(DatabaseError::operation)?;
    // Ancestors supply lifecycle/cycle context. Their siblings supply all name
    // reservations; unrelated sibling subtrees need no decoding or projection.
    // Include repaired root children, since a cycle repair can collide at root.
    let scope = tx
        .prepare_cached(
            "WITH RECURSIVE context(id) AS (
        SELECT value FROM json_each(?1)
        UNION SELECT id FROM local_directory_intent WHERE parent_id=?2
        UNION SELECT id FROM dir WHERE parent_id=?2 AND deleted_at IS NULL
        UNION SELECT d.parent_id FROM context c CROSS JOIN local_directory_intent d ON d.id=c.id WHERE d.parent_id IS NOT NULL AND d.id!=?2
        UNION SELECT d.parent_id FROM context c CROSS JOIN dir d ON d.id=c.id WHERE d.id!=?2
    ), parents(id) AS (
        SELECT d.parent_id FROM context c CROSS JOIN local_directory_intent d ON d.id=c.id
        UNION SELECT d.parent_id FROM context c CROSS JOIN dir d ON d.id=c.id
    ), scope(id) AS (
        SELECT id FROM context
        UNION SELECT d.id FROM parents p CROSS JOIN local_directory_intent d ON d.parent_id=p.id
        UNION SELECT d.id FROM parents p CROSS JOIN dir d ON d.parent_id=p.id WHERE d.deleted_at IS NULL
    ) SELECT id FROM scope WHERE id!=?2",
        )?
        .query_map((&affected, ROOT_DIR_ID.to_string()), |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    serde_json::to_string(&scope).map_err(DatabaseError::operation)
}

fn project_directory_tree(tx: &Transaction<'_>, purged: &HashSet<DirId>, scope: &str) -> Result<(), DatabaseError> {
    let before = tx
        .prepare_cached("SELECT d.id,d.parent_id,d.name,d.deleted_at IS NOT NULL FROM json_each(?1) affected JOIN dir d ON d.id=affected.value")?
        .query_map([scope], |r| Ok((parse_uuid_column(text_column(r, 0)?, 0)?, (parse_uuid_column(text_column(r, 1)?, 1)?, r.get::<_, String>(2)?, r.get::<_, bool>(3)?))))?
        .collect::<Result<HashMap<_, _>, _>>()?;
    let mut intents = HashMap::new();
    let rows = tx
        .prepare_cached(
            "SELECT d.id,d.parent_id,d.name,d.lifecycle FROM json_each(?1) affected JOIN local_directory_intent d ON d.id=affected.value
        WHERE d.parent_id IS NOT NULL AND d.name IS NOT NULL AND d.lifecycle IS NOT NULL",
        )?
        .query_map([scope], |row| Ok((parse_uuid_column(text_column(row, 0)?, 0)?, parse_uuid_column(text_column(row, 1)?, 1)?, row.get::<_, String>(2)?, row.get::<_, i64>(3)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    for (id, parent_id, name, lifecycle) in rows {
        let lifecycle = match lifecycle {
            0 => DirectoryLifecycleState::Present,
            1 => DirectoryLifecycleState::Deleted,
            _ => DirectoryLifecycleState::Purged,
        };
        intents.insert(id, DirectoryIntent { id, parent_id, name, lifecycle });
    }
    let projection = library_replica::project_directories(&intents);
    let expected: BTreeMap<_, _> = intents
        .iter()
        .filter(|(id, _)| !purged.contains(id))
        .map(|(id, intent)| {
            let suppressed = projection.suppressed.contains(id);
            let parent = if suppressed { ROOT_DIR_ID } else { projection.parents[id] };
            let name = projection.names.get(id).unwrap_or(&intent.name);
            (*id, (parent, name.clone(), suppressed))
        })
        .collect();
    // Local commands may already have installed the final display. In
    // particular, creating an empty folder must not invalidate every placement.
    let unchanged = before.iter().all(|(id, actual)| {
        if *id == ROOT_DIR_ID {
            return true;
        }
        if purged.contains(id) {
            return false;
        }
        expected.get(id).map_or(actual.2, |state| actual == state)
    });
    let unchanged = unchanged && expected.keys().all(|id| before.contains_key(id));
    if unchanged {
        return Ok(());
    }
    let changed = before
        .iter()
        .filter_map(|(id, actual)| (*id != ROOT_DIR_ID && expected.get(id) != Some(actual)).then_some(*id))
        .collect::<Vec<_>>();
    // Hide and detach changing edges before installing the final tree so
    // intermediate cycles and live-name collisions cannot escape the writer.
    for id in &changed {
        tx.execute("UPDATE dir SET deleted_at=1,parent_id=?2 WHERE id=?1", (id.to_string(), ROOT_DIR_ID.to_string()))?;
    }
    for id in purged {
        if before.contains_key(id) {
            let id = id.to_string();
            #[cfg(not(target_arch = "wasm32"))]
            tx.projection_queue_purged_directory_files(&id)?;
            tx.projection_remove_directory(&id)?;
            #[cfg(not(target_arch = "wasm32"))]
            tx.shared_placements_queue_directory_work(&id)?;
        }
    }
    for (id, (parent, name, _)) in &expected {
        if before.get(id) == expected.get(id) {
            continue;
        }
        let directory = id.to_string();
        tx.ensure_remote_directory_placeholder(&directory)?;
        {
            // A recreated ancestor can revive descendants whose disposable
            // rows were removed or replaced by placeholders after a purge.
            let intent = &intents[id];
            tx.set_remote_directory_name(&directory, &intent.name)?;
            tx.set_remote_directory_parent(&directory, &intent.parent_id.to_string())?;
            let lifecycle = match intent.lifecycle {
                DirectoryLifecycleState::Present => 0,
                DirectoryLifecycleState::Deleted => 1,
                DirectoryLifecycleState::Purged => 2,
            };
            tx.set_remote_directory_lifecycle(&directory, lifecycle)?;
        }
        tx.project_remote_directory(&directory, &parent.to_string(), name)?;
    }
    for id in projection.ordered_active {
        if before.get(&id) != expected.get(&id) {
            tx.activate_remote_directory(&id.to_string()).map_err(DatabaseError::operation)?;
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let changed = expected
            .iter()
            .filter_map(|(id, state)| (before.get(id) != Some(state)).then(|| id.to_string()))
            .collect::<Vec<_>>();
        let mut paths = crate::placements::FolderPaths::for_directories(tx, changed.iter().map(String::as_str))?;
        for id in changed {
            // Lifecycle changes do not relocate directories. Only materialize
            // a live directory whose desired path differs from its stored path.
            let Some(desired) = paths.folder(&id)? else { continue };
            let retained = tx.projection_directory_path_retained(&id, &desired, |row| row.get::<_, bool>(0))?;
            if retained {
                continue;
            }
            tx.shared_placements_queue_directory_work(&id)?;
        }
    }
    Ok(())
}

/// Project only changed state kinds. Synthetic book dirtiness requests a full rebuild.
fn book_is_purged(values: &[(LibraryStateEntry, u64)]) -> bool {
    values.iter().any(|(body, _)| matches!(body, LibraryStateEntry::BookLifecycle { value: BookLifecycleState::Purged, .. }))
}

fn book_addition(values: &[(LibraryStateEntry, u64)]) -> Option<&BookAddition> {
    values.iter().find_map(|(body, _)| match body {
        LibraryStateEntry::BookAdd { value, .. } => Some(value),
        _ => None,
    })
}

fn project_book(tx: &Transaction<'_>, hash: &BookId, values: &[(LibraryStateEntry, u64)], changed: &HashSet<String>, purged: &HashSet<DirId>) -> Result<(), DatabaseError> {
    if book_is_purged(values) {
        return remove_book(tx, hash);
    }
    if !tx.projection_book_exists(&hash, |row| row.get::<_, bool>(0))? {
        return Ok(());
    }
    let needs = |kind| changed.contains("book_projection") || changed.contains(kind);
    if needs("book_lifecycle") {
        let mut deleted_at = None;
        let mut origin = None;
        for (body, time) in values {
            if let LibraryStateEntry::BookLifecycle { value: BookLifecycleState::Deleted { origin_folder_id }, .. } = body {
                deleted_at = Some(storage_i64(*time, "deletion timestamp")?);
                origin = origin_folder_id.as_deref();
            }
        }
        let was_deleted: bool = tx.projection_book_deleted(&hash, |row| row.get(0))?;
        tx.projection_set_book_lifecycle(hash, deleted_at, origin)?;
        if deleted_at.is_some() {
            tx.projection_cancel_deleted_cover_work(&hash)?;
            tx.projection_clear_deleted_downloads(&hash)?;
        } else if was_deleted {
            tx.projection_restore_cover_work(&hash)?;
        }
    }
    if needs("reading_position") {
        let mut reading = None;
        let mut progress = 0.0;
        let mut read_at = None;
        for (body, time) in values {
            if let LibraryStateEntry::ReadingPosition { value, .. } = body {
                reading = Some(value.location.as_str());
                progress = value.progress;
                read_at = Some(storage_i64(*time, "reading timestamp")?);
            }
        }
        tx.projection_set_book_reading(hash, reading.as_deref(), progress, read_at)?;
    }
    if needs("book_add") {
        let addition = book_addition(values);
        if let Some(addition) = addition {
            project_addition(tx, hash, addition)?;
        } else {
            tx.projection_clear_description(&hash)?;
            tx.projection_clear_navigation(&hash)?;
            tx.projection_clear_pdf_metadata(&hash)?;
            tx.projection_clear_recording(&hash)?;
        }
    }
    if needs("book_content") || needs("book_add") {
        use crate::sync::apply::registers::canonical_body;
        if let Some((bytes, _)) = canonical_body(tx, "book_content", &StateKey::Book(*hash), "")? {
            if let LibraryStateEntry::BookContent { pdf, .. } = wire::decode::<LibraryStateEntry>(&bytes, wire::MAX_DECODED_REQUEST_BYTES).map_err(DatabaseError::operation)? {
                if let Some(pdf) = pdf {
                    let bytes = stored_document::encode(&pdf)?;
                    tx.pdf_reader_metadata_upsert(hash, &bytes)?;
                }
            }
        }
    }
    if changed.contains("book_projection") {
        tx.projection_trim_annotations(&hash)?;
    } else if needs("annotation") {
        tx.projection_trim_dirty_annotations(&hash)?;
    }
    if needs("placement") {
        tx.projection_trim_placements(&hash)?;
    }
    for (body, _) in values {
        if matches!(body, LibraryStateEntry::Annotation { .. } | LibraryStateEntry::Placement { .. }) {
            project_value(tx, body, purged)?;
        }
    }
    if needs("book_add") || needs("book_content") || (needs("annotation") && values.iter().any(|(body, _)| matches!(body,LibraryStateEntry::Annotation { value } if value.id.starts_with("pdf-import:")))) {
        crate::notebook::project_pdf(tx, *hash)?;
    }
    Ok(())
}

fn project_value(tx: &Transaction<'_>, body: &LibraryStateEntry, purged: &HashSet<DirId>) -> Result<(), DatabaseError> {
    if let LibraryStateEntry::DirectoryName { dir_id, .. } | LibraryStateEntry::DirectoryParent { dir_id, .. } | LibraryStateEntry::DirectoryLifecycle { dir_id, .. } | LibraryStateEntry::Placement { dir_id, .. } = body {
        if purged.contains(dir_id) {
            return Ok(());
        }
        if *dir_id != ROOT_DIR_ID {
            tx.ensure_remote_directory_placeholder(&dir_id.to_string())?;
        }
    }
    match body {
        LibraryStateEntry::DirectoryName { dir_id, name } => {
            tx.set_remote_directory_name(&dir_id.to_string(), name.as_str()).map_err(DatabaseError::operation)?;
        }
        LibraryStateEntry::DirectoryParent { dir_id, parent_id } => {
            if *parent_id != ROOT_DIR_ID && !purged.contains(parent_id) {
                tx.ensure_remote_directory_placeholder(&parent_id.to_string()).map_err(DatabaseError::operation)?;
            }
            tx.set_remote_directory_parent(&dir_id.to_string(), &parent_id.to_string()).map_err(DatabaseError::operation)?;
        }
        LibraryStateEntry::DirectoryLifecycle { dir_id, value } => {
            let value = match value {
                DirectoryLifecycleState::Present => 0,
                DirectoryLifecycleState::Deleted => 1,
                DirectoryLifecycleState::Purged => 2,
            };
            tx.set_remote_directory_lifecycle(&dir_id.to_string(), value).map_err(DatabaseError::operation)?;
        }
        LibraryStateEntry::BookLifecycle { .. } | LibraryStateEntry::ReadingPosition { .. } | LibraryStateEntry::ChapterReadStatus { .. } | LibraryStateEntry::BookAdd { .. } | LibraryStateEntry::BookContent { .. } => {
            unreachable!("book records and scalar registers are projected together")
        }
        LibraryStateEntry::Placement { dir_id, book_id, present, origin_folder_id } => {
            if *present {
                // Filenames are a local projection. A tombstoned placement's
                // old name may have been reused since it was removed.
                let directory = dir_id.to_string();
                let mut statement = tx.prepare_cached(include_str!("projection/placement_name.sql"))?;
                let (requested, conflict): (String, bool) = statement.query_row(rusqlite::params![directory, book_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
                drop(statement);
                let chosen = if conflict {
                    let occupied = query_rows(|callback| tx.projection_other_placement_names(&directory, &(&book_id), callback), |row| row.get::<_, String>(0))?;
                    // Use a short fallback when the old name conflicts or fills
                    // the filesystem's component byte limit.
                    library_replica::unique_file_name("Untitled", occupied.iter().map(String::as_str))
                } else {
                    requested
                };
                tx.apply_remote_book_dir_added(&directory, book_id, &chosen).map_err(DatabaseError::operation)?;
            } else {
                tx.apply_remote_book_dir_removed(&dir_id.to_string(), book_id, origin_folder_id.as_deref()).map_err(DatabaseError::operation)?;
            }
        }
        LibraryStateEntry::Annotation { value } => {
            let modified_at = storage_i64(value.modified_at, "annotation modification timestamp").map_err(DatabaseError::operation)?;
            let body = value.encode().map_err(DatabaseError::operation)?;
            tx.upsert_annotation(value.id.as_str(), &value.book_id, value.toc_ordinal, value.progress, body.as_slice(), modified_at, value.deleted.then_some(modified_at)).map_err(DatabaseError::operation)?;
            crate::annotations::project_recurrence(tx, value)?;
            crate::notebook::project_annotation(tx, value)?;
        }
    }
    Ok(())
}

/// Project the finalized record. Enrichment was already applied to it before
/// publication, or while decoding a historical two-record addition.
fn project_addition(tx: &Transaction<'_>, hash: &BookId, addition: &BookAddition) -> Result<(), DatabaseError> {
    let enriched_entries = match &addition.enrichment {
        Some(EnrichmentResult::Found { toc, .. }) => toc.as_deref(),
        _ => None,
    };
    tx.set_description(hash, &normalized_description(&addition.record.description)).map_err(DatabaseError::operation)?;

    if let Some(recording) = &addition.recording {
        let json = serde_json::to_string(recording).map_err(DatabaseError::operation)?;
        tx.audiobook_record_recording(hash, &json)?;
    } else {
        tx.projection_clear_recording(&hash)?;
    }
    if let Some(pdf) = &addition.pdf {
        let bytes = stored_document::encode(pdf)?;
        tx.pdf_reader_metadata_upsert(hash, &bytes).map_err(DatabaseError::operation)?;
    } else {
        tx.projection_clear_pdf_metadata(&hash)?;
    }
    if let Some(navigation) = &addition.navigation {
        // Provider entries apply only to audio navigation. Duration and track
        // offsets always come from the file, including when entries are empty.
        let entries = if navigation.duration_ms.is_some() { enriched_entries.unwrap_or(&navigation.entries) } else { &navigation.entries };
        let tracks = navigation.tracks.as_ref().map(stored_document::encode).transpose()?;
        let duration_ms = navigation.duration_ms.map(i64::try_from).transpose().map_err(DatabaseError::operation)?;
        let entry_count = i64::try_from(toc_entry_count(entries)).map_err(DatabaseError::operation)?;
        let toc = stored_document::encode(entries)?;
        tx.navigation_upsert(hash, entry_count, &toc, duration_ms, tracks.as_deref())?;
    } else {
        tx.projection_clear_navigation(&hash)?;
    }
    Ok(())
}

/// Local filenames are projection inputs, not synchronized
/// registers. Seed local values, then admit membership
/// through the same canonical path used by directory commands.
pub(crate) fn place_book(tx: &Transaction<'_>, directory: &DirId, hash: &BookId, name: &str) -> Result<(), DatabaseError> {
    tx.projection_upsert_placement(&(directory.to_string()), &(&hash), &name)?;
    crate::sync::apply::registers::record_local(tx, LibraryStateEntry::Placement { dir_id: *directory, book_id: *hash, present: true, origin_folder_id: None })
}

include_sqlite_sql::include_sql!("src/sql/projection.sql");

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Database;
    use crate::sql_fixtures::FixturesSql;
    use crate::sync::apply::registers::record_local;
    use crate::taxonomy_update::TaxonomySql;
    use book_model::{BookFormat, BookRecord, BookSubject};
    use library_replica::BookFacts;

    #[test]
    fn purge_projection_follows_deep_parents_and_seeded_cycles_without_crossing_root() {
        let db = Database::open(":memory:").unwrap();
        db.initialize_library().unwrap();
        let chain: Vec<_> = (1..=128).map(DirId::from_u128).collect();
        let id = DirId::from_u128;
        let mut parents: Vec<_> = chain.iter().enumerate().map(|(index, &directory)| (directory, if index == 0 { ROOT_DIR_ID } else { chain[index - 1] })).collect();
        parents.extend([(id(201), id(202)), (id(202), id(203)), (id(203), id(201)), (id(204), id(202)), (id(301), id(302)), (id(302), id(301)), (id(401), ROOT_DIR_ID), (id(402), id(401)), (id(501), chain[31]), (id(502), id(501))]);
        db.write(|tx| {
            for &(directory, parent) in parents.iter().rev() {
                for value in [
                    LibraryStateEntry::DirectoryName { dir_id: directory, name: format!("D{}", directory.as_u128()) },
                    LibraryStateEntry::DirectoryParent { dir_id: directory, parent_id: parent },
                    LibraryStateEntry::DirectoryLifecycle { dir_id: directory, value: DirectoryLifecycleState::Present },
                ] {
                    record_local(tx, value)?;
                }
            }
            Ok(())
        })
        .unwrap();
        let exists = |directory: DirId| db.connection.query_row("SELECT EXISTS(SELECT 1 FROM dir WHERE id=?1)", [directory.to_string()], |row| row.get::<_, bool>(0)).unwrap();
        assert!(parents.iter().all(|(directory, _)| exists(*directory)));
        db.write(|tx| {
            for directory in [chain[0], id(201)] {
                record_local(tx, LibraryStateEntry::DirectoryLifecycle { dir_id: directory, value: DirectoryLifecycleState::Purged })?;
            }
            // Even malformed retained root ancestry must not carry a purge
            // through the root into every unrelated branch.
            record_local(tx, LibraryStateEntry::DirectoryParent { dir_id: ROOT_DIR_ID, parent_id: chain[127] })
        })
        .unwrap();
        let removed: Vec<_> = chain.iter().copied().chain([id(201), id(202), id(203), id(204), id(501), id(502)]).collect();
        assert!(removed.iter().all(|directory| !exists(*directory)));
        for directory in [ROOT_DIR_ID, id(301), id(302), id(401), id(402)] {
            assert!(exists(directory));
        }
        for directory in [chain[127], id(202), id(204), id(502)] {
            assert_eq!(crate::placements::canonical_directory(&db.connection, &directory).unwrap().lifecycle, Some(DirectoryLifecycleState::Present), "descendants are suppressed by ancestry without authoring new lifecycle values");
        }
        db.write(|tx| record_local(tx, LibraryStateEntry::DirectoryName { dir_id: chain[127], name: "Retained hidden name".into() })).unwrap();
        assert!(removed.iter().all(|directory| !exists(*directory)), "later field projection must not recreate a purged descendant");
        assert!(exists(ROOT_DIR_ID));
        assert!(db.connection.is_autocommit());
    }

    #[test]
    fn cached_placement_lookup_retains_names_and_checks_only_other_live_books() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.create_scalar_function("portable_name_key", 1, rusqlite::functions::FunctionFlags::SQLITE_UTF8 | rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC, |context| {
            Ok(library_replica::portable_name_key(&context.get::<String>(0)?))
        })
        .unwrap();
        conn.execute_batch(include_str!("sync/sql/schema/tables.sql")).unwrap();
        conn.execute_batch(include_str!("placements/sql/schema/tables.sql")).unwrap();
        let directory = ROOT_DIR_ID.to_string();
        conn.execute("INSERT INTO dir(id,parent_id,name) VALUES(?1,?1,'Root')", [&directory]).unwrap();
        let hash = BookId::new(&"a".repeat(64));
        let other = BookId::new(&"b".repeat(64));
        for hash in [hash, other] {
            conn.execute("INSERT INTO book(content_hash) VALUES(?1)", [hash]).unwrap();
        }
        let lookup = || conn.prepare_cached(include_str!("projection/placement_name.sql")).unwrap().query_row(rusqlite::params![directory, hash], |row| Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?))).unwrap();
        assert_eq!(lookup(), ("Untitled".into(), false));
        conn.execute("INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash) SELECT ?1,row_id,'UNTITLED',NULL FROM book WHERE content_hash=?2", rusqlite::params![directory, other]).unwrap();
        assert_eq!(lookup(), ("Untitled".into(), true));
        conn.execute("UPDATE book_dir SET deleted_at=1", []).unwrap();
        assert_eq!(lookup(), ("Untitled".into(), false));
        conn.execute("INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,deleted_at) SELECT ?1,row_id,'Retained',NULL,1 FROM book WHERE content_hash=?2", rusqlite::params![directory, hash]).unwrap();
        assert_eq!(lookup(), ("Retained".into(), false));
        conn.execute("UPDATE book_dir SET file_name='retained',deleted_at=NULL WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=?1)", [other]).unwrap();
        assert_eq!(lookup(), ("Retained".into(), true));
    }

    #[test]
    fn book_record_lookup_uses_the_existing_primary_key() {
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        connection.execute_batch(include_str!("sync/sql/schema/tables.sql")).unwrap();
        let target = BookId::new(&format!("{:064x}", 128));
        for number in 1..=256 {
            let hash = BookId::new(&format!("{number:064x}"));
            connection.execute("INSERT INTO book(content_hash) VALUES(?1)", [&hash]).unwrap();
            connection.execute("INSERT INTO sync_state_version(state_kind,state_key,state_subkey,changed_at,conflict_rank,replica_id,replica_seq,body,book_key) VALUES('book_add',?1,'',0,0,'',0,?2,?1)", rusqlite::params![hash, number.to_string().as_bytes()]).unwrap();
        }
        for (full, kinds, expected) in [(true, "[]", 1), (false, "[\"book_add\"]", 1), (false, "[\"reading_position\"]", 0)] {
            let mut bodies = Vec::new();
            connection
                .projection_book_state(&target, full, kinds, |row| {
                    bodies.push(row.get::<_, Vec<u8>>(0)?);
                    assert_eq!(crate::query_methods::active_query_fullscan_steps(&connection), 0);
                    Ok(())
                })
                .unwrap();
            assert_eq!(bodies.len(), expected);
            if expected != 0 {
                assert_eq!(bodies[0], b"128");
            }
        }
    }

    #[test]
    fn addition_and_enrichment_project_only_the_final_metadata() {
        let db = Database::open(":memory:").unwrap();
        db.initialize_library().unwrap();
        let hash = BookId::new(&"e".repeat(64));
        db.connection.fixtures_insert_book(&hash).unwrap();
        db.connection.fixtures_create_transaction_probe().unwrap();
        db.connection.fixtures_track_subject_projection_writes().unwrap();
        let mut original = BookRecord { metadata: Default::default(), description: String::new() };
        original.title = "File title".into();
        original.book.subjects = vec![BookSubject::new(None, "Original", "file", None, None).unwrap()];
        let mut enriched = BookRecord { metadata: Default::default(), description: String::new() };
        enriched.title = "Provider title".into();
        enriched.book.subjects = vec![BookSubject::new(None, "Enriched", "provider", None, None).unwrap()];
        let mut addition = BookAddition { facts: BookFacts { added_at: 1, format: BookFormat::Epub }, record: original, navigation: None, pdf: None, recording: None, enrichment: None };
        addition.apply_enrichment(enriched, EnrichmentResult::Found { toc: None, cover_id: None });
        db.write(|tx| {
            record_local(tx, LibraryStateEntry::BookAdd { book_id: hash, value: addition })?;
            place_book(tx, &ROOT_DIR_ID, &hash, "book.epub")
        })
        .unwrap();
        assert_eq!(db.connection.fixtures_read_transaction_probe(|row| row.get::<_, i64>(0)).unwrap(), 1);
        assert_eq!(db.browse_book_detail(&hash).unwrap().book.title, "File title");
        let book = db.connection.fixtures_read_book_row_id(&hash, |row| row.get::<_, i64>(0)).unwrap();
        let subjects = query_rows(|callback| db.connection.taxonomy_book_subjects(&book, callback), |row| row.get::<_, String>(0)).unwrap();
        assert_eq!(subjects, ["Enriched"]);
        db.connection.fixtures_guard_non_reading_projection_writes().unwrap();
        db.update_reading_position(&hash, "epubcfi(/6/2)", Some(0.25), Some("Chapter")).unwrap();
        assert_eq!(db.reading_position(&hash).unwrap().as_deref(), Some("epubcfi(/6/2)"));
        assert_eq!(db.browse_book_detail(&hash).unwrap().book.title, "File title");
        assert_eq!(db.connection.fixtures_read_transaction_probe(|row| row.get::<_, i64>(0)).unwrap(), 1);

        db.connection.fixtures_drop_non_reading_projection_guards().unwrap();
        db.write(|tx| {
            tx.pull_mark_book_dirty(&hash)?;
            Ok(())
        })
        .unwrap();
        assert_eq!(db.reading_position(&hash).unwrap().as_deref(), Some("epubcfi(/6/2)"));
        assert_eq!(db.connection.fixtures_read_transaction_probe(|row| row.get::<_, i64>(0)).unwrap(), 1, "unchanged projection must not rewrite metadata");
    }
}
