use super::*;
use crate::assets::*;
use std::cell::Cell;
use std::time::Instant;

thread_local! { static STATEMENTS: Cell<usize> = const { Cell::new(0) }; }

fn fixture(count: usize) -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    crate::schema::init_db(&conn).unwrap();
    crate::sync_state::set_sync_metadata(&conn, "change_origin", "remote").unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    for index in 0..count {
        let hash = ContentHash::new(&format!("{index:064x}"));
        tx.execute(include_str!("../sql/planning/tests/book.sql"), [hash.as_str()]).unwrap();
        tx.execute(include_str!("../sql/planning/tests/placement.sql"), rusqlite::params![hash.as_str(), format!("{index}.epub")]).unwrap();
    }
    tx.execute_batch(include_str!("../sql/planning/tests/clear_file_work.sql")).unwrap();
    tx.commit().unwrap();
    conn
}

fn assert_matches_old(conn: &Connection, row: &AssetCandidate) {
    let old = queued_book_asset_state(conn, &row.content_hash).unwrap().unwrap();
    assert!(row.live);
    assert_eq!(row.path, get_book_file_path(conn, row.content_hash).unwrap());
    assert_eq!(row.book_upload_allowed, old.book_upload_allowed);
    assert_eq!(row.thumbnail_upload_allowed, old.thumbnail_upload_allowed);
    assert_eq!(row.thumbnail_remote, old.thumbnail_remote);
    assert_eq!(row.thumbnail_pending, old.thumbnail_pending);
    assert_eq!(row.requested, has_requested_book_download(conn, &row.content_hash).unwrap());
    assert_eq!(row.rejected, has_recent_rejected_upload(conn, &row.content_hash).unwrap());
    assert_eq!(row.file_work_pending, crate::file_work::has_pending_book_file_work(conn, &row.content_hash).unwrap());
}

#[test]
fn candidate_pages_match_existing_state_and_hold_a_fixed_watermark() {
    let conn = fixture(95);
    for index in 0..95 {
        let hash = ContentHash::new(&format!("{index:064x}"));
        match index % 4 {
            0 => crate::thumbnail_jobs::complete_thumbnail_work(&conn, &hash, true).unwrap(),
            1 => record_remote_asset(&conn, library_replica::BlobKind::Thumbnail, &hash).unwrap(),
            2 => record_rejected_asset_upload(&conn, library_replica::BlobKind::Book, &hash, "test").unwrap(),
            _ => crate::file_work::queue_book_work(&conn, &hash).unwrap(),
        }
    }
    let through = asset_work_watermark(&conn).unwrap();
    let mut after = 0;
    let mut count = 0;
    loop {
        let rows = asset_candidate_page(&conn, after, through).unwrap();
        if rows.is_empty() {
            break;
        }
        assert!(rows.len() <= 32);
        for row in &rows {
            assert_matches_old(&conn, row);
        }
        after = rows.last().unwrap().work_id;
        count += rows.len();
    }
    assert_eq!(count, 95);
    enqueue_asset_work(&conn, &ContentHash::new(&format!("{:064x}", 0))).unwrap();
    assert!(asset_candidate_page(&conn, after, through).unwrap().is_empty());
}

#[test]
fn completion_preserves_new_generations_and_new_file_work() {
    let conn = fixture(3);
    let rows = asset_candidate_page(&conn, 0, asset_work_watermark(&conn).unwrap()).unwrap();
    enqueue_asset_work(&conn, &rows[0].content_hash).unwrap();
    crate::file_work::queue_book_work(&conn, &rows[1].content_hash).unwrap();
    complete_asset_candidates(&conn, &rows.iter().map(|row| (row.work_id, true)).collect::<Vec<_>>()).unwrap();
    let remaining = asset_candidate_page(&conn, 0, asset_work_watermark(&conn).unwrap()).unwrap();
    assert_eq!(remaining.len(), 2);
    assert!(remaining.iter().any(|row| row.content_hash == rows[0].content_hash && row.work_id != rows[0].work_id));
    assert!(remaining.iter().any(|row| row.work_id == rows[1].work_id));
    complete_asset_candidates(&conn, &[(rows[1].work_id, false)]).unwrap();
    assert_eq!(asset_candidate_page(&conn, 0, asset_work_watermark(&conn).unwrap()).unwrap().len(), 1);
}

