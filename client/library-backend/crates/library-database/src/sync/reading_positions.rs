//! Durable reading-position work. Publication timing belongs to the coordinator.

use crate::sync::SyncOutboxSql;
use crate::{Database, DatabaseError};
use std::collections::HashSet;
use sync_common::BookId;

impl Database {
    pub fn sync_queued_reading_positions(&self) -> Result<HashSet<BookId>, DatabaseError> {
        let mut queued = HashSet::new();
        self.connection.outbox_reading_positions(|row| {
            queued.insert(row.get::<_, BookId>(0)?);
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

    fn hash() -> BookId {
        BookId::from_bytes([7; 32])
    }

    fn database(path: impl AsRef<std::path::Path>) -> Database {
        let db = Database::open(path).unwrap();
        db.initialize_library().unwrap();
        db.set_asset_storage_policy(false, false).unwrap();
        db.connection.fixtures_insert_book_7(&hash()).unwrap();
        db
    }

    #[test]
    fn bounded_outbox_scan_reaches_ready_work_after_a_full_page_of_deferred_positions() {
        let db = database(":memory:");
        let mut waiting = HashSet::new();
        for index in 0u64..300 {
            let mut bytes = [0; 32];
            bytes[..8].copy_from_slice(&index.to_le_bytes());
            let book = BookId::from_bytes(bytes);
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
        let mut next = db.sync_prepare_outbox_excluding_positions(Default::default()).unwrap();
        assert!(db.sync_next_outbox_page(&mut scan).unwrap().is_empty());
        assert_eq!(db.sync_next_outbox_page(&mut next).unwrap().len(), 1);
    }

    #[test]
    fn unchanged_positions_and_toc_only_updates_do_not_report_a_reading_change() {
        let db = database(":memory:");
        assert!(db.update_reading_position(&hash(), "epubcfi(/6/2)", Some(10.0), None).unwrap());
        assert!(!db.update_reading_position(&hash(), "epubcfi(/6/2)", None, Some("Chapter 1")).unwrap());
    }
}

// Async access owns its arguments until the synchronous database call completes.
database_methods! {
    fn sync_queued_reading_positions() -> HashSet<BookId>;
    arguments () = ();
    execute database, arguments => database.sync_queued_reading_positions();
}
