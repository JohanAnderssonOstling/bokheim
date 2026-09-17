//! Local import commits: inspection projections, placements, observations.

use crate::{Database, DatabaseError};
use crate::sync::{Cell, decode_book_metadata, identifier_scheme_label, identifier_scope_label, invalid_text_column, normalized_description, parse_author_name_opt, project_book, source_mapping_type, source_subject_version, toc_entry_count, unix_millis};

// ---- commit ----
// Import commit: one owner method, one transaction, everything inline.
//
// The backend orchestrates everything outside storage: validation, staging,
// inspection, leases, filesystem book, and thumbnail/file existence
// checks arrive as plain data. This method verifies the destination, records
// the inspection projection, the placement, and the book observation
// atomically. No helper here takes a transaction; shared pure projection
// pieces are reused from the pull path without touching storage.
use crate::browse::navigation_from_chapters;
use crate::browse::AudiobookSql;
use crate::sync::SyncApplySql;

include_sql!("src/import/import_commit.sql");
use sync_common::{ContentHash, DirId};

/// Filesystem book outcome, recorded verbatim. Produced by the
/// backend's connection-free publish step from a database snapshot.
pub struct PublishedImport {
    pub name: String,
    pub relative_path: String,
    pub published: bool,
    pub fingerprint: Option<String>,
}

/// Snapshot in for one import commit. All filesystem truth (book
/// outcome, thumbnail need, restore paths) is resolved by the caller.
pub struct ImportCommit {
    pub parent_id: DirId,
    pub file_name: String,
    pub format: book_model::BookFormat,
    pub content_hash: ContentHash,
    pub checksum: ContentHash,
    pub size_bytes: u64,
    pub inspection_version: i64,
    pub inspection: Option<book_metadata::InspectedBook>,
    pub published: PublishedImport,
    pub request_thumbnail: bool,
    pub restore_paths: Vec<String>,
}

