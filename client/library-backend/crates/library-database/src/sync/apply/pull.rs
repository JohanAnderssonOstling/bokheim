//! Applying a pull page: preparation is pure; all database decisions share one transaction.

use crate::query_methods::query_rows;
use sync_common::StateKey;
use crate::transactions::WriteOutcome::Commit;
use super::*;
use crate::projection::project_dirty;

fn read_cursor(conn: &rusqlite::Connection) -> Result<sync_common::SyncCursor, DatabaseError> {
    Ok(sync_common::SyncCursor { lifecycle_revision: parse_revision(read_cursor_revision(conn, PullCursorKey::Lifecycle)?)?, field_revision: parse_revision(read_cursor_revision(conn, PullCursorKey::Field)?)? })
}

impl Database {
    /// Acknowledges sent mutations and applies a page atomically. Wire decoding,
    /// batch planning and input-only serialization run before reserving the writer.
    /// Returns whether the page carried remote changes, including stale changes.
    pub fn sync_commit_pull_response(&self, ids: &[ReplicaSeq], page: &PullStateResponse) -> Result<bool, DatabaseError> {
        if page.mutations.is_empty() && ids.is_empty() && read_cursor(&self.connection)? == page.next_cursor {
            return Ok(false);
        }
        let mut changes = Vec::with_capacity(page.mutations.len());
        for change in &page.mutations {
            if let Some(state) = sync_common::state_encoding::decode_known::<LibraryStateEntry>(&change.state).map_err(DatabaseError::operation)? {
                changes.push(state);
            } else {
                log::debug!("Skipping unknown synchronized state kind {}", change.state.value.address.kind);
            }
        }
        let prepared = PreparedRemoteChanges { changes };
        let acknowledgements = if ids.is_empty() { None } else { Some(serde_json::to_string(&ids.iter().map(|seq| seq.get()).collect::<Vec<_>>()).map_err(DatabaseError::operation)?) };
        self.with_write_transaction(|tx| {
            tx.pragma_update(None, "defer_foreign_keys", "ON").map_err(DatabaseError::operation)?;
            let mut acknowledged_fields = Vec::new();
            if let Some(payload) = acknowledgements {
                // An acknowledgement confirms a publication even if its field
                // is on a later pull page. Capture only still-matching queue
                // generations: old acknowledgements cannot confirm new edits.
                let cells = query_rows(|callback| tx.pull_acknowledged_addresses(&(&payload), callback), |r| Ok((r.get::<_, String>(0)?, r.get::<_, StateKey>(1)?, r.get::<_, String>(2)?)))?
                    .collect::<Result<Vec<_>, _>>()?;
                for cell in cells {
                    if let Some(version) = publication_version(tx, &cell.0, &cell.1, &cell.2)? {
                        acknowledged_fields.push((cell, version));
                    }
                }
                tx.outbox_delete_for_ids(&payload).map_err(DatabaseError::operation)?;
            }
            if page.mutations.is_empty() && read_cursor(tx)? == page.next_cursor {
                return Ok(Commit(false));
            }
            {
                // The server delivers a book's lifecycle before any of its
                // fields, so the page's lifecycles are the only book creations.
                let lifecycles = prepared.changes.iter().filter_map(|state| match &state.value {
                    LibraryStateEntry::BookLifecycle { content_hash, .. } => Some((state, content_hash)),
                    _ => None,
                });
                // A still-existing row may have missed a complete purge/readd.
                // Remember its lifecycle before merging this page.
                let mut existing = std::collections::BTreeMap::new();
                for (_, hash) in lifecycles.clone() {
                    if tx.pull_book_exists(&hash, |r| r.get::<_, bool>(0))? {
                        existing.insert(hash.to_owned(), read_version(tx, "book_lifecycle", &StateKey::Book(*hash), "")?);
                    }
                }
                for (state, _) in lifecycles.clone() {
                    registers::merge(tx, state)?;
                }
                // Apply final lifecycle winners before admitting dependent fields.
                project_dirty(tx)?;
                {
                    for (_, hash) in lifecycles {
                        let Some((bytes, _)) = registers::canonical_body(tx, "book_lifecycle", &StateKey::Book(*hash), "")? else { continue };
                        let winner: LibraryStateEntry = sync_common::wire::decode(&bytes, sync_common::wire::MAX_DECODED_REQUEST_BYTES).map_err(DatabaseError::operation)?;
                        if matches!(winner, LibraryStateEntry::BookLifecycle { value: library_replica::BookLifecycleState::Purged, .. }) {
                            continue;
                        }
                        if tx.pull_ensure_book(&hash, &(crate::browse::text::search_title(None, None)))? != 0 {
                            tx.pull_mark_book_dirty(&hash)?;
                        }
                    }
                }
                for state in prepared.changes.iter().filter(|state| !matches!(state.value, LibraryStateEntry::BookLifecycle { .. })) {
                    registers::merge(tx, state)?;
                }
                project_dirty(tx)?;
                {
                    let prepared: &PreparedRemoteChanges = &prepared;
                    let acknowledged: Vec<((String, StateKey, String), library_replica::VersionKey)> = acknowledged_fields;
                    let mut confirmed = acknowledged.into_iter().collect::<std::collections::HashMap<_, _>>();
                    for state in &prepared.changes {
                        let version = state.version.clone();
                        let (kind, key, subkey) = sync_state_identity(&state.value);
                        confirmed
                            .entry((kind.to_owned(), key, subkey))
                            .and_modify(|old: &mut library_replica::VersionKey| {
                                if records::is_record(kind) || version > *old {
                                    *old = version.clone();
                                }
                            })
                            .or_insert(version);
                    }
                    for (hash, previous) in existing {
                        if read_version(tx, "book_lifecycle", &StateKey::Book(hash), "")? == previous || !tx.pull_book_exists(&(&hash), |r| r.get::<_, bool>(0))? {
                            continue;
                        }
                        for cell in registers::book_fields(tx, &hash)? {
                            let current = publication_version(tx, &cell.kind, &cell.entity_key, &cell.entity_subkey)?;
                            if confirmed.get(&(cell.kind.clone(), cell.entity_key.clone(), cell.entity_subkey.clone())).is_some_and(|seen| {
                                if records::is_record(&cell.kind) { Some(seen) == current.as_ref() }
                                else { Some(seen) >= current.as_ref() }
                            }) {
                                continue;
                            }
                            registers::recover(tx, &cell)?;
                        }
                    }
                };
                write_cursor_revision(tx, PullCursorKey::Lifecycle, page.next_cursor.lifecycle_revision)?;
                write_cursor_revision(tx, PullCursorKey::Field, page.next_cursor.field_revision)?;
            }
            Ok(Commit(!page.mutations.is_empty()))
        })
    }
}

