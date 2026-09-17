use crate::books::BooksSql;
use crate::browse::RebuildsSql as BrowseRebuildsSql;
use crate::file_work::queue_book_work;
use crate::file_work::queue_directory_work;
use crate::placements::PlacementsSql;
use crate::reading::ReadingSql;
use crate::storage_values::invalid_integer_column;
use crate::storage_values::parse_uuid_column;
use crate::storage_values::storage_i64;
use crate::sync_state::get_sync_metadata;
use crate::sync_state::set_sync_metadata;
use crate::sync_state::RemovedBookDir;
use crate::sync_state::StoredVersion;
use include_sqlite_sql::include_sql;
use book_model::ReadProgress;
use library_replica::DirectoryLifecycleState;
use book_model::BookFormat;
use library_replica::MutationBody;
use library_replica::VersionKey;
use library_replica::{project_directories, DirectoryIntent};
use rusqlite::OptionalExtension;
use rusqlite::Transaction;
use std::collections::HashMap;
use sync_common::ContentHash;
use sync_common::DirId;
use sync_common::FileName;
use sync_common::ServerMutation;

struct TxOriginGuard<'a> {
    tx: &'a Transaction<'a>,
    previous: String,
    active: bool,
}
impl<'a> TxOriginGuard<'a> {
    fn activate(tx: &'a Transaction<'a>) -> rusqlite::Result<Self> {
        let previous = get_sync_metadata(tx, "change_origin")?.unwrap_or_else(|| "local".into());
        set_sync_metadata(tx, "change_origin", "remote")?;
        Ok(Self { tx, previous, active: true })
    }
    fn finish(mut self) -> rusqlite::Result<()> {
        set_sync_metadata(self.tx, "change_origin", &self.previous)?;
        self.active = false;
        Ok(())
    }
}
impl Drop for TxOriginGuard<'_> {
    fn drop(&mut self) {
        if self.active {
            let _ = set_sync_metadata(self.tx, "change_origin", &self.previous);
        }
    }
}

pub fn apply_remote_change_in_tx(tx: &Transaction, change: &ServerMutation) -> Result<Option<RemovedBookDir>, Box<dyn std::error::Error>> {
    Ok(apply_remote_changes_in_tx(tx, std::slice::from_ref(change))?.pop())
}

/// Decoded wire values and state identities only. Never snapshots database
/// state: winning versions and lifecycle checks still run under the writer.
pub struct PreparedRemoteChanges<'a> {
    changes: Vec<(&'a ServerMutation, MutationBody, (&'static str, String, String))>,
}

pub fn prepare_remote_changes(changes: &[ServerMutation]) -> Result<PreparedRemoteChanges<'_>, Box<dyn std::error::Error>> {
    let mut prepared = Vec::with_capacity(changes.len());
    for change in changes {
        if change.mutation.kind == sync_common::mutation_kind::CATALOG_BOOK {
            log::debug!("Skipping retired catalog-book mutation");
            continue;
        }
        if let Some(body) = MutationBody::from_wire(&change.mutation)? {
            let identity = sync_state_identity(&body);
            prepared.push((change, body, identity));
        } else {
            log::debug!("Skipping unknown synchronized state kind {}", change.mutation.kind);
        }
    }
    Ok(PreparedRemoteChanges { changes: prepared })
}

/// Applies a page under one lazy origin guard; stale/unknown-only pages write no flag.
/// The caller owns the transaction and must roll back on any returned error.
pub fn apply_remote_changes_in_tx(tx: &Transaction, changes: &[ServerMutation]) -> Result<Vec<RemovedBookDir>, Box<dyn std::error::Error>> {
    apply_prepared_remote_changes_in_tx(tx, &prepare_remote_changes(changes)?)
}