#[test]
#[ignore = "explicit release performance comparison"]
fn benchmark_asset_candidate_pages() {
    let conn = fixture(10_000);
    if std::env::var_os("ASSET_PLANNER_EXPLAIN").is_some() {
        let sql = concat!(include_str!("../sql/planning/tests/explain.sql"), include_str!("../sql/planning/candidate_page.sql"));
        let mut query = conn.prepare(sql).unwrap();
        for detail in query.query_map([0, i64::MAX], |row| row.get::<_, String>(3)).unwrap() {
            eprintln!("{}", detail.unwrap());
        }
    }
    let through = asset_work_watermark(&conn).unwrap();
    conn.trace_v2(
        rusqlite::trace::TraceEventCodes::SQLITE_TRACE_STMT,
        Some(|event| {
            if matches!(event, rusqlite::trace::TraceEvent::Stmt(..)) {
                STATEMENTS.with(|count| count.set(count.get() + 1));
            }
        }),
    );
    for batched in [false, true] {
        STATEMENTS.with(|count| count.set(0));
        let started = Instant::now();
        let mut after = 0;
        let mut count = 0;
        loop {
            if batched {
                let rows = asset_candidate_page(&conn, after, through).unwrap();
                if rows.is_empty() {
                    break;
                }
                count += rows.len();
                after = rows.last().unwrap().work_id;
            } else {
                let rows = asset_work_page(&conn, after, through).unwrap();
                if rows.is_empty() {
                    break;
                }
                for (_, hash) in &rows {
                    std::hint::black_box(queued_book_asset_state(&conn, hash).unwrap());
                    std::hint::black_box(get_book_file_path(&conn, *hash).unwrap());
                    std::hint::black_box(has_requested_book_download(&conn, hash).unwrap());
                    std::hint::black_box(has_recent_rejected_upload(&conn, hash).unwrap());
                    std::hint::black_box(crate::file_work::has_pending_book_file_work(&conn, hash).unwrap());
                }
                count += rows.len();
                after = rows.last().unwrap().0;
            }
        }
        assert_eq!(count, 10_000);
        assert_eq!(STATEMENTS.with(Cell::get), if batched { 314 } else { 50_314 });
        eprintln!("batched={batched} candidates={count} statements={} elapsed={:?}", STATEMENTS.with(Cell::get), started.elapsed());
    }
}

#[test]
fn nested_placements_and_ancestor_work_match_existing_queries() {
    let conn = fixture(2);
    conn.execute_batch(include_str!("../sql/planning/tests/nested.sql")).unwrap();
    crate::file_work::queue_directory_work(&conn, &"10000000-0000-0000-0000-000000000000".parse().unwrap()).unwrap();
    let rows = asset_candidate_page(&conn, 0, asset_work_watermark(&conn).unwrap()).unwrap();
    for row in &rows {
        assert_matches_old(&conn, row);
        assert!(row.path.as_ref().unwrap().as_str().starts_with("/Shelf/Nested/"));
        assert!(row.file_work_pending);
    }
    complete_asset_candidates(&conn, &rows.iter().map(|row| (row.work_id, true)).collect::<Vec<_>>()).unwrap();
    assert_eq!(asset_work_page(&conn, 0, asset_work_watermark(&conn).unwrap()).unwrap().len(), 2);
    conn.execute_batch(include_str!("../sql/planning/tests/source_projection.sql")).unwrap();
    crate::file_work::queue_directory_work(&conn, &"30000000-0000-0000-0000-000000000000".parse().unwrap()).unwrap();
    for row in asset_candidate_page(&conn, 0, asset_work_watermark(&conn).unwrap()).unwrap() {
        assert_matches_old(&conn, &row);
        assert!(row.file_work_pending, "a materialized source outside the live placement remains pending");
    }
    complete_asset_candidates(&conn, &rows.iter().map(|row| (row.work_id, true)).collect::<Vec<_>>()).unwrap();
    assert_eq!(asset_work_page(&conn, 0, asset_work_watermark(&conn).unwrap()).unwrap().len(), 2);
    conn.execute(include_str!("../sql/planning/tests/delete_book.sql"), [rows[0].content_hash.as_str()]).unwrap();
    let deleted = asset_candidate_page(&conn, 0, asset_work_watermark(&conn).unwrap()).unwrap().into_iter().find(|row| row.content_hash == rows[0].content_hash).unwrap();
    assert!(!deleted.live);
    assert!(deleted.path.is_none());
}