impl Database {
    /// Committing this transaction is the import's success point. Callers
    /// resolve all fallible filesystem data before it.
    pub fn commit_import(&self, import: ImportCommit) -> Result<(), DatabaseError> {
        let ImportCommit { parent_id, file_name: _file_name, format, content_hash, checksum, size_bytes, inspection_version, inspection, published, request_thumbnail, restore_paths } = import;
        let hash = content_hash.as_str();
        let mut live: Option<bool> = None;
        self.connection
            .live_destination_select(&parent_id.to_string(), |row| {
                live = row.get(0)?;
                Ok(())
            })
            .map_err(DatabaseError::operation)?;
        let live = live.ok_or(rusqlite::Error::QueryReturnedNoRows).map_err(DatabaseError::operation)?;
        if !live {
            return Err(DatabaseError::operation(format!("library folder does not exist: {parent_id}")));
        }
        let tx = self.connection.unchecked_transaction().map_err(DatabaseError::operation)?;
        // Reserve the write lock up front; a deferred transaction cannot
        // upgrade a stale snapshot after a concurrent scan.
        tx.reserve_write_lock().map_err(DatabaseError::operation)?;
        tx.pragma_update(None, "defer_foreign_keys", "ON").map_err(DatabaseError::operation)?;
        // Acquire the writer before reading the destination.
        let mut live: Option<bool> = None;
        tx.live_destination_select(&parent_id.to_string(), |row| {
            live = row.get(0)?;
            Ok(())
        })
        .map_err(DatabaseError::operation)?;
        let live = live.ok_or(rusqlite::Error::QueryReturnedNoRows).map_err(DatabaseError::operation)?;
        if !live {
            return Err(DatabaseError::operation(format!("library folder does not exist: {parent_id}")));
        }
        let mut current: Option<String> = None;
        tx.scan_seq_select(|row| {
            current = row.get(0)?;
            Ok(())
        })
        .map_err(DatabaseError::operation)?;
        let current = current.unwrap_or_default().parse::<i64>().map_err(|error| DatabaseError::operation(format!("scan sequence is not readable: {error}")))?;
        let scan_id = current.checked_add(1).ok_or_else(|| DatabaseError::operation("scan sequence overflow"))?;
        tx.scan_seq_update(scan_id).map_err(DatabaseError::operation)?;
        if inspection.is_none() {
            let mut inspected: Option<bool> = None;
            tx.has_import_inspection_select(hash, format.canonical_extension(), inspection_version, |row| {
                inspected = row.get(0)?;
                Ok(())
            })
            .map_err(DatabaseError::operation)?;
            let inspected = inspected.ok_or(rusqlite::Error::QueryReturnedNoRows).map_err(DatabaseError::operation)?;
            if !inspected {
                return Err(DatabaseError::operation("book changed during import; retry the import"));
            }
        }
        if let Some(inspection) = inspection {
            // Fresh inspection projects exactly like a rescan: normalize the
            // title identity, resolve contributor and publisher agents with
            // provisional-name reuse, then replace every projection row.
            let mut metadata = inspection.metadata.clone();
            book_enrichment::title_identity::normalize(&mut metadata).map_err(|error| DatabaseError::operation(format!("book title is not readable: {error}")))?;
            let mut raw: Option<Vec<u8>> = None;
            tx.meta_stored_book_select(hash, |row| {
                raw = Some(row.get::<_, Vec<u8>>(0)?);
                Ok(())
            })
            .map_err(DatabaseError::operation)?;
            let previous: Option<book_model::BookMetadata> =
                raw.and_then(|raw| decode_book_metadata(&raw).ok());
            let mut existed: Option<bool> = None;
            tx.scanned_book_exists_select(hash, |row| {
                existed = row.get(0)?;
                Ok(())
            })
            .map_err(DatabaseError::operation)?;
            let existed = existed.ok_or(rusqlite::Error::QueryReturnedNoRows).map_err(DatabaseError::operation)?;
            let mut stored_contributors: Vec<(i64, book_model::Contributor)> = Vec::new();
            for (position, contributor) in metadata.contributors.iter().enumerate() {
                let Some(name) = parse_author_name_opt(contributor.name())? else { continue };
                let position = i64::try_from(position).unwrap_or(i64::MAX);
                let mut credited: Option<(i64, String, String)> = None;
                tx.existing_book_contributor_credit(hash, position, |row| {
                    credited = Some((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?));
                    Ok(())
                })
                .map_err(DatabaseError::operation)?;
                let mut reused: Option<(i64, book_model::AgentId)> = None;
                if let Some((local_id, stable_id, credited_name)) = credited {
                    if let Ok(Some(existing_name)) = parse_author_name_opt(&credited_name) {
                        if existing_name.match_key() == name.match_key() {
                            let stable_id = book_model::AgentId::parse_str(&stable_id).map_err(|error| DatabaseError::operation(format!("stored author identity is not readable: {error}")))?;
                            reused = Some((local_id, stable_id));
                        }
                    }
                }
                let (identity_local_id, contributor_id) = match reused {
                    Some(value) => value,
                    None => {
                        let mut incoming: Option<(i64, String)> = None;
                        tx.author_identity_by_stable_id(&contributor.agent_id().to_string(), |row| {
                            incoming = Some((row.get::<_, i64>(0)?, row.get::<_, String>(1)?));
                            Ok(())
                        })
                        .map_err(DatabaseError::operation)?;
                        let selected = match incoming {
                            Some(value) => Some(value),
                            None => {
                                let mut provisional: Option<(i64, String)> = None;
                                tx.provisional_author_identity_by_name(name.match_key().as_str(), |row| {
                                    provisional = Some((row.get::<_, i64>(0)?, row.get::<_, String>(1)?));
                                    Ok(())
                                })
                                .map_err(DatabaseError::operation)?;
                                provisional
                            }
                        };
                        match selected {
                            Some((local_id, stable_id)) => {
                                let stable_id = book_model::AgentId::parse_str(&stable_id).map_err(|error| DatabaseError::operation(format!("stored author identity is not readable: {error}")))?;
                                (local_id, stable_id)
                            }
                            None => {
                                tx.insert_author_identity(
                                    &contributor.agent_id().to_string(),
                                    name.as_str(),
                                    name.match_key().as_str(),
                                    unix_millis()?,
                                )
                                .map_err(DatabaseError::operation)?;
                                (tx.last_insert_rowid(), contributor.agent_id())
                            }
                        }
                    }
                };
                let reidentified = book_model::Contributor::with_id(contributor_id, contributor.name(), contributor.sort_name().map(ToOwned::to_owned), contributor.role_code())
                    .map_err(|error| DatabaseError::operation(format!("contributor credit is not writable: {error}")))?;
                stored_contributors.push((identity_local_id, reidentified));
            }
            let mut prepared_publishers = Vec::with_capacity(metadata.book.publishers.len());
            for publisher in &metadata.book.publishers {
                let Some(name) = parse_author_name_opt(publisher.name().as_str())? else {
                    return Err(DatabaseError::operation("publisher name did not produce an agent name"));
                };
                let mut incoming: Option<(i64, String)> = None;
                tx.author_identity_by_stable_id(&publisher.agent_id().to_string(), |row| {
                    incoming = Some((row.get::<_, i64>(0)?, row.get::<_, String>(1)?));
                    Ok(())
                })
                .map_err(DatabaseError::operation)?;
                let selected = match incoming {
                    Some(value) => Some(value),
                    None => {
                        let mut provisional: Option<(i64, String)> = None;
                        tx.provisional_author_identity_by_name(name.match_key().as_str(), |row| {
                            provisional = Some((row.get::<_, i64>(0)?, row.get::<_, String>(1)?));
                            Ok(())
                        })
                        .map_err(DatabaseError::operation)?;
                        provisional
                    }
                };
                let (_, stable_id) = match selected {
                    Some((local_id, stable_id)) => {
                        let stable_id = book_model::AgentId::parse_str(&stable_id).map_err(|error| DatabaseError::operation(format!("stored author identity is not readable: {error}")))?;
                        (local_id, stable_id)
                    }
                    None => {
                        tx.insert_author_identity(
                            &publisher.agent_id().to_string(),
                            name.as_str(),
                            name.match_key().as_str(),
                            unix_millis()?,
                        )
                        .map_err(DatabaseError::operation)?;
                        (tx.last_insert_rowid(), publisher.agent_id())
                    }
                };
                prepared_publishers.push(
                    book_model::PublisherCredit::with_id(stable_id, publisher.name().as_str())
                        .map_err(|error| DatabaseError::operation(format!("publisher credit is not writable: {error}")))?,
                );
            }
            let mut prepared_book = metadata.book.clone();
            prepared_book.publishers = prepared_publishers;
            let encoded = sync_common::wire::encode(&prepared_book).map_err(DatabaseError::operation)?;
            let description = normalized_description(&metadata.description);
            let added_at = unix_millis()?;
            // The local relationship rows reference SQLite's integer key, so establish
            // the content-hash boundary row before projecting contributors into it.
            tx.upsert_scanned_book(hash, &metadata.title, &encoded, added_at, format.canonical_extension())
                .map_err(DatabaseError::operation)?;
            tx.set_description(hash, &description).map_err(DatabaseError::operation)?;
            tx.mark_description_scanned(hash).map_err(DatabaseError::operation)?;
            // Contributor writes batch their outbox payload while the change
            // origin is local; a remote origin leaves the flag alone.
            let mut origin = None;
            tx.change_origin_select(|row| {
                origin = row.get::<_, Option<String>>(0)?;
                Ok(())
            })
            .map_err(DatabaseError::operation)?;
            let origin: Option<String> = origin.flatten();
            let owns_flag = origin.as_deref() != Some("remote");
            if owns_flag {
                tx.batch_flag_set(|_| Ok(())).map_err(DatabaseError::operation)?;
            }
            for (position, (identity_local_id, credit)) in stored_contributors.iter().enumerate() {
                let role = credit.role_code().0;
                tx.authors_upsert_book_contributor(
                    hash,
                    *identity_local_id,
                    i64::try_from(position).unwrap_or(i64::MAX),
                    credit.name(),
                    credit.sort_name(),
                    &role,
                )
                .map_err(DatabaseError::operation)?;
            }
            tx.authors_trim_book_contributors(hash, i64::try_from(stored_contributors.len()).unwrap_or(i64::MAX))
                .map_err(DatabaseError::operation)?;
            if owns_flag {
                tx.batch_flag_flush().map_err(DatabaseError::operation)?;
            }
            if previous.as_ref() != Some(&prepared_book) {
                let projected = project_book(&prepared_book, encoded, previous.as_ref())?;
                let mut book_id: Option<i64> = None;
                tx.meta_replace_prepared_projections_select(hash, |r| {
                    book_id = r.get(0)?;
                    Ok(())
                })
                .map_err(DatabaseError::operation)?;
                let book_id = book_id.ok_or(rusqlite::Error::QueryReturnedNoRows).map_err(DatabaseError::operation)?;
                let mut identity_agents: std::collections::HashMap<book_model::AgentId, i64> = std::collections::HashMap::new();
                let mut identity_publishers: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
                if previous.as_ref().is_none_or(|old| old.publishers != projected.metadata.publishers) {
                    tx.clear_book_publishers(book_id).map_err(DatabaseError::operation)?;
                    for (position, publisher) in projected.metadata.publishers.iter().enumerate() {
                        let name = catalog_model::AuthorName::parse(publisher.name().as_str())
                            .map_err(|error| DatabaseError::operation(format!("publisher name is not readable: {error}")))?;
                        let agent = if let Some(row) = identity_agents.get(&publisher.agent_id()) {
                            *row
                        } else {
                            let mut incoming: Option<(i64, String)> = None;
                            tx.author_identity_by_stable_id(&publisher.agent_id().to_string(), |row| {
                                incoming = Some((row.get::<_, i64>(0)?, row.get::<_, String>(1)?));
                                Ok(())
                            })
                            .map_err(DatabaseError::operation)?;
                            let (row, _) = match incoming {
                                Some((local_id, _)) => (local_id, publisher.agent_id()),
                                None => {
                                    tx.insert_author_identity(
                                        &publisher.agent_id().to_string(),
                                        name.as_str(),
                                        name.match_key().as_str(),
                                        unix_millis()?,
                                    )
                                    .map_err(DatabaseError::operation)?;
                                    (tx.last_insert_rowid(), publisher.agent_id())
                                }
                            };
                            identity_agents.insert(publisher.agent_id(), row);
                            row
                        };
                        let key = publisher.name().match_key().as_str().to_owned();
                        let row = if let Some(row) = identity_publishers.get(&key) {
                            *row
                        } else {
                            tx.insert_publisher(publisher.name().as_str(), &key)
                                .map_err(DatabaseError::operation)?;
                            let mut row: Option<i64> = None;
                            tx.authors_publisher_select(&key, |r| {
                                row = r.get(0)?;
                                Ok(())
                            })
                            .map_err(DatabaseError::operation)?;
                            let row = row.ok_or(rusqlite::Error::QueryReturnedNoRows).map_err(DatabaseError::operation)?;
                            identity_publishers.insert(key, row);
                            row
                        };
                        tx.insert_book_publisher(book_id, row, agent, position as i64, publisher.name().as_str())
                            .map_err(DatabaseError::operation)?;
                    }
                }
                if previous.as_ref().is_none_or(|old| old.identifiers != projected.metadata.identifiers) {
                    tx.clear_book_identifiers(book_id).map_err(DatabaseError::operation)?;
                    for (position, identifier) in projected.metadata.identifiers.iter().enumerate() {
                        tx.insert_book_identifier(
                            book_id,
                            position as i64,
                            &identifier_scheme_label(identifier.scheme()),
                            identifier.value(),
                            identifier.canonical_value().as_deref(),
                            identifier.source(),
                            identifier_scope_label(identifier.scope()),
                        )
                        .map_err(DatabaseError::operation)?;
                    }
                }
                if previous.as_ref().is_none_or(|old| old.languages != projected.metadata.languages) {
                    tx.clear_book_languages(book_id).map_err(DatabaseError::operation)?;
                    for (position, language) in projected.metadata.languages.iter().enumerate() {
                        tx.insert_book_language(book_id, position as i64, language.as_str())
                            .map_err(DatabaseError::operation)?;
                    }
                }
                if previous.as_ref().is_none_or(|old| old.subjects != projected.metadata.subjects) {
                    for assignment in &projected.assignments {
                        if assignment.system_id() != subject_projection::BISAC_SYSTEM_ID {
                            let components: Vec<&str> = assignment.subject_path().split(" / ").map(str::trim).filter(|component| !component.is_empty()).collect();
                            tx.meta_ensure_external_subject_nodes_insert(assignment.system_id())
                                .map_err(DatabaseError::operation)?;
                            let mut path = String::new();
                            let mut parent_path = String::new();
                            for component in components {
                                if !path.is_empty() {
                                    path.push_str(" / ");
                                }
                                path.push_str(component);
                                tx.meta_ensure_external_subject_nodes_insert_2(assignment.system_id(), &path, &parent_path, component)
                                    .map_err(DatabaseError::operation)?;
                                parent_path.clone_from(&path);
                            }
                            tx.meta_ensure_external_subject_nodes_update(assignment.system_id(), assignment.matcher_version())
                                .map_err(DatabaseError::operation)?;
                        }
                    }
                    for (index, concept, path) in &projected.concepts {
                        let code = &projected.subject_codes[*index];
                        let version = source_subject_version(code.system_id());
                        tx.apply_insert(code.system_id(), version, code.code())
                            .map_err(DatabaseError::operation)?;
                        tx.apply_insert_2(code.system_id(), version, code.code(), *concept, source_mapping_type(code.system_id()), subject_projection::UNIFIED_MATCHER_VERSION)
                            .map_err(DatabaseError::operation)?;
                        tx.apply_insert_3(code.system_id(), code.code(), path, subject_projection::UNIFIED_MATCHER_VERSION)
                            .map_err(DatabaseError::operation)?;
                    }
                    for rows in &projected.classification_rows {
                        let (select_sql, delete_sql, insert_sql): (&str, &str, &str) = match rows.table {
                            "book_subject" => (
                                include_str!("../sync/reconcile/book_subject_select.sql"),
                                include_str!("../sync/reconcile/book_subject_delete.sql"),
                                include_str!("../sync/reconcile/book_subject_insert.sql"),
                            ),
                            "book_subject_code" => (
                                include_str!("../sync/reconcile/book_subject_code_select.sql"),
                                include_str!("../sync/reconcile/book_subject_code_delete.sql"),
                                include_str!("../sync/reconcile/book_subject_code_insert.sql"),
                            ),
                            "book_subject_assignment" => (
                                include_str!("../sync/reconcile/book_subject_assignment_select.sql"),
                                include_str!("../sync/reconcile/book_subject_assignment_delete.sql"),
                                include_str!("../sync/reconcile/book_subject_assignment_insert.sql"),
                            ),
                            "book_subject_assignment_evidence" => (
                                include_str!("../sync/reconcile/book_subject_assignment_evidence_select.sql"),
                                include_str!("../sync/reconcile/book_subject_assignment_evidence_delete.sql"),
                                include_str!("../sync/reconcile/book_subject_assignment_evidence_insert.sql"),
                            ),
                            "book_unified_concept" => (
                                include_str!("../sync/reconcile/book_unified_concept_select.sql"),
                                include_str!("../sync/reconcile/book_unified_concept_delete.sql"),
                                include_str!("../sync/reconcile/book_unified_concept_insert.sql"),
                            ),
                            "book_unified_concept_evidence" => (
                                include_str!("../sync/reconcile/book_unified_concept_evidence_select.sql"),
                                include_str!("../sync/reconcile/book_unified_concept_evidence_delete.sql"),
                                include_str!("../sync/reconcile/book_unified_concept_evidence_insert.sql"),
                            ),
                            _ => return Err(DatabaseError::operation("unknown classification table")),
                        };
                        let existing: std::collections::BTreeMap<Vec<Cell>, Vec<Cell>> = tx
                            .prepare_cached(select_sql)
                            .map_err(DatabaseError::operation)?
                            .query_map([book_id], |r| {
                                (0..rows.width)
                                    .map(|i| match r.get_ref(i)? {
                                        rusqlite::types::ValueRef::Null => Ok(Cell::Null),
                                        rusqlite::types::ValueRef::Integer(v) => Ok(Cell::Integer(v)),
                                        rusqlite::types::ValueRef::Text(v) => Ok(Cell::Text(std::str::from_utf8(v).map_err(|error| invalid_text_column(i, error.to_string()))?.into())),
                                        _ => Err(invalid_text_column(i, "unexpected classification value")),
                                    })
                                    .collect::<rusqlite::Result<Vec<_>>>()
                            })
                            .map_err(DatabaseError::operation)?
                            .collect::<Result<Vec<_>, rusqlite::Error>>()
                            .map_err(DatabaseError::operation)?
                            .into_iter()
                            .map(|v| (v[..rows.keys].to_vec(), v))
                            .collect();
                        for key in existing.keys().filter(|key| !rows.desired.contains_key(*key)) {
                            tx.prepare_cached(delete_sql)
                                .map_err(DatabaseError::operation)?
                                .execute(rusqlite::params_from_iter(std::iter::once(Cell::Integer(book_id)).chain(key.iter().cloned())))
                                .map_err(DatabaseError::operation)?;
                        }
                        for (key, values) in &rows.desired {
                            if existing.get(key) == Some(values) {
                                continue;
                            }
                            tx.prepare_cached(insert_sql)
                                .map_err(DatabaseError::operation)?
                                .execute(rusqlite::params_from_iter(std::iter::once(Cell::Integer(book_id)).chain(values.iter().cloned())))
                                .map_err(DatabaseError::operation)?;
                        }
                    }
                }
                if previous.as_ref().is_none_or(|old| old.collections != projected.metadata.collections) {
                    tx.clear_book_collections(book_id).map_err(DatabaseError::operation)?;
                    for (position, collection) in projected.metadata.collections.iter().enumerate() {
                        tx.insert_book_collection(
                            book_id,
                            position as i64,
                            collection.name(),
                            collection.collection_type(),
                            collection.position(),
                        )
                        .map_err(DatabaseError::operation)?;
                    }
                }
                if previous.as_ref().is_none_or(|old| old.dates != projected.metadata.dates) {
                    tx.clear_book_dates(book_id).map_err(DatabaseError::operation)?;
                    for (position, date) in projected.metadata.dates.iter().enumerate() {
                        tx.insert_book_date(
                            book_id,
                            position as i64,
                            date.value(),
                            None::<i32>,
                            None::<i64>,
                            None::<i64>,
                            date.event(),
                        )
                        .map_err(DatabaseError::operation)?;
                    }
                }
                let mut ordered_titles = projected.metadata.alternate_titles.iter().collect::<Vec<_>>();
                ordered_titles.sort_by_key(|title| title.source() == "title:colon-fallback" && title.title_type() == Some("subtitle"));
                let alternate_titles = ordered_titles
                    .into_iter()
                    .map(|value| (value.value().to_owned(), value.title_type().map(ToOwned::to_owned), value.sort_as().map(ToOwned::to_owned)))
                    .collect::<Vec<_>>();
                for (kind, values) in [
                    ("alternate_title", alternate_titles),
                    ("rights", projected.metadata.rights.iter().cloned().map(|value| (value, None, None)).collect::<Vec<_>>()),
                    ("source", projected.metadata.sources.iter().cloned().map(|value| (value, None, None)).collect::<Vec<_>>()),
                    ("type", projected.metadata.types.iter().cloned().map(|value| (value, None, None)).collect::<Vec<_>>()),
                    ("format", projected.metadata.formats.iter().cloned().map(|value| (value, None, None)).collect::<Vec<_>>()),
                    ("relation", projected.metadata.relations.iter().cloned().map(|value| (value, None, None)).collect::<Vec<_>>()),
                    ("coverage", projected.metadata.coverage.iter().cloned().map(|value| (value, None, None)).collect::<Vec<_>>()),
                    (
                        "property",
                        projected
                            .metadata
                            .properties
                            .iter()
                            .map(|property| (property.value().to_owned(), property.refines().map(ToOwned::to_owned), Some(property.property().to_owned())))
                            .collect::<Vec<_>>(),
                    ),
                    ("enrichment_isbn", projected.references.clone()),
                ] {
                    let changed = previous.as_ref().is_none_or(|old| match kind {
                        "alternate_title" => old.alternate_titles != projected.metadata.alternate_titles,
                        "rights" => old.rights != projected.metadata.rights,
                        "source" => old.sources != projected.metadata.sources,
                        "type" => old.types != projected.metadata.types,
                        "format" => old.formats != projected.metadata.formats,
                        "relation" => old.relations != projected.metadata.relations,
                        "coverage" => old.coverage != projected.metadata.coverage,
                        "property" | "enrichment_isbn" => old.properties != projected.metadata.properties,
                        _ => true,
                    });
                    if !changed {
                        continue;
                    }
                    tx.meta_replace_prepared_projections_delete(book_id, kind)
                        .map_err(DatabaseError::operation)?;
                    for (position, (value, qualifier, secondary)) in values.iter().enumerate() {
                        tx.insert_book_value(
                            book_id,
                            kind,
                            position as i64,
                            value,
                            qualifier.as_deref(),
                            secondary.as_deref(),
                        )
                        .map_err(DatabaseError::operation)?;
                    }
                }
            }
            if !existed {
                // The insert trigger runs before contributor projections exist. Fold
                // those projections into the initial lifecycle body and remove the
                // redundant metadata cell produced while building the new row.
                tx.scanned_fold_lifecycle_update(hash).map_err(DatabaseError::operation)?;
                tx.scanned_fold_metadata_delete(hash).map_err(DatabaseError::operation)?;
            }
            if let Some(audiobook) = inspection.audiobook.as_ref() {
                let duration_ms = i64::try_from(audiobook.duration_ms).map_err(|_| DatabaseError::operation("audiobook duration exceeds SQLite integer range"))?;
                tx.audiobook_upsert_metadata(hash, duration_ms)
                    .map_err(DatabaseError::operation)?;
                // Preserve a database-only chapter plan while the imported evidence is unchanged.
                let mut applied_request: Option<String> = None;
                tx.audiobook_applied_request_select(hash, |row| {
                    applied_request = row.get(0)?;
                    Ok(())
                })
                .map_err(DatabaseError::operation)?;
                let request = audiobook.audible_lookup_request();
                let preserve_chapters = applied_request
                    .and_then(|json| serde_json::from_str::<metadata_contract::audible::LookupRequest>(&json).ok())
                    .is_some_and(|applied| applied == request);
                if !preserve_chapters {
                    let entries = navigation_from_chapters(&audiobook.chapters);
                    let entry_count = i64::try_from(toc_entry_count(&entries)).map_err(|_| DatabaseError::operation("table of contents entry count exceeds SQLite integer range"))?;
                    let toc_json = serde_json::to_string(&entries).map_err(DatabaseError::operation)?;
                    tx.book_toc_upsert(hash, entry_count, &toc_json)
                        .map_err(DatabaseError::operation)?;
                }
                let request_json = serde_json::to_string(&request).map_err(DatabaseError::operation)?;
                let now = unix_millis()?.saturating_div(1000);
                tx.audible_discover_insert(hash, metadata_contract::audible::POLICY_VERSION, &request_json, now)
                    .map_err(DatabaseError::operation)?;
            }
            // A document that yielded nothing is still recorded, so that nothing
            // parses it again hoping for a different answer.
            let inspected_toc = match format {
                book_model::BookFormat::Epub | book_model::BookFormat::Mobi | book_model::BookFormat::Pdf => Some(inspection.toc.clone()),
                book_model::BookFormat::M4b => None,
            };
            if let Some(entries) = inspected_toc {
                let entry_count = i64::try_from(toc_entry_count(&entries)).map_err(|_| DatabaseError::operation("table of contents entry count exceeds SQLite integer range"))?;
                let toc_json = serde_json::to_string(&entries).map_err(DatabaseError::operation)?;
                tx.book_toc_upsert(hash, entry_count, &toc_json)
                    .map_err(DatabaseError::operation)?;
            }
            if let Some(pdf) = &inspection.pdf {
                if !pdf.valid_for(checksum.as_str()) {
                    return Err(DatabaseError::operation("PDF ingestion checksum or metadata is invalid"));
                }
                let bytes = serde_json::to_vec(pdf).map_err(DatabaseError::operation)?;
                tx.pdf_reader_metadata_upsert(hash, checksum.as_str(), &bytes)
                    .map_err(DatabaseError::operation)?;
            }
            tx.record_import_inspection(hash, format.canonical_extension(), inspection_version)
                .map_err(DatabaseError::operation)?;
        }
        // The published file name is authoritative: connection-free book
        // may have uniquified it while this transaction was not held. Browser
        // imports have no placement filesystem, so they skip this recheck.
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut conflict: Option<bool> = None;
            tx.publish_conflict_select(&parent_id.to_string(), &library_replica::portable_name_key(&published.name), hash, |row| {
                conflict = row.get(0)?;
                Ok(())
            })
            .map_err(DatabaseError::operation)?;
            let conflict = conflict.ok_or(rusqlite::Error::QueryReturnedNoRows).map_err(DatabaseError::operation)?;
            if conflict {
                return Err(DatabaseError::operation("destination filename changed during import; retry the import"));
            }
        }
        tx.insert_book_dir(&parent_id.to_string(), hash, &published.name, "", scan_id, 1)
            .map_err(DatabaseError::operation)?;
        #[cfg(not(target_arch = "wasm32"))]
        tx.publish_insert(hash, &parent_id.to_string(), &published.relative_path)
            .map_err(DatabaseError::operation)?;
        if published.published {
            if let Some(fingerprint) = published.fingerprint.as_deref() {
                tx.publish_update_2(&parent_id.to_string(), hash, fingerprint)
                    .map_err(DatabaseError::operation)?;
            }
        }
        if request_thumbnail {
            tx.request_thumbnail(hash).map_err(DatabaseError::operation)?;
        }
        if published.published {
            let size = i64::try_from(size_bytes).map_err(|_| DatabaseError::operation("book size exceeds SQLite range"))?;
            let mut previous: Option<String> = None;
            tx.observe_book_select(&parent_id.to_string(), hash, |row| {
                previous = row.get(0)?;
                Ok(())
            })
            .map_err(DatabaseError::operation)?;
            if previous.as_deref() != Some(checksum.as_str()) {
                let unseen = tx
                    .observe_insert(hash, checksum.as_str(), size, "local")
                    .map_err(DatabaseError::operation)?
                    != 0;
                let changed = previous.is_some();
                if unseen || changed {
                    tx.enqueue_version_insert(hash, checksum.as_str(), size)
                        .map_err(DatabaseError::operation)?;
                    tx.enqueue_version_delete(hash).map_err(DatabaseError::operation)?;
                    tx.enqueue_asset_work_insert(hash).map_err(DatabaseError::operation)?;
                }
                let origin: String = if changed {
                    "local".to_owned()
                } else {
                    let mut origin: Option<String> = None;
                    tx.observe_origin_select(hash, checksum.as_str(), |row| {
                        origin = row.get(0)?;
                        Ok(())
                    })
                    .map_err(DatabaseError::operation)?;
                    origin.ok_or(rusqlite::Error::QueryReturnedNoRows).map_err(DatabaseError::operation)?
                };
                tx.remember_placement_insert(&parent_id.to_string(), hash, checksum.as_str(), &origin)
                    .map_err(DatabaseError::operation)?;
            }
        }
        for path in &restore_paths {
            tx.queue_file_work_insert("restore", hash, path)
                .map_err(DatabaseError::operation)?;
        }
        tx.commit().map_err(DatabaseError::operation)?;
        Ok(())
    }

    /// Placement rows for one book with their remembered materialized paths.
    /// The orchestrator filters these against the filesystem (without holding
    /// the writer) and hands the restore set back to the commit.
    pub fn import_restore_candidates(&self, hash: &ContentHash) -> Result<Vec<(String, Option<String>)>, DatabaseError> {
        let mut candidates = Vec::new();
        self.connection
            .restore_candidates_select(hash.as_str(), |row| {
                candidates.push((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?));
                Ok(())
            })
            .map_err(DatabaseError::operation)?;
        Ok(candidates)
    }
}