pub fn apply_prepared_remote_changes_in_tx(tx: &Transaction, changes: &PreparedRemoteChanges<'_>) -> Result<Vec<RemovedBookDir>, Box<dyn std::error::Error>> {
    let mut origin = None;
    let result = (|| {
        let mut removed = Vec::new();
        if batching::independent_additions(changes) {
            batching::apply(tx, changes, &mut origin)?;
        } else if batching::independent_metadata(changes) {
            batching::apply_metadata(tx, changes, &mut origin)?;
        } else {
            for (change, body, identity) in &changes.changes {
                if let Some(value) = apply_remote_change_inner(tx, change, body, identity, &mut origin)? {
                    removed.push(value);
                }
            }
        }
        Ok(removed)
    })();
    if let Some(guard) = origin {
        guard.finish()?;
    }
    result
}

/// Applies one remote change. Returns the placement to delete locally when the
/// change was a removal that won the LWW gate; `None` otherwise.
fn apply_remote_change_inner<'a>(
    tx: &'a Transaction<'a>, change: &ServerMutation, body: &MutationBody, identity: &(&'static str, String, String), origin: &mut Option<TxOriginGuard<'a>>,
) -> Result<Option<RemovedBookDir>, Box<dyn std::error::Error>> {
    let (state_kind, state_key, state_subkey) = identity;
    if !sync_state_version_wins(tx, change, state_kind, &state_key, &state_subkey, None)? {
        return Ok(None);
    }

    if origin.is_none() {
        *origin = Some(TxOriginGuard::activate(tx)?);
    }
    let mut removed = None;
    let applied = match &body {
        MutationBody::DirectoryName { dir_id, name } => {
            apply_remote_directory_name(tx, dir_id, name)?;
            true
        }
        MutationBody::DirectoryParent { dir_id, parent_id } => {
            apply_remote_directory_parent(tx, dir_id, parent_id)?;
            true
        }
        MutationBody::DirectoryLifecycle { dir_id, value } => {
            apply_remote_directory_lifecycle(tx, dir_id, *value)?;
            true
        }
        MutationBody::BookLifecycle { content_hash, value: library_replica::BookLifecycleState::Present { metadata, added_at, format } } => {
            apply_remote_book_added(tx, content_hash.clone(), metadata, *added_at, *format, None)?;
            true
        }
        MutationBody::BookLifecycle { content_hash, value: library_replica::BookLifecycleState::Deleted { origin_folder_id } } => {
            apply_remote_book_deleted(tx, content_hash.clone(), change.mutation.changed_at, origin_folder_id.as_deref())?;
            true
        }
        MutationBody::BookLifecycle { content_hash, value: library_replica::BookLifecycleState::Purged } => {
            apply_remote_book_purged(tx, content_hash.clone(), change.mutation.changed_at)?;
            tx.execute(include_str!("sql/remote_reducer/apply_remote_change_inner_insert.sql"), [content_hash.as_str()])?;
            true
        }
        MutationBody::Placement { dir_id, content_hash, present, origin_folder_id } => {
            if !book_is_purged(tx, content_hash)? {
                apply_remote_book_dir(tx, change.mutation.changed_at, content_hash.clone(), dir_id, *present, origin_folder_id.as_deref())?;
                if !*present {
                    removed = Some(RemovedBookDir { dir_id: dir_id.clone(), content_hash: content_hash.clone() });
                }
            }
            true
        }
        MutationBody::ReadingPosition { content_hash, value } => {
            if !book_is_purged(tx, content_hash)? {
                ReadProgress::new(value.progress).map_err(|error| error.to_string())?;
                tx.first_book_insert(content_hash.as_str(), storage_i64(change.mutation.changed_at, "change timestamp")?)?;
                let location = value.location.as_str();
                tx.set_reading_position(content_hash.as_str(), &location, Some(value.progress))?;
            }
            true
        }
        MutationBody::Annotation { annotation_id, value } => {
            if !book_is_purged(tx, &value.content_hash)? {
                apply_remote_annotation(tx, change, annotation_id, value)?;
            }
            true
        }
        MutationBody::CatalogBook { .. } => unreachable!("catalog mutations are filtered before decoding"),
        MutationBody::PdfReaderMetadata { content_hash, value } => {
            if !book_is_purged(tx, content_hash)? {
                tx.first_book_insert(content_hash.as_str(), storage_i64(change.mutation.changed_at, "change timestamp")?)?;
                super::pdf_metadata::upsert(tx, content_hash, &ContentHash::new(&value.checksum), value)?;
            }
            true
        }
        MutationBody::BookToc { content_hash, value } => {
            if !book_is_purged(tx, content_hash)? {
                tx.first_book_insert(content_hash.as_str(), storage_i64(change.mutation.changed_at, "change timestamp")?)?;
                super::toc::upsert_book_toc(tx, content_hash, value)?;
            }
            true
        }
        MutationBody::Description { content_hash, value } => {
            if !book_is_purged(tx, content_hash)? {
                tx.first_book_insert(content_hash.as_str(), storage_i64(change.mutation.changed_at, "change timestamp")?)?;
                super::metadata::update_book_description(tx, *content_hash, value)?;
            }
            true
        }
        MutationBody::Metadata { content_hash, value } => {
            #[cfg(test)]
            let _profile = profiling::Span::new("metadata_edit");
            if !book_is_purged(tx, content_hash)? {
                tx.first_book_insert(content_hash.as_str(), storage_i64(change.mutation.changed_at, "change timestamp")?)?;
                super::metadata::update_book_metadata_state_preserving_ids(tx, content_hash.clone(), value)?;
            }
            true
        }
    };
    if applied {
        record_sync_state_version(tx, change, state_kind, &state_key, &state_subkey)?;
        #[cfg(not(target_arch = "wasm32"))]
        match &body {
            MutationBody::Placement { content_hash, .. } | MutationBody::BookLifecycle { content_hash, .. } => queue_book_work(tx, content_hash)?,
            _ => {}
        }
        if matches!(body, MutationBody::DirectoryName { .. } | MutationBody::DirectoryParent { .. } | MutationBody::DirectoryLifecycle { .. }) {
            reproject_directories(tx)?;
        }
    }
    Ok(removed)
}

