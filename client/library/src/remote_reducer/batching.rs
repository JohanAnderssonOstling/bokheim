//! Read snapshots only for independent additions; mixed pages retain ordered reads.
use super::*;
use std::collections::HashSet;

pub(super) struct BookSnapshot {
    pub version: Option<VersionKey>,
    pub has_metadata: bool,
    pub is_purged: bool,
    pub exists: bool,
}

pub(super) fn independent_additions(changes: &PreparedRemoteChanges<'_>) -> bool {
    let mut seen = HashSet::new();
    !changes.changes.is_empty() && changes.changes.iter().all(|(_, body, _)| matches!(body, MutationBody::BookLifecycle { content_hash, value: library_replica::BookLifecycleState::Present { .. } } if seen.insert(*content_hash)))
}

pub(super) fn independent_metadata(changes: &PreparedRemoteChanges<'_>) -> bool {
    let mut seen = HashSet::new();
    !changes.changes.is_empty() && changes.changes.iter().all(|(_, body, _)| matches!(body, MutationBody::Metadata { content_hash, .. } if seen.insert(*content_hash)))
}

pub(super) fn apply_metadata<'a>(tx: &'a Transaction<'a>, changes: &PreparedRemoteChanges<'_>, origin: &mut Option<TxOriginGuard<'a>>) -> Result<(), Box<dyn std::error::Error>> {
    for page in changes.changes.chunks(100) {
        let identities = page.iter().map(|(_, _, identity)| identity).collect::<Vec<_>>();
        let snapshots = snapshots(tx, &identities)?;
        let mut metadata = Vec::new();
        let mut missing = Vec::new();
        let mut versions = Vec::new();
        for ((change, body, identity), snapshot) in page.iter().zip(&snapshots) {
            if !sync_state_version_wins(tx, change, identity.0, &identity.1, &identity.2, Some(snapshot))? {
                continue;
            }
            let MutationBody::Metadata { content_hash, value } = body else { unreachable!("independent metadata checked by caller") };
            versions.push((
                identity.0,
                identity.1.as_str(),
                identity.2.as_str(),
                storage_i64(change.mutation.changed_at, "change timestamp")?,
                i64::from(change.mutation.conflict_rank),
                change.replica_id.to_string(),
                storage_i64(change.mutation.replica_seq.get(), "actor sequence")?,
                change.mutation.mutation_id.to_string(),
            ));
            if !snapshot.is_purged {
                let timestamp = storage_i64(change.mutation.changed_at, "change timestamp")?;
                if !snapshot.exists {
                    missing.push((content_hash.as_str(), timestamp));
                }
                metadata.push((*content_hash, value));
            }
        }
        if versions.is_empty() {
            continue;
        }
        if origin.is_none() {
            *origin = Some(TxOriginGuard::activate(tx)?);
        }
        crate::books::ensure_remote_books(tx, &missing)?;
        #[cfg(test)]
        let profile = profiling::Span::new("metadata_edit_batch");
        crate::metadata::update_independent_metadata(tx, &metadata)?;
        #[cfg(test)]
        drop(profile);
        tx.prepare_cached(include_str!("sql/record_versions_batch.sql"))?.execute([serde_json::to_string(&versions)?])?;
    }
    Ok(())
}