#[cfg(test)]
mod commit_tests {
    use crate::*;

    struct TestDatabase {
        database: Database,
        path: std::path::PathBuf,
    }

    impl Drop for TestDatabase {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    fn open_test_database() -> TestDatabase {
        static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("library-database-import-commit-{}-{id}", std::process::id()));
        let database = Database::open(&path, |_| Ok(())).expect("open test database");
        database.initialize_library().expect("initialize test library");
        TestDatabase { database, path }
    }

    fn inspected(title: &str, author: &str) -> book_metadata::InspectedBook {
        book_metadata::InspectedBook {
            pdf: None,
            metadata: book_model::BookRecord {
                title: title.to_owned(),
                contributors: vec![book_model::Contributor::new(author, None, book_model::MarcRelatorCode(*b"aut")).unwrap()],
                description: "A synopsis.".to_owned(),
                book: book_model::BookMetadata::default(),
            },
            audiobook: None,
            toc: vec![book_model::BookTocEntry { title: "Chapter 1".to_owned(), target: "ch1".to_owned(), children: Vec::new() }],
        }
    }

    #[test]
    fn commit_import_projects_inspection_placement_and_book() {
        let holder = open_test_database();
        let database = &holder.database;
        let parent = database.create_directory(&sync_common::ROOT_DIR_ID, &"Fiction".to_owned()).expect("create folder");
        let hash = ContentHash::new(&"9".repeat(64));
        let checksum = ContentHash::new(&"a".repeat(64));
        database
            .commit_import(ImportCommit {
                parent_id: parent.id,
                file_name: "book.epub".to_owned(),
                format: book_model::BookFormat::Epub,
                content_hash: hash,
                checksum,
                size_bytes: 1024,
                inspection_version: 1,
                inspection: Some(inspected("Imported Title", "Import Author")),
                published: PublishedImport { name: "book.epub".to_owned(), relative_path: "Fiction/book.epub".to_owned(), published: true, fingerprint: Some("fp1".to_owned()) },
                request_thumbnail: true,
                restore_paths: vec!["Fiction/book.epub".to_owned()],
            })
            .expect("commit import");
        let detail = database.browse_book_detail(hash).expect("read detail");
        assert_eq!(detail.book.title, "Imported Title");
        assert_eq!(detail.authors.iter().map(|author| author.name.as_str()).collect::<Vec<_>>(), vec!["Import Author"]);
        assert!(database.has_book_placement(hash).expect("placement"));
        assert!(database.pending_thumbnail_job_count().expect("thumbnail jobs") >= 1);
        let candidates = database.import_restore_candidates(&hash).expect("restore candidates");
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].1.as_deref(), Some("Fiction/book.epub"));
        let _ = database.has_pending_asset_work().expect("pending asset work");
        // A cached inspection reuses the recorded proof and still commits.
        let other = database.create_directory(&sync_common::ROOT_DIR_ID, &"Nonfiction".to_owned()).expect("create folder");
        database
            .commit_import(ImportCommit {
                parent_id: other.id,
                file_name: "book.epub".to_owned(),
                format: book_model::BookFormat::Epub,
                content_hash: hash,
                checksum,
                size_bytes: 1024,
                inspection_version: 1,
                inspection: None,
                published: PublishedImport { name: "book.epub".to_owned(), relative_path: "Nonfiction/book.epub".to_owned(), published: true, fingerprint: Some("fp1".to_owned()) },
                request_thumbnail: false,
                restore_paths: Vec::new(),
            })
            .expect("commit cached import");
    }
}