fn book_is_purged(tx: &Transaction<'_>, content_hash: &ContentHash) -> Result<bool, rusqlite::Error> {
    tx.prepare_cached(include_str!("sql/remote_reducer/book_is_purged_select.sql"))?.query_row([content_hash.as_str()], |row| row.get(0))
}

fn apply_remote_annotation(tx: &Transaction, change: &ServerMutation, id: &str, value: &book_model::AnnotationState) -> Result<(), Box<dyn std::error::Error>> {
    tx.first_book_insert(value.content_hash.as_str(), storage_i64(change.mutation.changed_at, "change timestamp")?)?;
    let style = match value.style {
        book_model::AnnotationStyle::Highlight => "highlight",
        book_model::AnnotationStyle::Underline => "underline",
        book_model::AnnotationStyle::Squiggly => "squiggly",
        book_model::AnnotationStyle::Strikethrough => "strikethrough",
    };
    let app_anchor = book_model::AnnotationAnchor::from(value.anchor.clone());
    let anchor = super::marks::store_annotation_anchor(&app_anchor)?;
    let created_at = storage_i64(value.created_at, "annotation creation timestamp")?;
    let modified_at = storage_i64(value.modified_at, "annotation modification timestamp")?;
    tx.upsert_remote_annotation(
        id,
        value.content_hash.as_str(),
        anchor.kind,
        anchor.cfi.as_deref(),
        anchor.page,
        anchor.rects.as_deref(),
        anchor.fallback_cfi.as_deref(),
        &value.exact_text,
        style,
        &value.color,
        &value.note,
        created_at,
        modified_at,
        value.deleted.then_some(modified_at),
    )?;
    Ok(())
}

