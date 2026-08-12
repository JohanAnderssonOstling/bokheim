use super::*;
use library_model::{BookFormat, DirectoryLifecycleState};
use library_replica::{project_directories, DirectoryIntent};

/// Applies one remote change. Returns the placement to delete locally when the
/// change was a removal that won the LWW gate; `None` otherwise.
pub fn apply_remote_change_in_tx(tx: &Transaction, change: &ServerMutation) -> Result<Option<RemovedBookDir>, Box<dyn std::error::Error>> {
    let Some(body) = MutationBody::from_wire(&change.mutation)? else {
        log::debug!("Skipping unknown synchronized state kind {}", change.mutation.kind);
        return Ok(None);
    };
    let (state_kind, state_key, state_subkey) = sync_state_identity(&body);
    if !sync_state_version_wins(tx, change, state_kind, &state_key, &state_subkey)? {
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
        MutationBody::BookLifecycle { content_hash, value: library_model::BookLifecycleState::Present { metadata, added_at, format } } => {
            apply_remote_book_added(tx, content_hash.clone(), metadata, *added_at, *format)?;
            true
        }
        MutationBody::BookLifecycle { content_hash, value: library_model::BookLifecycleState::Deleted { origin_folder_id } } => {
            apply_remote_book_deleted(tx, content_hash.clone(), change.mutation.changed_at, origin_folder_id.as_deref())?;
            true
        }
        MutationBody::BookLifecycle { content_hash, value: library_model::BookLifecycleState::Purged } => {
            apply_remote_book_purged(tx, content_hash.clone(), change.mutation.changed_at)?;
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
                crate::ReadProgress::new(value.progress).map_err(|error| error.to_string())?;
                tx.first_book_insert(content_hash.as_str(), storage_i64(change.mutation.changed_at, "change timestamp")?)?;
                let location = value.location.as_str();
                tx.update_reading_position(content_hash.as_str(), &location, value.progress)?;
            }
            true
        }
        MutationBody::Annotation { annotation_id, value } => {
            if !book_is_purged(tx, &value.content_hash)? {
                apply_remote_annotation(tx, change, annotation_id, value)?;
            }
            true
        }
        MutationBody::Bookmark { bookmark_id, value } => {
            if !book_is_purged(tx, &value.content_hash)? {
                apply_remote_bookmark(tx, change, bookmark_id, value)?;
            }
            true
        }
        MutationBody::BookLink { link_id, value } => {
            if !book_is_purged(tx, &value.source_content_hash)? && !book_is_purged(tx, &value.target_content_hash)? {
                apply_remote_book_link(tx, change, link_id, value)?;
            }
            true
        }
        MutationBody::CatalogPublication { source_id, publication, content_hash } => {
            if !book_is_purged(tx, content_hash)? {
                tx.first_book_insert(content_hash.as_str(), storage_i64(change.mutation.changed_at, "change timestamp")?)?;
                super::catalog::record_catalog_publication(tx, source_id, publication, content_hash)?;
            }
            true
        }
        MutationBody::Metadata { content_hash, value } => {
            if !book_is_purged(tx, content_hash)? {
                tx.first_book_insert(content_hash.as_str(), storage_i64(change.mutation.changed_at, "change timestamp")?)?;
                super::metadata::update_book_metadata_state_preserving_ids(tx, content_hash.clone(), value)?;
            }
            true
        }
    };
    if applied {
        record_sync_state_version(tx, change, state_kind, &state_key, &state_subkey)?;
        if matches!(body, MutationBody::DirectoryName { .. } | MutationBody::DirectoryParent { .. } | MutationBody::DirectoryLifecycle { .. }) {
            reproject_directories(tx)?;
        }
    }
    Ok(removed)
}

fn book_is_purged(tx: &Transaction<'_>, content_hash: &ContentHash) -> Result<bool, rusqlite::Error> {
    tx.query_row("SELECT COALESCE((SELECT purged_at IS NOT NULL FROM book WHERE content_hash=?1),0)", [content_hash.as_str()], |row| row.get(0))
}

