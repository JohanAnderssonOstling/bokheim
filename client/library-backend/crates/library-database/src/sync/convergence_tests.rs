//! Compatibility fixtures: compare canonical winners and their visible projection.
use crate::query_methods::query_rows;
use crate::{Database, sync::apply};
use library_replica::{BookDetails, BookLifecycleState, DirectoryLifecycleState, LibraryStateEntry, ReadingPositionState};
use rusqlite::OptionalExtension;
use sync_common::{ContentHash, LibraryRevision, PullStateResponse, ROOT_DIR_ID, ReplicaSeq, StateDelivery, SyncCursor};

pub(super) fn db() -> Database {
    let db = Database::open(":memory:").unwrap();
    db.initialize_library().unwrap();
    db
}
pub(super) fn hash() -> ContentHash {
    ContentHash::new(&"a".repeat(64))
}
fn book_snapshot(db: &Database) -> (Option<String>, Option<String>, Vec<String>, bool) {
    let row = db.connection.fixtures_read_book_title_read_pos(&(hash()), |r| Ok((r.get(0)?, r.get(1)?))).optional().unwrap();
    let Some((title, pos)) = row else {
        let (bytes, _) = apply::registers::canonical_body(&db.connection, "book_lifecycle", &sync_common::StateKey::Book(hash()), "").unwrap().expect("missing book requires a known purge in this fixture");
        let body: LibraryStateEntry = sync_common::wire::decode(&bytes, sync_common::wire::MAX_DECODED_REQUEST_BYTES).unwrap();
        assert!(matches!(body, LibraryStateEntry::BookLifecycle { value: BookLifecycleState::Purged, .. }));
        return (None, None, vec![], true);
    };
    let languages = query_rows(|callback| db.connection.fixtures_read_book_language_language(callback), |r| r.get(0)).unwrap().collect::<Result<_, _>>().unwrap();
    (title, pos, languages, false)
}
fn tree(db: &Database) -> Vec<(String, String, String, bool)> {
    query_rows(|callback| db.connection.fixtures_read_dir_id_parent_id_name(callback), |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).unwrap().collect::<Result<_, _>>().unwrap()
}

#[test]
fn failed_projection_rolls_back_canonical_winners_and_local_outbox() {
    let replica = db();
    let dir = uuid::Uuid::from_u128(5);
    replica.create_directory_with_id(&dir, &ROOT_DIR_ID, &"Before".into()).unwrap();
    let before = apply::registers::canonical_body(&replica.connection, "directory_name", &sync_common::StateKey::Text(dir.to_string()), "").unwrap();
    let outbox = replica.sync_publishable_mutations().unwrap();
    replica.connection.fixtures_create_OF().unwrap();
    assert!(replica.move_directory(&dir, None, Some("After")).is_err());
    assert_eq!(apply::registers::canonical_body(&replica.connection, "directory_name", &sync_common::StateKey::Text(dir.to_string()), "").unwrap(), before);
    assert_eq!(replica.sync_publishable_mutations().unwrap(), outbox);
    assert_eq!(replica.connection.fixtures_read_dir_name(&(dir.to_string()), |r| r.get::<_, String>(0)).unwrap(), "Before");
}

#[test]
fn publication_is_lazy_and_in_flight_ack_preserves_new_local_edit() {
    let replica = db();
    let dir = uuid::Uuid::from_u128(1);
    replica.create_directory_with_id(&dir, &ROOT_DIR_ID, &"First".into()).unwrap();
    let pending: i64 = replica.connection.fixtures_read_sync_outbox(|r| r.get(0)).unwrap();
    assert_eq!(pending, 3, "a local edit queues identities, not copied payloads");
    let sent = replica.sync_publishable_mutations().unwrap();
    assert_eq!(sent, replica.sync_publishable_mutations().unwrap());
    replica.move_directory(&dir, None, Some("Second")).unwrap();
    replica.sync_acknowledge_mutations(&sent.iter().map(|m| m.publisher_seq).collect::<Vec<_>>()).unwrap();
    let next = replica.sync_publishable_mutations().unwrap();
    assert_eq!(next.len(), 1);
    assert!(matches!(&next[0].state.value,LibraryStateEntry::DirectoryName {name,..} if name=="Second"));
    assert!(!sent.iter().any(|m| m.publisher_seq == next[0].publisher_seq));
}

#[test]
fn publication_snapshot_survives_reopen() {
    let path = std::env::temp_dir().join(format!("publication-{}.sqlite3", uuid::Uuid::new_v4()));
    let replica = Database::open(&path).unwrap();
    replica.initialize_library().unwrap();
    replica.create_directory_with_id(&uuid::Uuid::from_u128(1), &ROOT_DIR_ID, &"Shelf".into()).unwrap();
    let sent = replica.sync_publishable_mutations().unwrap();
    drop(replica);
    let replica = Database::open(&path).unwrap();
    replica.initialize_library().unwrap();
    assert_eq!(sent, replica.sync_publishable_mutations().unwrap());
    replica.sync_acknowledge_mutations(&sent.iter().map(|m| m.publisher_seq).collect::<Vec<_>>()).unwrap();
    assert!(replica.sync_publishable_mutations().unwrap().is_empty());
    drop(replica);
    std::fs::remove_file(path).unwrap();
}

impl Database {
}

/// Model server acknowledgement only after proving these publications already
/// match its winning fields. Reconciliation must not invent a new edit/version.
pub(super) fn acknowledge_confirmed_reconciliation(replica: &Database, server: &[StateDelivery]) {
    let pending = replica.sync_publishable_mutations().unwrap();
    for publication in &pending {
        let wire = sync_common::transport::representation::<_, sync_common::OpaquePublication>(&publication).unwrap();
        assert!(wire.state.value.book.is_some() && wire.state.value.address.kind != "book_lifecycle", "reconciliation must only publish owned fields");
        let winner = server
            .iter()
            .filter(|event| {
                (event.state.value.address.kind.as_str(), &event.state.value.address.entity_key, event.state.value.address.entity_subkey.as_str())
                    == (wire.state.value.address.kind.as_str(), &wire.state.value.address.entity_key, wire.state.value.address.entity_subkey.as_str())
            })
            .max_by_key(|event| event.state.version.clone())
            .expect("publication must exist in the server fixture");
        assert_eq!(wire.state.value.bytes, winner.state.value.bytes);
        assert_eq!((wire).state.version.clone(), winner.state.version.clone());
    }
    replica.sync_acknowledge_mutations(&pending.iter().map(|m| m.publisher_seq).collect::<Vec<_>>()).unwrap();
}

#[cfg(test)]
use crate::sql_fixtures::FixturesSql;