fn sync_state_identity(body: &MutationBody) -> (&'static str, String, String) {
    match body {
        MutationBody::DirectoryName { dir_id, .. } => ("directory_name", dir_id.to_string(), String::new()),
        MutationBody::DirectoryParent { dir_id, .. } => ("directory_parent", dir_id.to_string(), String::new()),
        MutationBody::DirectoryLifecycle { dir_id, .. } => ("directory_lifecycle", dir_id.to_string(), String::new()),
        MutationBody::BookLifecycle { content_hash, .. } => ("book_lifecycle", content_hash.to_string(), String::new()),
        MutationBody::Placement { dir_id, content_hash, .. } => ("placement", content_hash.to_string(), dir_id.to_string()),
        MutationBody::ReadingPosition { content_hash, .. } => ("reading_position", content_hash.to_string(), String::new()),
        MutationBody::Annotation { annotation_id, .. } => ("annotation", annotation_id.clone(), String::new()),
        MutationBody::CatalogBook { .. } => unreachable!("catalog mutations are filtered before decoding"),
        MutationBody::Metadata { content_hash, .. } => ("metadata", content_hash.to_string(), String::new()),
        MutationBody::PdfReaderMetadata { content_hash, value } => ("pdf_reader_metadata", content_hash.to_string(), value.checksum.clone()),
        MutationBody::Description { content_hash, .. } => ("description", content_hash.to_string(), String::new()),
        MutationBody::BookToc { content_hash, .. } => ("book_toc", content_hash.to_string(), String::new()),
    }
}

fn sync_state_version_wins(tx: &Transaction, change: &ServerMutation, kind: &str, key: &str, subkey: &str, snapshot: Option<&batching::BookSnapshot>) -> Result<bool, rusqlite::Error> {
    #[cfg(test)]
    let _profile = profiling::Span::new("version_check");
    let version = VersionKey::new(change.mutation.changed_at, change.mutation.conflict_rank, change.replica_id.clone(), change.mutation.replica_seq.get(), change.mutation.mutation_id);
    if let Some(snapshot) = snapshot {
        Ok(snapshot.version.as_ref().is_none_or(|stored| &version > stored))
    } else {
        Ok(read_sync_state_version(tx, kind, key, subkey)?.is_none_or(|stored| version > stored))
    }
}

fn record_sync_state_version(tx: &Transaction, change: &ServerMutation, kind: &str, key: &str, subkey: &str) -> Result<(), rusqlite::Error> {
    #[cfg(test)]
    let _profile = profiling::Span::new("version_write");
    tx.prepare_cached(include_str!("sql/remote_reducer/record_sync_state_version_insert.sql"))?
        .execute(rusqlite::params![
            kind,
            key,
            subkey,
            storage_i64(change.mutation.changed_at, "change timestamp")?,
            i64::from(change.mutation.conflict_rank),
            &change.replica_id.to_string(),
            storage_i64(change.mutation.replica_seq.get(), "actor sequence")?,
            &change.mutation.mutation_id.to_string(),
        ])
        .map(|_| ())
}

fn apply_remote_directory_name(tx: &Transaction, dir_id: &DirId, name: &FileName) -> Result<(), Box<dyn std::error::Error>> {
    ensure_directory_placeholder(tx, dir_id)?;
    tx.set_remote_directory_name(&dir_id.to_string(), name.as_str())?;
    Ok(())
}

fn apply_remote_directory_parent(tx: &Transaction, dir_id: &DirId, parent_id: &DirId) -> Result<(), Box<dyn std::error::Error>> {
    ensure_directory_placeholder(tx, parent_id)?;
    ensure_directory_placeholder(tx, dir_id)?;
    tx.set_remote_directory_parent(&dir_id.to_string(), &parent_id.to_string())?;
    Ok(())
}

fn apply_remote_directory_lifecycle(tx: &Transaction, dir_id: &DirId, value: DirectoryLifecycleState) -> Result<(), Box<dyn std::error::Error>> {
    ensure_directory_placeholder(tx, dir_id)?;
    let value = match value {
        DirectoryLifecycleState::Present => 0,
        DirectoryLifecycleState::Deleted => 1,
        DirectoryLifecycleState::Purged => 2,
    };
    tx.set_remote_directory_lifecycle(&dir_id.to_string(), value)?;
    Ok(())
}

/// A pull cursor advances page by page, so a child can legitimately arrive one
/// page before its parent. Keep a deleted placeholder to satisfy SQLite's
/// foreign key without exposing an incomplete hierarchy. Applying the real
/// directory state later replaces its intent and the projection activates the
/// complete parent-before-child path.
fn ensure_directory_placeholder(tx: &Transaction, dir_id: &DirId) -> Result<(), rusqlite::Error> {
    if *dir_id == sync_common::ROOT_DIR_ID {
        return Ok(());
    }
    tx.ensure_remote_directory_placeholder(&dir_id.to_string())?;
    Ok(())
}

