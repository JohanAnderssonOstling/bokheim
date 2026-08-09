use super::*;
use domain::BookFormat;

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
        MutationBody::DirectoryLifecycle { dir_id, deleted } => {
            apply_remote_directory_lifecycle(tx, dir_id, *deleted)?;
            true
        }
        MutationBody::BookLifecycle { content_hash, value: domain::BookLifecycleState::Present { metadata, added_at, format } } => {
            apply_remote_book_added(tx, content_hash.clone(), metadata, *added_at, *format)?;
            true
        }
        MutationBody::BookLifecycle { content_hash, value: domain::BookLifecycleState::Deleted } => {
            apply_remote_book_deleted(tx, content_hash.clone(), change.mutation.changed_at)?;
            true
        }
        MutationBody::Placement { dir_id, content_hash, present } => {
            apply_remote_book_dir(tx, change.mutation.changed_at, content_hash.clone(), dir_id, *present)?;
            if !*present {
                removed = Some(RemovedBookDir { dir_id: dir_id.clone(), content_hash: content_hash.clone() });
            }
            true
        }
        MutationBody::ReadingPosition { content_hash, value } => {
            crate::ReadProgress::new(value.progress).map_err(|error| error.to_string())?;
            tx.first_book_insert(content_hash.as_str(), storage_i64(change.mutation.changed_at, "change timestamp")?)?;
            let location = value.location.as_str();
            tx.update_reading_position(content_hash.as_str(), &location, value.progress)?;
            true
        }
        MutationBody::Annotation { annotation_id, value } => {
            apply_remote_annotation(tx, change, annotation_id, value)?;
            true
        }
        MutationBody::Bookmark { bookmark_id, value } => {
            apply_remote_bookmark(tx, change, bookmark_id, value)?;
            true
        }
        MutationBody::BookLink { link_id, value } => {
            apply_remote_book_link(tx, change, link_id, value)?;
            true
        }
        MutationBody::Metadata { content_hash, value } => {
            tx.first_book_insert(content_hash.as_str(), storage_i64(change.mutation.changed_at, "change timestamp")?)?;
            super::metadata::update_book_metadata_state_preserving_ids(tx, content_hash.clone(), value)?;
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

fn apply_remote_annotation(tx: &Transaction, change: &ServerMutation, id: &str, value: &domain::AnnotationState) -> Result<(), Box<dyn std::error::Error>> {
    tx.first_book_insert(value.content_hash.as_str(), storage_i64(change.mutation.changed_at, "change timestamp")?)?;
    let style = match value.style {
        domain::AnnotationStyle::Highlight => "highlight",
        domain::AnnotationStyle::Underline => "underline",
        domain::AnnotationStyle::Squiggly => "squiggly",
        domain::AnnotationStyle::Strikethrough => "strikethrough",
    };
    let anchor = super::marks::store_annotation_anchor(&value.anchor)?;
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

fn apply_remote_bookmark(tx: &Transaction, change: &ServerMutation, id: &str, value: &domain::BookmarkState) -> Result<(), Box<dyn std::error::Error>> {
    tx.first_book_insert(value.content_hash.as_str(), storage_i64(change.mutation.changed_at, "change timestamp")?)?;
    let created_at = storage_i64(value.created_at, "bookmark creation timestamp")?;
    let modified_at = storage_i64(value.modified_at, "bookmark modification timestamp")?;
    tx.upsert_remote_bookmark(id, value.content_hash.as_str(), &value.cfi, &value.label, created_at, modified_at, value.deleted.then_some(modified_at))?;
    Ok(())
}

fn apply_remote_book_link(tx: &Transaction, change: &ServerMutation, id: &str, value: &domain::BookLinkState) -> Result<(), Box<dyn std::error::Error>> {
    let changed_at = storage_i64(change.mutation.changed_at, "change timestamp")?;
    tx.first_book_insert(value.source_content_hash.as_str(), changed_at)?;
    tx.first_book_insert(value.target_content_hash.as_str(), changed_at)?;
    let created_at = storage_i64(value.created_at, "book-link creation timestamp")?;
    let modified_at = storage_i64(value.modified_at, "book-link modification timestamp")?;
    tx.upsert_remote_book_link(
        id,
        value.source_content_hash.as_str(),
        value.target_content_hash.as_str(),
        &value.relation,
        &value.note,
        created_at,
        modified_at,
        value.deleted.then_some(modified_at),
    )?;
    Ok(())
}

fn sync_state_identity(body: &MutationBody) -> (&'static str, String, String) {
    match body {
        MutationBody::DirectoryName { dir_id, .. } => ("directory_name", dir_id.as_str().to_owned(), String::new()),
        MutationBody::DirectoryParent { dir_id, .. } => ("directory_parent", dir_id.as_str().to_owned(), String::new()),
        MutationBody::DirectoryLifecycle { dir_id, .. } => ("directory_lifecycle", dir_id.as_str().to_owned(), String::new()),
        MutationBody::BookLifecycle { content_hash, .. } => ("book_lifecycle", content_hash.to_string(), String::new()),
        MutationBody::Placement { dir_id, content_hash, .. } => ("placement", content_hash.to_string(), dir_id.as_str().to_owned()),
        MutationBody::ReadingPosition { content_hash, .. } => ("reading_position", content_hash.to_string(), String::new()),
        MutationBody::Annotation { annotation_id, .. } => ("annotation", annotation_id.clone(), String::new()),
        MutationBody::Bookmark { bookmark_id, .. } => ("bookmark", bookmark_id.clone(), String::new()),
        MutationBody::BookLink { link_id, .. } => ("book_link", link_id.clone(), String::new()),
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
        change.replica_id.as_str(),
        storage_i64(change.mutation.replica_seq.get(), "actor sequence")?,
        &change.mutation.mutation_id.to_string(),
    )
    .map(|_| ())
}

fn apply_remote_directory_name(tx: &Transaction, dir_id: &DirId, name: &FileName) -> Result<(), Box<dyn std::error::Error>> {
    ensure_directory_placeholder(tx, dir_id)?;
    tx.set_remote_directory_name(dir_id.as_str(), name.as_str())?;
    Ok(())
}

fn apply_remote_directory_parent(tx: &Transaction, dir_id: &DirId, parent_id: &DirId) -> Result<(), Box<dyn std::error::Error>> {
    ensure_directory_placeholder(tx, parent_id)?;
    ensure_directory_placeholder(tx, dir_id)?;
    tx.set_remote_directory_parent(dir_id.as_str(), parent_id.as_str())?;
    Ok(())
}

fn apply_remote_directory_lifecycle(tx: &Transaction, dir_id: &DirId, deleted: bool) -> Result<(), Box<dyn std::error::Error>> {
    ensure_directory_placeholder(tx, dir_id)?;
    tx.set_remote_directory_lifecycle(dir_id.as_str(), i32::from(deleted))?;
    Ok(())
}

/// A pull cursor advances page by page, so a child can legitimately arrive one
/// page before its parent. Keep a deleted placeholder to satisfy SQLite's
/// foreign key without exposing an incomplete hierarchy. Applying the real
/// directory state later replaces its intent and the projection activates the
/// complete parent-before-child path.
fn ensure_directory_placeholder(tx: &Transaction, dir_id: &DirId) -> Result<(), rusqlite::Error> {
    if dir_id.as_str() == "__root__" {
        return Ok(());
    }
    tx.ensure_remote_directory_placeholder(dir_id.as_str())?;
    Ok(())
}

fn read_directory_intents(tx: &Transaction) -> Result<HashMap<DirId, projection::DirectoryIntent>, rusqlite::Error> {
    let mut intents = HashMap::new();
    tx.get_remote_directory_intents(|row| {
        let id = DirId::parse(&row.get::<_, String>(0)?).map_err(|error| invalid_text_column(0, error.to_string()))?;
        let parent_id = DirId::parse(&row.get::<_, String>(1)?).map_err(|error| invalid_text_column(1, error.to_string()))?;
        let name = FileName::parse(&row.get::<_, String>(2)?).map_err(|error| invalid_text_column(2, error.to_string()))?;
        let deleted = row.get(3)?;
        let name_version = read_sync_state_version(tx, "directory_name", id.as_str(), "")?;
        let parent_version = read_sync_state_version(tx, "directory_parent", id.as_str(), "")?;
        let lifecycle_version = read_sync_state_version(tx, "directory_lifecycle", id.as_str(), "")?;
        let (Some(name_version), Some(parent_version), Some(_)) = (name_version, parent_version, lifecycle_version) else { return Ok(()) };
        let version = Some(name_version.max(parent_version));
        intents.insert(id.clone(), projection::DirectoryIntent { id, parent_id, name, deleted, version });
        Ok(())
    })?;
    Ok(intents)
}

fn reproject_directories(tx: &Transaction) -> Result<(), Box<dyn std::error::Error>> {
    let intents = read_directory_intents(tx)?;
    let projection = projection::project_directories(intents.values().cloned())?;

    tx.hide_remote_directories()?;
    let mut ids: Vec<_> = intents.keys().cloned().collect();
    ids.sort();
    for id in &ids {
        let intent = &intents[id];
        let projected_parent = if projection.suppressed.contains(id) { "__root__" } else { intent.parent_id.as_str() };
        tx.project_remote_directory(intent.id.as_str(), projected_parent, intent.name.as_str())?;
    }

    for id in projection.ordered_active {
        tx.activate_remote_directory(id.as_str())?;
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

fn apply_remote_book_added(tx: &Transaction, content_hash: ContentHash, metadata: &domain::BookMetadataState, added_at: sync_common::UnixMillis, format: BookFormat) -> Result<(), Box<dyn std::error::Error>> {
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

fn apply_remote_book_deleted(tx: &Transaction, content_hash: ContentHash, changed_at: sync_common::UnixMillis) -> Result<(), Box<dyn std::error::Error>> {
    tx.apply_remote_book_deleted(content_hash.as_str(), storage_i64(changed_at, "book deletion timestamp")?)?;
    Ok(())
}

pub(super) fn apply_remote_book_dir(tx: &Transaction, changed_at: sync_common::UnixMillis, content_hash: ContentHash, dir_id: &DirId, present: bool) -> Result<(), Box<dyn std::error::Error>> {
    let stored_content_hash = content_hash.to_string();
    ensure_directory_placeholder(tx, dir_id)?;
    tx.first_book_insert(&stored_content_hash, storage_i64(changed_at, "placement timestamp")?)?;
    if present {
        tx.apply_remote_book_dir_added(dir_id.as_str(), &stored_content_hash)?;
    } else {
        tx.apply_remote_book_dir_removed(dir_id.as_str(), &stored_content_hash)?;
    }
    Ok(())
}
