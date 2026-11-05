//! Applying a pull page: preparation is pure; all database decisions share one transaction.

use super::*;
use crate::projection::project_dirty;
use crate::query_methods::query_rows;
use crate::transactions::WriteOutcome::Commit;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use sync_common::{BookId, StateAddress, StateKey};

/// Changes accepted by a committed pull, excluding stale or repeated mutations.
#[derive(Debug, Default)]
pub struct SyncPullChanges {
    pub contents_changed: bool,
    pub reading_progress: BTreeSet<BookId>,
}

impl SyncPullChanges {
    fn record(&mut self, entry: &LibraryStateEntry) {
        match entry {
            LibraryStateEntry::ReadingPosition { book_id, .. } | LibraryStateEntry::ChapterReadStatus { book_id, .. } => {
                self.reading_progress.insert(*book_id);
            }
            _ => self.contents_changed = true,
        }
    }
}

impl Database {
    /// Acknowledges sent mutations and applies a page atomically. Wire decoding,
    /// batch planning and input-only serialization run before reserving the writer.
    /// Returns the kinds of changes accepted by the merge for presentation updates.
    pub fn sync_commit_pull_response(&self, ids: &[ReplicaSeq], page: &PullStateResponse) -> Result<SyncPullChanges, DatabaseError> {
        if page.mutations.is_empty() && ids.is_empty() && read_pull_cursor(&self.connection)? == page.next_cursor {
            return Ok(SyncPullChanges::default());
        }
        let mut changes = Vec::with_capacity(page.mutations.len());
        for change in &page.mutations {
            if let Some(state) = sync_common::state_encoding::decode_known::<LibraryStateEntry>(&change.state).map_err(DatabaseError::operation)? {
                changes.push(state);
            } else {
                log::debug!("Skipping unknown synchronized state kind {}", change.state.value.address.kind);
            }
        }
        let acknowledgements = if ids.is_empty() { None } else { Some(serde_json::to_string(ids).map_err(DatabaseError::operation)?) };
        self.with_write_transaction(|tx| {
            tx.pragma_update(None, "defer_foreign_keys", "ON").map_err(DatabaseError::operation)?;
            let mut confirmed = HashMap::new();
            if let Some(payload) = acknowledgements {
                // An acknowledgement confirms a publication even if its field
                // is on a later pull page. Capture only still-matching queue
                // generations: old acknowledgements cannot confirm new edits.
                let cells = query_rows(|callback| tx.pull_acknowledged_addresses(&(&payload), callback), |r| Ok(StateAddress { kind: r.get(0)?, entity_key: r.get(1)?, entity_subkey: r.get(2)? }))?;
                for cell in cells {
                    if let Some(version) = publication_version(tx, &cell.kind, &cell.entity_key, &cell.entity_subkey)? {
                        confirmed.insert(cell, version);
                    }
                }
                tx.outbox_delete_for_ids(&payload).map_err(DatabaseError::operation)?;
            }
            if page.mutations.is_empty() && read_pull_cursor(tx)? == page.next_cursor {
                return Ok(Commit(SyncPullChanges::default()));
            }
            let mut applied = SyncPullChanges::default();
            {
                // The server delivers a book's lifecycle before any of its
                // fields, so the page's lifecycles are the only book creations.
                let lifecycles = changes.iter().filter_map(|state| match &state.value {
                    LibraryStateEntry::BookLifecycle { book_id, .. } => Some((state, book_id)),
                    _ => None,
                });
                // A still-existing row may have missed a complete purge/readd.
                // Remember its lifecycle before merging this page.
                let mut existing = BTreeMap::new();
                for (_, hash) in lifecycles.clone() {
                    if tx.pull_book_exists(&hash, |r| r.get::<_, bool>(0))? {
                        existing.insert(hash.to_owned(), read_version(tx, "book_lifecycle", &StateKey::Book(*hash), "")?);
                    }
                }
                for (state, _) in lifecycles.clone() {
                    if registers::merge(tx, state)? {
                        applied.record(&state.value);
                    }
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
                            applied.contents_changed = true;
                        }
                    }
                }
                for state in changes.iter().filter(|state| !matches!(state.value, LibraryStateEntry::BookLifecycle { .. })) {
                    if registers::merge(tx, state)? {
                        applied.record(&state.value);
                    }
                }
                project_dirty(tx)?;
                {
                    for state in changes {
                        let version = state.version;
                        let address = state.value.address();
                        let immutable = records::is_record(&address.kind);
                        confirmed
                            .entry(address)
                            .and_modify(|old| {
                                if immutable || version > *old {
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
                            if confirmed.get(&cell).is_some_and(|seen| if records::is_record(&cell.kind) { Some(seen) == current.as_ref() } else { Some(seen) >= current.as_ref() }) {
                                continue;
                            }
                            registers::recover(tx, &cell)?;
                        }
                    }
                };
                write_pull_cursor(tx, &page.next_cursor)?;
            }
            Ok(Commit(applied))
        })
    }
}

// Async access owns its arguments until the synchronous database call completes.
database_methods! {
    fn sync_commit_pull_response(ids: &[ReplicaSeq], page: &PullStateResponse) -> SyncPullChanges;
    arguments (Vec<ReplicaSeq>, PullStateResponse,) = (ids.to_vec(), page.clone(),);
    execute database, arguments => database.sync_commit_pull_response(&arguments.0, &arguments.1);
}