fn read_directory_intents(tx: &Transaction) -> Result<HashMap<DirId, DirectoryIntent>, rusqlite::Error> {
    let mut intents = HashMap::new();
    tx.get_remote_directory_intents(|row| {
        let id_text = row.get::<_, String>(0)?;
        let parent_text = row.get::<_, String>(1)?;
        let id = parse_uuid_column(id_text.clone(), 0)?;
        let parent_id = parse_uuid_column(parent_text, 1)?;
        let name = row.get::<_, String>(2)?;
        let lifecycle = match row.get::<_, i64>(3)? {
            0 => DirectoryLifecycleState::Present,
            1 => DirectoryLifecycleState::Deleted,
            2 => DirectoryLifecycleState::Purged,
            _ => return Err(invalid_integer_column(3, "invalid directory lifecycle state")),
        };
        let name_version = read_sync_state_version(tx, "directory_name", &id_text, "")?;
        let parent_version = read_sync_state_version(tx, "directory_parent", &id_text, "")?;
        let lifecycle_version = read_sync_state_version(tx, "directory_lifecycle", &id_text, "")?;
        let (Some(name_version), Some(parent_version), Some(_)) = (name_version, parent_version, lifecycle_version) else { return Ok(()) };
        let version = name_version.max(parent_version);
        intents.insert(id, DirectoryIntent { id, parent_id, name, lifecycle, version });
        Ok(())
    })?;
    Ok(intents)
}

#[cfg(not(target_arch = "wasm32"))]
fn directory_projection_rows(tx: &Transaction) -> rusqlite::Result<std::collections::HashMap<String, (String, String, bool)>> {
    tx.prepare(include_str!("sql/remote_reducer/directory_projection_rows_select.sql"))?.query_map([], |row| Ok((row.get(0)?, (row.get(1)?, row.get(2)?, row.get(3)?))))?.collect()
}

fn reproject_directories(tx: &Transaction) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(not(target_arch = "wasm32"))]
    let before = directory_projection_rows(tx)?;
    let intents = read_directory_intents(tx)?;
    let projection = project_directories(&intents);

    tx.hide_remote_directories()?;
    let mut ids: Vec<_> = intents.keys().copied().collect();
    ids.sort();
    for id in &ids {
        let intent = &intents[id];
        let projected_parent = if projection.suppressed.contains(id) { sync_common::ROOT_DIR_ID } else { intent.parent_id };
        let projected_name = projection.names.get(id).unwrap_or(&intent.name);
        tx.project_remote_directory(&intent.id.to_string(), &projected_parent.to_string(), projected_name.as_str())?;
        if intent.lifecycle == DirectoryLifecycleState::Purged {
            tx.mark_remote_directory_purged(&intent.id.to_string())?;
        }
    }

    for id in projection.ordered_active {
        tx.activate_remote_directory(&id.to_string())?;
    }

    // Actual parent/lifecycle columns are projected in a batch while
    // change_origin is remote, so the per-row hierarchy trigger is suppressed.
    // Rebuild the derived ownership once after the final live tree is known.
    tx.rebuild_folder_book_ownership()?;
    #[cfg(not(target_arch = "wasm32"))]
    for (id, state) in directory_projection_rows(tx)? {
        if before.get(&id) != Some(&state) {
            queue_directory_work(tx, &DirId::parse_str(&id)?)?;
        }
    }
    Ok(())
}

fn read_sync_state_version(tx: &Transaction, kind: &str, key: &str, subkey: &str) -> Result<Option<VersionKey>, rusqlite::Error> {
    tx.prepare_cached(include_str!("sql/remote_reducer/read_sync_state_version_select.sql"))?.query_row(rusqlite::params![kind, key, subkey], |row| StoredVersion::decode_row(row, 0)).optional().map(Option::flatten)
}

