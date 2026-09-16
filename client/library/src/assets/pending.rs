use rusqlite::Connection;

/// Cheap transfer candidates from committed local state. Files are checked only
/// when building the actual transfer plan, so stale availability may overestimate work.
pub fn has_pending_asset_work(conn: &Connection) -> rusqlite::Result<bool> {
    conn.query_row(include_str!("sql/pending/has_pending_asset_work_with.sql"), [], |row| row.get(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("sql/pending/fixture_create.sql")).unwrap();
        conn
    }

    #[test]
    fn synchronized_local_books_without_covers_have_no_asset_work() {
        let conn = fixture();
        assert!(!has_pending_asset_work(&conn).unwrap());
        conn.execute(include_str!("sql/pending/synchronized_local_books_without_covers_have_no_asset_work_delete.sql"), []).unwrap();
        assert!(!has_pending_asset_work(&conn).unwrap(), "missing availability cache is not upload intent");
        conn.execute(include_str!("sql/pending/deleted_and_unreachable_placements_do_not_schedule_uploads_insert.sql"), []).unwrap();
        assert!(has_pending_asset_work(&conn).unwrap(), "a newly scanned local book needs upload");
        conn.execute(include_str!("sql/pending/synchronized_local_books_without_covers_have_no_asset_work_insert_2.sql"), []).unwrap();
        assert!(has_pending_asset_work(&conn).unwrap(), "older server availability cannot suppress a queued edit");
    }

    #[test]
    fn thumbnail_generation_and_download_completion_settle_work() {
        let conn = fixture();
        conn.execute(include_str!("sql/pending/remote_books_need_covers_unless_generation_is_pending_update_2.sql"), []).unwrap();
        assert!(!has_pending_asset_work(&conn).unwrap());
        conn.execute(include_str!("sql/pending/remote_books_need_covers_unless_generation_is_pending_update_3.sql"), []).unwrap();
        assert!(has_pending_asset_work(&conn).unwrap(), "generated cover needs upload");
        conn.execute(include_str!("sql/pending/remote_books_need_covers_unless_generation_is_pending_insert.sql"), []).unwrap();
        assert!(!has_pending_asset_work(&conn).unwrap());
        conn.execute(include_str!("sql/pending/thumbnail_generation_and_download_completion_settle_work_delete.sql"), []).unwrap();
        assert!(has_pending_asset_work(&conn).unwrap(), "missing known remote cover needs download");
        conn.execute(include_str!("sql/pending/thumbnail_generation_and_download_completion_settle_work_insert_2.sql"), []).unwrap();
        assert!(!has_pending_asset_work(&conn).unwrap(), "downloaded cover is settled");
    }

    #[test]
    fn remote_books_need_covers_unless_generation_is_pending() {
        let conn = fixture();
        conn.execute(include_str!("sql/pending/remote_books_need_covers_unless_generation_is_pending_update.sql"), []).unwrap();
        assert!(has_pending_asset_work(&conn).unwrap());
        conn.execute(include_str!("sql/pending/remote_books_need_covers_unless_generation_is_pending_update_2.sql"), []).unwrap();
        assert!(!has_pending_asset_work(&conn).unwrap());
        conn.execute(include_str!("sql/pending/remote_books_need_covers_unless_generation_is_pending_update_3.sql"), []).unwrap();
        conn.execute(include_str!("sql/pending/remote_books_need_covers_unless_generation_is_pending_insert.sql"), []).unwrap();
        assert!(!has_pending_asset_work(&conn).unwrap());
        conn.execute(include_str!("sql/pending/remote_books_need_covers_unless_generation_is_pending_insert_2.sql"), []).unwrap();
        assert!(has_pending_asset_work(&conn).unwrap(), "explicit book downloads remain independent");
    }

    #[test]
    fn upload_rejections_expire_independently_for_books_and_covers() {
        let conn = fixture();
        conn.execute(include_str!("sql/pending/deleted_and_unreachable_placements_do_not_schedule_uploads_insert.sql"), []).unwrap();
        conn.execute(include_str!("sql/pending/deleted_and_unreachable_placements_do_not_schedule_uploads_delete.sql"), []).unwrap();
        conn.execute(include_str!("sql/pending/upload_rejections_expire_independently_for_books_and_covers_insert_2.sql"), []).unwrap();
        assert!(!has_pending_asset_work(&conn).unwrap());
        conn.execute(include_str!("sql/pending/remote_books_need_covers_unless_generation_is_pending_update_3.sql"), []).unwrap();
        assert!(has_pending_asset_work(&conn).unwrap(), "book rejection must not suppress cover upload");
        conn.execute(include_str!("sql/pending/upload_rejections_expire_independently_for_books_and_covers_insert_3.sql"), []).unwrap();
        assert!(!has_pending_asset_work(&conn).unwrap());
        conn.execute(include_str!("sql/pending/upload_rejections_expire_independently_for_books_and_covers_update_2.sql"), []).unwrap();
        assert!(has_pending_asset_work(&conn).unwrap());
    }

    #[test]
    fn rejected_upload_retries_after_five_minutes() {
        let conn = fixture();
        conn.execute(include_str!("sql/pending/deleted_and_unreachable_placements_do_not_schedule_uploads_insert.sql"), []).unwrap();
        conn.execute(include_str!("sql/pending/deleted_and_unreachable_placements_do_not_schedule_uploads_delete.sql"), []).unwrap();
        conn.execute(include_str!("sql/pending/upload_rejections_expire_independently_for_books_and_covers_insert_2.sql"), []).unwrap();
        assert!(!has_pending_asset_work(&conn).unwrap());
        let deadline = super::super::rejections::next_upload_retry_at(&conn).unwrap().unwrap();
        conn.execute(include_str!("sql/pending/age_book_rejection.sql"), [60]).unwrap();
        assert!(!has_pending_asset_work(&conn).unwrap(), "do not retry immediately");
        assert_eq!(super::super::rejections::next_upload_retry_at(&conn).unwrap(), Some(deadline - 60));
        conn.execute(include_str!("sql/pending/age_book_rejection.sql"), [241]).unwrap();
        assert!(has_pending_asset_work(&conn).unwrap(), "retry after five minutes, not a day");
        assert_eq!(super::super::rejections::next_upload_retry_at(&conn).unwrap(), None);
    }

    #[test]
    fn deleted_and_unreachable_placements_do_not_schedule_uploads() {
        let conn = fixture();
        conn.execute(include_str!("sql/pending/deleted_and_unreachable_placements_do_not_schedule_uploads_insert.sql"), []).unwrap();
        conn.execute(include_str!("sql/pending/deleted_and_unreachable_placements_do_not_schedule_uploads_delete.sql"), []).unwrap();
        for sql in [
            include_str!("sql/pending/deleted_and_unreachable_placements_do_not_schedule_uploads_update.sql"),
            include_str!("sql/pending/deleted_and_unreachable_placements_do_not_schedule_uploads_update_2.sql"),
            include_str!("sql/pending/deleted_and_unreachable_placements_do_not_schedule_uploads_update_3.sql"),
        ] {
            conn.execute_batch(include_str!("sql/pending/deleted_and_unreachable_placements_do_not_schedule_uploads_update_4.sql")).unwrap();
            conn.execute(sql, []).unwrap();
            assert!(!has_pending_asset_work(&conn).unwrap(), "{sql}");
        }
    }
}
