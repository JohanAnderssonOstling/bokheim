//! Durable reading-position work. Publication timing belongs to the coordinator.

use crate::sync::SyncOutboxSql;
use crate::{Database, DatabaseError};
use std::collections::HashSet;
use sync_common::ContentHash;

impl Database {
    pub fn sync_queued_reading_positions(&self) -> Result<HashSet<ContentHash>, DatabaseError> {
        let mut queued = HashSet::new();
        self.connection.outbox_reading_positions(|row| {
            queued.insert(row.get::<_, ContentHash>(0)?);
            Ok(())
        })?;
        Ok(queued)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sql_fixtures::FixturesSql;
    use library_replica::LibraryStateEntry;
    use sync_common::ROOT_DIR_ID;

    fn hash() -> ContentHash {
        ContentHash::from_bytes([7; 32])
    }

    fn database(path: impl AsRef<std::path::Path>) -> Database {
        let db = Database::open(path).unwrap();
        db.initialize_library().unwrap();
        db.set_asset_storage_policy(false, false).unwrap();
        db.connection.fixtures_insert_book_7(&hash()).unwrap();
        db
    }

    #[test]
    fn positions_are_durable_immediately_and_publication_policy_is_supplied_by_the_caller() {
        let db = database(":memory:");
        assert!(db.update_reading_position(&hash(), "epubcfi(/6/2)", Some(10.0), None).unwrap());
        assert_eq!(db.reading_position(&hash()).unwrap().as_deref(), Some("epubcfi(/6/2)"));
        assert!(db.stored_sync_status().unwrap().has_unsynced_changes);
        db.create_directory_with_id(&uuid::Uuid::from_u128(1), &ROOT_DIR_ID, &"Shelf".into()).unwrap();
        let mut scan = db.sync_prepare_outbox_excluding_positions(HashSet::from([hash()])).unwrap();
        let mut selected = Vec::new();
        loop {
            let page = db.sync_next_outbox_page(&mut scan).unwrap();
            if page.is_empty() {
                break;
            }
            selected.extend(page);
        }
        assert!(selected.iter().any(|p| matches!(p.state.value, LibraryStateEntry::DirectoryName { .. })));
        assert!(!selected.iter().any(|p| matches!(p.state.value, LibraryStateEntry::ReadingPosition { .. })));
        assert_eq!(db.sync_queued_reading_positions().unwrap(), HashSet::from([hash()]));
        assert!(db.sync_publishable_mutations().unwrap().iter().any(|p| matches!(p.state.value, LibraryStateEntry::ReadingPosition { .. })));
    }

    #[test]
    fn bounded_outbox_scan_reaches_ready_work_after_a_full_page_of_deferred_positions() {
        let db = database(":memory:");
        let mut waiting = HashSet::new();
        for index in 0u64..300 {
            let mut bytes = [0; 32];
            bytes[..8].copy_from_slice(&index.to_le_bytes());
            let book = ContentHash::from_bytes(bytes);
            db.connection.fixtures_insert_book_7(&book).unwrap();
            db.update_reading_position(&book, "epubcfi(/6/2)", Some(10.0), None).unwrap();
            waiting.insert(book);
        }
        db.create_directory_with_id(&uuid::Uuid::from_u128(1), &ROOT_DIR_ID, &"Ready shelf".into()).unwrap();
        let mut scan = db.sync_prepare_outbox_excluding_positions(waiting).unwrap();
        let page = db.sync_next_outbox_page(&mut scan).unwrap();
        assert!(!page.is_empty());
        assert!(page.iter().all(|p| !matches!(p.state.value, LibraryStateEntry::ReadingPosition { .. })));
        assert!(db.sync_next_outbox_page(&mut scan).unwrap().is_empty());
    }

    #[test]
    fn prepared_scan_keeps_exclusions_until_the_next_scan() {
        let db = database(":memory:");
        db.update_reading_position(&hash(), "epubcfi(/6/2)", Some(10.0), None).unwrap();
        let mut scan = db.sync_prepare_outbox_excluding_positions(HashSet::from([hash()])).unwrap();
        let mut next = db.sync_prepare_outbox().unwrap();
        assert!(db.sync_next_outbox_page(&mut scan).unwrap().is_empty());
        assert_eq!(db.sync_next_outbox_page(&mut next).unwrap().len(), 1);
    }

    #[test]
    fn unchanged_positions_and_toc_only_updates_do_not_report_a_reading_change() {
        let db = database(":memory:");
        assert!(db.update_reading_position(&hash(), "epubcfi(/6/2)", Some(10.0), None).unwrap());
        assert!(!db.update_reading_position(&hash(), "epubcfi(/6/2)", None, Some("Chapter 1")).unwrap());
    }

    #[test]
    fn a_new_position_survives_acknowledging_an_older_snapshot() {
        let db = database(":memory:");
        db.update_reading_position(&hash(), "epubcfi(/6/2)", Some(10.0), None).unwrap();
        let sent = db.sync_publishable_mutations().unwrap();
        assert_eq!(sent.len(), 1);
        db.update_reading_position(&hash(), "epubcfi(/6/4)", Some(20.0), None).unwrap();
        db.sync_acknowledge_mutations(&[sent[0].publisher_seq]).unwrap();
        let newer = db.sync_publishable_mutations().unwrap();
        assert_eq!(newer.len(), 1);
        assert_ne!(newer[0].publisher_seq, sent[0].publisher_seq);
        assert!(matches!(&newer[0].state.value, LibraryStateEntry::ReadingPosition { value, .. } if value.location.as_str() == "epubcfi(/6/4)"));
    }

    #[test]
    fn reopening_can_publish_the_latest_durable_position_without_waiting_again() {
        let path = std::env::temp_dir().join(format!("reading-debounce-{}.sqlite", uuid::Uuid::new_v4()));
        let db = database(&path);
        db.update_reading_position(&hash(), "epubcfi(/6/2)", Some(10.0), None).unwrap();
        db.update_reading_position(&hash(), "epubcfi(/6/4)", Some(20.0), None).unwrap();
        drop(db);
        let db = Database::open(&path).unwrap();
        db.initialize_library().unwrap();
        let pending = db.sync_publishable_mutations().unwrap();
        assert_eq!(pending.len(), 1);
        assert!(matches!(&pending[0].state.value, LibraryStateEntry::ReadingPosition { value, .. } if value.location.as_str() == "epubcfi(/6/4)"));
        drop(db);
        std::fs::remove_file(path).unwrap();
    }
}