pub(super) fn snapshots(tx: &Transaction<'_>, identities: &[&(&'static str, String, String)]) -> Result<Vec<BookSnapshot>, Box<dyn std::error::Error>> {
    let keys = serde_json::to_string(identities)?;
    Ok(tx
        .prepare_cached(include_str!("sql/book_addition_snapshots.sql"))?
        .query_map([keys], |row| Ok(BookSnapshot { is_purged: row.get(6)?, exists: row.get(7)?, has_metadata: row.get(0)?, version: if row.get::<_, Option<i64>>(1)?.is_some() { StoredVersion::decode_row(row, 1)? } else { None } }))?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

/// All keys are distinct, so lifecycle rows can precede metadata and versions.
/// The caller rolls back the entire transaction if any stage fails.
pub(super) fn apply<'a>(tx: &'a Transaction<'a>, changes: &PreparedRemoteChanges<'_>, origin: &mut Option<TxOriginGuard<'a>>) -> Result<(), Box<dyn std::error::Error>> {
    for page in changes.changes.chunks(100) {
        let identities = page.iter().map(|(_, _, identity)| identity).collect::<Vec<_>>();
        let snapshots = snapshots(tx, &identities)?;
        let mut accepted = Vec::new();
        let mut books = Vec::new();
        let mut versions = Vec::new();
        for ((change, body, identity), snapshot) in page.iter().zip(&snapshots) {
            if !sync_state_version_wins(tx, change, identity.0, &identity.1, &identity.2, Some(snapshot))? {
                continue;
            }
            let MutationBody::BookLifecycle { content_hash, value: library_replica::BookLifecycleState::Present { metadata, added_at, format } } = body else { unreachable!("independent additions checked by caller") };
            books.push((content_hash.as_str(), metadata.title.as_str(), storage_i64(*added_at, "book addition timestamp")?, format.canonical_extension()));
            versions.push((
                identity.0,
                identity.1.as_str(),
                identity.2.as_str(),
                storage_i64(change.mutation.changed_at, "change timestamp")?,
                i64::from(change.mutation.conflict_rank),
                change.replica_id.to_string(),
                storage_i64(change.mutation.replica_seq.get(), "actor sequence")?,
                change.mutation.mutation_id.to_string(),
            ));
            accepted.push((content_hash, metadata, snapshot.has_metadata));
        }
        if accepted.is_empty() {
            continue;
        }
        if origin.is_none() {
            *origin = Some(TxOriginGuard::activate(tx)?);
        }
        crate::books::apply_remote_books_added(tx, &books)?;
        let metadata = accepted.iter().filter(|(_, _, has_metadata)| !has_metadata).map(|(hash, metadata, _)| (**hash, *metadata)).collect::<Vec<_>>();
        #[cfg(test)]
        let profile = profiling::Span::new("metadata_batch_write");
        crate::metadata::update_independent_metadata(tx, &metadata)?;
        #[cfg(test)]
        drop(profile);
        tx.prepare_cached(include_str!("sql/record_versions_batch.sql"))?.execute([serde_json::to_string(&versions)?])?;
        crate::file_work::queue_book_work_batch(tx, &accepted.iter().map(|(hash, _, _)| **hash).collect::<Vec<_>>())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn change(index: u64, timestamp: u64, body: Option<MutationBody>) -> ServerMutation {
        ServerMutation {
            mutation: library_replica::StateMutation {
                mutation_id: sync_common::MutationId::new(),
                replica_seq: sync_common::ReplicaSeq::new(timestamp).unwrap(),
                changed_at: timestamp,
                body: body.unwrap_or_else(|| MutationBody::BookLifecycle {
                    content_hash: test_support::fixture_content_hash(index),
                    value: library_replica::BookLifecycleState::Present {
                        metadata: library_replica::SyncBookMetadata {
                            title: format!("Title {timestamp}"),
                            contributors: vec![book_model::Contributor::new("Shared Author", None, book_model::AUTHOR_MARC_RELATOR_CODE).unwrap()],
                            book: book_model::BookMetadata {
                                languages: vec![book_model::LanguageTag::parse("sv-SE").unwrap()],
                                subjects: vec![book_model::BookSubject::new(None, "Distributed systems", "dc:subject", None, None).unwrap()],
                                rights: vec!["Copyright example".into()],
                                ..Default::default()
                            },
                        },
                        added_at: timestamp,
                        format: BookFormat::Epub,
                    },
                }),
            }
            .to_wire()
            .unwrap(),
            replica_id: test_support::fixture_replica_id("batch-regression"),
            revision: sync_common::LibraryRevision::new(timestamp).unwrap(),
        }
    }

    fn connection() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::schema::init_db(&conn).unwrap();
        conn
    }

    fn apply(conn: &mut Connection, prepared: &PreparedRemoteChanges<'_>, batched: bool) {
        let tx = crate::schema::library_write_transaction(conn).unwrap();
        if batched {
            apply_prepared_remote_changes_in_tx(&tx, prepared).unwrap();
        } else {
            let mut origin = None;
            for (change, body, identity) in &prepared.changes {
                apply_remote_change_inner(&tx, change, body, identity, &mut origin).unwrap();
            }
            if let Some(origin) = origin {
                origin.finish().unwrap();
            }
        }
        tx.commit().unwrap();
    }

    fn state(conn: &Connection) -> Vec<String> {
        conn.prepare(include_str!("sql/batching_tests/state.sql")).unwrap().query_map([], |row| row.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap()
    }

    #[test]
    fn independent_additions_match_ordered_reducer_across_bounded_snapshots() {
        let mut batched = connection();
        let mut ordered = connection();
        let incoming = (1..=205).map(|index| change(index, 50, None)).collect::<Vec<_>>();
        let prepared = prepare_remote_changes(&incoming).unwrap();
        assert!(independent_additions(&prepared));
        // These writes occur AFTER wire preparation. Snapshots must observe them.
        let seed = vec![
            change(1, 60, None),
            change(2, 10, None),
            change(3, 10, None),
            change(4, 10, None),
            change(3, 20, Some(MutationBody::BookLifecycle { content_hash: test_support::fixture_content_hash(3), value: library_replica::BookLifecycleState::Deleted { origin_folder_id: None } })),
            change(4, 20, Some(MutationBody::BookLifecycle { content_hash: test_support::fixture_content_hash(4), value: library_replica::BookLifecycleState::Purged })),
            change(2, 70, Some(MutationBody::Metadata { content_hash: test_support::fixture_content_hash(2), value: library_replica::SyncBookMetadata { title: "Metadata owns this title".into(), ..Default::default() } })),
        ];
        let seed = prepare_remote_changes(&seed).unwrap();
        apply(&mut batched, &seed, true);
        apply(&mut ordered, &seed, false);
        apply(&mut batched, &prepared, true);
        apply(&mut ordered, &prepared, false);
        assert_eq!(state(&batched), state(&ordered));
        assert!(state(&batched).iter().any(|row| row.contains("Metadata owns this title")));
        assert!(state(&batched).iter().any(|row| row.contains("Title 60")));
        // Replaying stale/equal versions must not replace queue tokens.
        let before = state(&batched);
        apply(&mut batched, &prepared, true);
        assert_eq!(state(&batched), before);
    }

    fn metadata_change(index: u64, timestamp: u64) -> ServerMutation {
        change(
            index,
            timestamp,
            Some(MutationBody::Metadata {
                content_hash: test_support::fixture_content_hash(index),
                value: library_replica::SyncBookMetadata {
                    title: format!("Edit {timestamp}"),
                    contributors: vec![book_model::Contributor::new("Edit Author", None, book_model::AUTHOR_MARC_RELATOR_CODE).unwrap()],
                    book: book_model::BookMetadata { rights: vec!["Updated rights".into()], ..Default::default() },
                },
            }),
        )
    }

    #[test]
    fn independent_metadata_matches_ordered_for_missing_deleted_purged_and_stale_books() {
        let mut batched = connection();
        let mut ordered = connection();
        let seed = vec![
            metadata_change(1, 60),
            change(2, 10, None),
            change(3, 10, None),
            change(2, 20, Some(MutationBody::BookLifecycle { content_hash: test_support::fixture_content_hash(2), value: library_replica::BookLifecycleState::Deleted { origin_folder_id: None } })),
            change(3, 20, Some(MutationBody::BookLifecycle { content_hash: test_support::fixture_content_hash(3), value: library_replica::BookLifecycleState::Purged })),
        ];
        let seed = prepare_remote_changes(&seed).unwrap();
        apply(&mut batched, &seed, true);
        apply(&mut ordered, &seed, false);
        let incoming = (1..=205).map(|index| metadata_change(index, 50)).collect::<Vec<_>>();
        let prepared = prepare_remote_changes(&incoming).unwrap();
        assert!(independent_metadata(&prepared));
        apply(&mut batched, &prepared, true);
        apply(&mut ordered, &prepared, false);
        assert_eq!(state(&batched), state(&ordered));
        let before = state(&batched);
        apply(&mut batched, &prepared, true);
        assert_eq!(state(&batched), before);
        let repeated = vec![metadata_change(4, 80), metadata_change(4, 70)];
        let repeated = prepare_remote_changes(&repeated).unwrap();
        assert!(!independent_metadata(&repeated));
        apply(&mut batched, &repeated, true);
        apply(&mut ordered, &repeated, false);
        assert_eq!(state(&batched), state(&ordered));
    }

    #[test]
    fn failed_metadata_batch_rolls_back_earlier_chunks() {
        let mut conn = connection();
        conn.execute_batch(include_str!("sql/batching_tests/reject_metadata_version.sql")).unwrap();
        let before = state(&conn);
        let incoming = (1..=205).map(|index| metadata_change(index, 50)).collect::<Vec<_>>();
        let prepared = prepare_remote_changes(&incoming).unwrap();
        let tx = crate::schema::library_write_transaction(&mut conn).unwrap();
        assert!(apply_prepared_remote_changes_in_tx(&tx, &prepared).is_err());
        tx.rollback().unwrap();
        assert_eq!(state(&conn), before);
    }

    #[test]
    fn metadata_snapshots_match_ordered_updates_for_existing_credits() {
        let mut batched = connection();
        let mut ordered = connection();
        let seed = (1..=3).map(|index| change(index, 10, None)).collect::<Vec<_>>();
        let prepared = prepare_remote_changes(&seed).unwrap();
        apply(&mut batched, &prepared, true);
        apply(&mut ordered, &prepared, false);
        let mut updates = prepared
            .changes
            .iter()
            .map(|(_, body, _)| {
                let MutationBody::BookLifecycle { content_hash, value: library_replica::BookLifecycleState::Present { metadata, .. } } = body else { unreachable!() };
                (*content_hash, metadata.clone())
            })
            .collect::<Vec<_>>();
        // Exact match, changed credits/book, and removal of all credits.
        updates[1].1.contributors.push(book_model::Contributor::new("Narrator", None, book_model::NARRATOR_MARC_RELATOR_CODE).unwrap());
        updates[1].1.book.rights = vec!["Updated rights".into()];
        updates[2].1.contributors.clear();
        let references = updates.iter().map(|(hash, metadata)| (*hash, metadata)).collect::<Vec<_>>();
        for (conn, batch) in [(&mut batched, true), (&mut ordered, false)] {
            let tx = crate::schema::library_write_transaction(conn).unwrap();
            let origin = TxOriginGuard::activate(&tx).unwrap();
            if batch {
                crate::metadata::update_independent_metadata(&tx, &references).unwrap();
            } else {
                for (hash, metadata) in &references {
                    crate::metadata::update_book_metadata_state_preserving_ids(&tx, *hash, metadata).unwrap();
                }
            }
            origin.finish().unwrap();
            tx.commit().unwrap();
        }
        assert_eq!(state(&batched), state(&ordered));
    }

    #[test]
    fn failed_bulk_version_write_rolls_back_books_metadata_and_queue() {
        let mut conn = connection();
        conn.execute_batch(include_str!("sql/batching_tests/reject_version.sql")).unwrap();
        let before = state(&conn);
        let incoming = (1..=205).map(|index| change(index, 50, None)).collect::<Vec<_>>();
        let prepared = prepare_remote_changes(&incoming).unwrap();
        let tx = crate::schema::library_write_transaction(&mut conn).unwrap();
        assert!(apply_prepared_remote_changes_in_tx(&tx, &prepared).is_err());
        assert_eq!(get_sync_metadata(&tx, "change_origin").unwrap().as_deref(), Some("local"));
        tx.rollback().unwrap();
        assert_eq!(state(&conn), before);
    }

    #[test]
    fn repeated_keys_and_mixed_lifecycle_pages_keep_ordered_reads() {
        let mut batched = connection();
        let mut ordered = connection();
        let incoming = vec![change(1, 10, None), change(1, 30, None), change(1, 20, None)];
        let prepared = prepare_remote_changes(&incoming).unwrap();
        assert!(!independent_additions(&prepared));
        apply(&mut batched, &prepared, true);
        apply(&mut ordered, &prepared, false);
        assert_eq!(state(&batched), state(&ordered));
        let mixed = vec![
            change(1, 40, Some(MutationBody::BookLifecycle { content_hash: test_support::fixture_content_hash(1), value: library_replica::BookLifecycleState::Deleted { origin_folder_id: None } })),
            change(1, 35, None),
            change(2, 50, None),
        ];
        let prepared = prepare_remote_changes(&mixed).unwrap();
        assert!(!independent_additions(&prepared));
        apply(&mut batched, &prepared, true);
        apply(&mut ordered, &prepared, false);
        assert_eq!(state(&batched), state(&ordered));
        assert!(crate::assets::queued_book_asset_state(&batched, &test_support::fixture_content_hash(1)).unwrap().is_none());
    }
}