fn apply_remote_annotation(tx: &Transaction, change: &ServerMutation, id: &str, value: &library_model::AnnotationState) -> Result<(), Box<dyn std::error::Error>> {
    tx.first_book_insert(value.content_hash.as_str(), storage_i64(change.mutation.changed_at, "change timestamp")?)?;
    let style = match value.style {
        library_model::AnnotationStyle::Highlight => "highlight",
        library_model::AnnotationStyle::Underline => "underline",
        library_model::AnnotationStyle::Squiggly => "squiggly",
        library_model::AnnotationStyle::Strikethrough => "strikethrough",
    };
    let app_anchor = app_contract::reader_state::AnnotationAnchor::from(value.anchor.clone());
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

fn apply_remote_bookmark(tx: &Transaction, change: &ServerMutation, id: &str, value: &library_model::BookmarkState) -> Result<(), Box<dyn std::error::Error>> {
    tx.first_book_insert(value.content_hash.as_str(), storage_i64(change.mutation.changed_at, "change timestamp")?)?;
    let created_at = storage_i64(value.created_at, "bookmark creation timestamp")?;
    let modified_at = storage_i64(value.modified_at, "bookmark modification timestamp")?;
    tx.upsert_remote_bookmark(id, value.content_hash.as_str(), &value.cfi, &value.label, created_at, modified_at, value.deleted.then_some(modified_at))?;
    Ok(())
}

fn apply_remote_book_link(tx: &Transaction, change: &ServerMutation, id: &str, value: &library_model::BookLinkState) -> Result<(), Box<dyn std::error::Error>> {
    let changed_at = storage_i64(change.mutation.changed_at, "change timestamp")?;
    tx.first_book_insert(value.source_content_hash.as_str(), changed_at)?;
    tx.first_book_insert(value.target_content_hash.as_str(), changed_at)?;
    let created_at = storage_i64(value.created_at, "book-link creation timestamp")?;
    let modified_at = storage_i64(value.modified_at, "book-link modification timestamp")?;
    tx.upsert_remote_book_link(id, value.source_content_hash.as_str(), value.target_content_hash.as_str(), &value.relation, &value.note, created_at, modified_at, value.deleted.then_some(modified_at))?;
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
        MutationBody::Bookmark { bookmark_id, .. } => ("bookmark", bookmark_id.clone(), String::new()),
        MutationBody::BookLink { link_id, .. } => ("book_link", link_id.clone(), String::new()),
        MutationBody::CatalogPublication { source_id, publication, .. } => ("catalog_publication", source_id.as_str().to_owned(), catalog_publication_state_subkey(&publication.edition_id, publication.work_id.as_deref())),
        MutationBody::Metadata { content_hash, .. } => ("metadata", content_hash.to_string(), String::new()),
    }
}

fn sync_state_version_wins(tx: &Transaction, change: &ServerMutation, kind: &str, key: &str, subkey: &str) -> Result<bool, rusqlite::Error> {
    let version = VersionKey::new(change.mutation.changed_at, change.mutation.conflict_rank, change.replica_id.clone(), change.mutation.replica_seq.get(), change.mutation.mutation_id);
    Ok(read_sync_state_version(tx, kind, key, subkey)?.is_none_or(|stored| version > stored))
}

fn record_sync_state_version(tx: &Transaction, change: &ServerMutation, kind: &str, key: &str, subkey: &str) -> Result<(), rusqlite::Error> {
    tx.upsert_sync_state_version(
        kind,
        key,
        subkey,
        storage_i64(change.mutation.changed_at, "change timestamp")?,
        i64::from(change.mutation.conflict_rank),
        &change.replica_id.to_string(),
        storage_i64(change.mutation.replica_seq.get(), "actor sequence")?,
        &change.mutation.mutation_id.to_string(),
    )
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

fn reproject_directories(tx: &Transaction) -> Result<(), Box<dyn std::error::Error>> {
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

    Ok(())
}

fn read_sync_state_version(tx: &Transaction, kind: &str, key: &str, subkey: &str) -> Result<Option<VersionKey>, rusqlite::Error> {
    let mut version = None;
    tx.get_sync_state_version(kind, key, subkey, |row| {
        version = StoredVersion::decode_row(row, 0)?;
        Ok(())
    })?;
    Ok(version)
}

fn apply_remote_book_added(tx: &Transaction, content_hash: ContentHash, metadata: &library_model::BookMetadataState, added_at: sync_common::UnixMillis, format: BookFormat) -> Result<(), Box<dyn std::error::Error>> {
    // Lifecycle metadata seeds a new row (including placeholders created by a
    // placement or reader-state mutation), but it is not a second metadata
    // register. Preserve any title/author already owned by metadata state.
    let mut has_metadata = false;
    tx.remote_book_has_metadata(content_hash.as_str(), |row| {
        has_metadata = row.get(0)?;
        Ok(())
    })?;
    tx.apply_remote_book_added(content_hash.as_str(), &metadata.title, storage_i64(added_at, "book addition timestamp")?, format.canonical_extension())?;
    if !has_metadata {
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
    let row_id: i64 = tx.query_row("SELECT row_id FROM book WHERE content_hash=?1", [content_hash.as_str()], |row| row.get(0))?;
    for table in [
        "book_publisher",
        "book_identifier",
        "book_language",
        "book_contributor",
        "book_subject_assignment_evidence",
        "book_subject_assignment",
        "book_subject",
        "external_metadata_attempt",
        "book_collection",
        "book_publication_date",
        "book_publication_value",
        "catalog_publication_book",
        "annotation",
        "bookmark",
    ] {
        tx.execute(&format!("DELETE FROM {table} WHERE book_row_id=?1"), [row_id])?;
    }
    tx.execute("DELETE FROM book_link WHERE source_book_row_id=?1 OR target_book_row_id=?1", [row_id])?;
    tx.execute("DELETE FROM book_dir WHERE book_row_id=?1", [row_id])?;
    tx.execute("DELETE FROM audiobook_metadata WHERE content_hash=?1", [content_hash.as_str()])?;
    tx.execute("UPDATE book SET title=NULL,description='',description_scanned=0,publication_metadata=X'',format='',hidden_at=NULL,read_pos=NULL,read_progress=0,added_at=NULL,trash_origin_dir_id=NULL WHERE row_id=?1", [row_id])?;
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
