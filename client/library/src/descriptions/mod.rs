//! Canonical description storage, separate from frequently accessed book rows.
#[cfg(test)]
use crate::schema::init_db;
#[cfg(test)]
use crate::schema::library_write_transaction;
#[cfg(test)]
use crate::sync_state::SyncSql;
use include_sqlite_sql::include_sql;
use rusqlite::Connection;
use sync_common::ContentHash;

/// Retain the small inspection-completion flag on book; the text lives only in
/// book_description. Empty, never-stored descriptions need no additional row.
pub(super) fn set(conn: &Connection, hash: ContentHash, description: &str) -> rusqlite::Result<()> {
    conn.set_description(hash.as_str(), description)?;
    conn.mark_description_scanned(hash.as_str())?;
    Ok(())
}

include_sql!("src/descriptions/sql/descriptions.sql");

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync_state::set_sync_metadata;

    #[test]
    fn description_edits_sync_invalidate_browse_and_cascade_without_book_text() {
        let mut conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        let hash = test_support::fixture_content_hash(568);
        conn.execute(include_str!("sql/descriptions/description_edits_sync_invalidate_browse_and_cascade_without_book_text_insert.sql"), [hash.as_str()]).unwrap();
        let revision: i64 = conn.query_row(include_str!("sql/descriptions/description_edits_sync_invalidate_browse_and_cascade_without_book_text_select.sql"), [], |r| r.get(0)).unwrap();
        let tx = library_write_transaction(&mut conn).unwrap();
        super::super::metadata::update_book_description(&tx, hash, "Description").unwrap();
        tx.commit().unwrap();
        assert!(conn.query_row(include_str!("sql/descriptions/description_edits_sync_invalidate_browse_and_cascade_without_book_text_select.sql"), [], |r| r.get::<_, i64>(0)).unwrap() > revision);
        assert!(conn.query_row(include_str!("sql/descriptions/description_edits_sync_invalidate_browse_and_cascade_without_book_text_select_3.sql"), [hash.as_str()], |r| r.get::<_, bool>(0)).unwrap());
        let changes = conn.total_changes();
        let tx = library_write_transaction(&mut conn).unwrap();
        super::super::metadata::update_book_description(&tx, hash, "Description").unwrap();
        tx.commit().unwrap();
        assert_eq!(conn.total_changes(), changes);
        conn.execute(include_str!("sql/descriptions/description_edits_sync_invalidate_browse_and_cascade_without_book_text_delete.sql"), []).unwrap();
        set_sync_metadata(&conn, "change_origin", "remote").unwrap();
        let tx = library_write_transaction(&mut conn).unwrap();
        super::super::metadata::update_book_description(&tx, hash, "Remote description").unwrap();
        tx.commit().unwrap();
        assert_eq!(conn.query_row(include_str!("sql/descriptions/outbox_count_select.sql"), [], |r| r.get::<_, i64>(0)).unwrap(), 0);
        conn.execute(include_str!("sql/descriptions/description_edits_sync_invalidate_browse_and_cascade_without_book_text_delete_2.sql"), [hash.as_str()]).unwrap();
        assert_eq!(conn.query_row(include_str!("sql/descriptions/description_edits_sync_invalidate_browse_and_cascade_without_book_text_select_5.sql"), [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    }
}