fn apply_remote_book_added(
    tx: &Transaction, content_hash: ContentHash, metadata: &library_replica::SyncBookMetadata, added_at: sync_common::UnixMillis, format: BookFormat, known_metadata: Option<bool>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Lifecycle metadata seeds a new row (including placeholders created by a
    // placement or reader-state mutation), but it is not a second metadata
    // register. Preserve any title/author already owned by metadata state.
    #[cfg(test)]
    let profile = profiling::Span::new("metadata_presence");
    let has_metadata = if let Some(value) = known_metadata {
        value
    } else {
        let mut value = false;
        tx.remote_book_has_metadata(content_hash.as_str(), |row| {
            value = row.get(0)?;
            Ok(())
        })?;
        value
    };
    #[cfg(test)]
    drop(profile);
    #[cfg(test)]
    let profile = profiling::Span::new("book_row_write");
    crate::books::apply_remote_book_added(tx, &content_hash, &metadata.title, storage_i64(added_at, "book addition timestamp")?, format.canonical_extension())?;
    #[cfg(test)]
    drop(profile);
    if !has_metadata {
        #[cfg(test)]
        let _profile = profiling::Span::new("metadata_write");
        super::metadata::update_book_metadata_state_preserving_ids(tx, content_hash, metadata)?;
    }
    Ok(())
}

fn apply_remote_book_deleted(tx: &Transaction, content_hash: ContentHash, changed_at: sync_common::UnixMillis, origin_folder_id: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    tx.apply_remote_book_deleted(content_hash.as_str(), storage_i64(changed_at, "book deletion timestamp")?, origin_folder_id)?;
    Ok(())
}

fn apply_remote_book_purged(tx: &Transaction, content_hash: ContentHash, changed_at: sync_common::UnixMillis) -> Result<(), Box<dyn std::error::Error>> {
    tx.apply_remote_book_purged(content_hash.as_str(), storage_i64(changed_at, "book purge timestamp")?)?;
    let row_id: i64 = tx.query_row(include_str!("sql/remote_reducer/apply_remote_book_purged_select.sql"), [content_hash.as_str()], |row| row.get(0))?;
    for sql in [
        include_str!("sql/purge_book_publisher.sql"),
        include_str!("sql/purge_book_identifier.sql"),
        include_str!("sql/purge_book_language.sql"),
        include_str!("sql/purge_book_contributor.sql"),
        include_str!("sql/purge_book_subject_assignment_evidence.sql"),
        include_str!("sql/purge_book_subject_assignment.sql"),
        include_str!("sql/purge_book_subject.sql"),
        include_str!("sql/purge_book_external_entity.sql"),
        include_str!("sql/purge_external_metadata_attempt.sql"),
        include_str!("sql/purge_book_collection.sql"),
        include_str!("sql/purge_book_date.sql"),
        include_str!("sql/purge_book_value.sql"),
        include_str!("sql/purge_annotation.sql"),
    ] {
        tx.execute(sql, [row_id])?;
    }
    tx.execute(include_str!("sql/remote_reducer/apply_remote_book_purged_delete.sql"), [row_id])?;
    tx.execute(include_str!("sql/remote_reducer/apply_remote_book_purged_delete_2.sql"), [content_hash.as_str()])?;
    tx.execute(include_str!("sql/remote_reducer/apply_remote_book_purged_update.sql"), [row_id])?;
    tx.execute(include_str!("sql/remote_reducer/apply_remote_book_purged_delete_3.sql"), [row_id])?;
    Ok(())
}

pub(super) fn apply_remote_book_dir(tx: &Transaction, changed_at: sync_common::UnixMillis, content_hash: ContentHash, dir_id: &DirId, present: bool, origin_folder_id: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    let stored_content_hash = content_hash.to_string();
    ensure_directory_placeholder(tx, dir_id)?;
    tx.first_book_insert(&stored_content_hash, storage_i64(changed_at, "placement timestamp")?)?;
    if present {
        tx.apply_remote_book_dir_added(&dir_id.to_string(), &stored_content_hash)?;
    } else {
        tx.apply_remote_book_dir_removed(&dir_id.to_string(), &stored_content_hash, origin_folder_id)?;
    }
    Ok(())
}

include_sql!("src/remote_reducer/sql/remote_reducer.sql");

#[cfg(test)]
mod profiling;

mod batching;
