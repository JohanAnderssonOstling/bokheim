//! Coalesce metadata payloads without writing a persistent batching flag.
#[cfg(test)]
use crate::schema::init_db;
use crate::sync_state::get_sync_metadata;
use rusqlite::Connection;
use rusqlite::Transaction;
use rusqlite::functions::FunctionFlags;
#[cfg(test)]
use sync_common::ContentHash;

pub(super) fn configure(conn: &Connection) -> rusqlite::Result<()> {
    use std::sync::atomic::{AtomicBool, Ordering};
    let active = AtomicBool::new(false);
    conn.create_scalar_function("metadata_batch_flag", 1, FunctionFlags::SQLITE_UTF8, move |ctx| {
        Ok(match ctx.get::<Option<bool>>(0)? {
            Some(value) => active.swap(value, Ordering::Relaxed),
            None => active.load(Ordering::Relaxed),
        })
    })
}

struct Scope<'a> {
    conn: &'a Connection,
    owns_flag: bool,
    released: bool,
}
impl Drop for Scope<'_> {
    fn drop(&mut self) {
        if !self.released {
            let _ = self.conn.execute_batch(include_str!("sql/metadata_batch/drop_rollback.sql"));
        }
        if self.owns_flag {
            let _ = self.conn.query_row(include_str!("sql/metadata_batch/drop_select.sql"), [], |r| r.get::<_, bool>(0));
        }
    }
}

pub(super) fn run<T>(tx: &Transaction<'_>, write: impl FnOnce() -> rusqlite::Result<T>) -> rusqlite::Result<T> {
    let active: bool = tx.query_row(include_str!("sql/metadata_batch/run_select.sql"), [], |r| r.get(0))?;
    let owns_flag = !active && get_sync_metadata(tx, "change_origin")?.as_deref() != Some("remote");
    // Every nesting level owns a savepoint, even if only the outermost level flushes.
    tx.execute_batch("SAVEPOINT metadata_payload_batch;")?;
    let mut scope = Scope { conn: tx, owns_flag, released: false };
    if owns_flag {
        tx.query_row(include_str!("sql/metadata_batch/run_select_2.sql"), [], |r| r.get::<_, bool>(0))?;
    }
    let result = write()?;
    if owns_flag {
        tx.execute_batch(include_str!("sql/metadata_batch/run_insert.sql"))?;
    }
    tx.execute_batch("RELEASE metadata_payload_batch;")?;
    scope.released = true;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync_state::queries::SyncSql;
    fn setup() -> (Connection, ContentHash) {
        let mut conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        let hash = test_support::fixture_content_hash(992);
        let tx = conn.transaction().unwrap();
        super::super::metadata::insert_scanned_book(&tx, hash, "Initial", "Author").unwrap();
        tx.commit().unwrap();
        conn.execute_batch(include_str!("sql/metadata_batch/setup_delete.sql")).unwrap();
        (conn, hash)
    }
    fn count(conn: &Connection) -> i64 {
        conn.query_row(include_str!("sql/metadata_batch/count_select.sql"), [], |r| r.get(0)).unwrap()
    }
    #[test]
    fn nested_batch_serializes_final_metadata_once() {
        let (mut conn, hash) = setup();
        let tx = conn.transaction().unwrap();
        run(&tx, || {
            tx.execute(include_str!("sql/metadata_batch/nested_batch_serializes_final_metadata_once_update.sql"), [hash.as_str()])?;
            run(&tx, || {
                tx.execute(include_str!("sql/metadata_batch/nested_batch_serializes_final_metadata_once_update_2.sql"), [])?;
                tx.execute(include_str!("sql/metadata_batch/nested_batch_serializes_final_metadata_once_update_3.sql"), [hash.as_str()])?;
                super::super::descriptions::set(&tx, hash, "Final description")?;
                Ok(())
            })?;
            assert_eq!(count(&tx), 0);
            Ok(())
        })
        .unwrap();
        assert_eq!(count(&tx), 1);
        let matches: bool = tx.query_row(include_str!("sql/metadata_batch/nested_batch_serializes_final_metadata_once_select.sql"), [], |r| r.get(0)).unwrap();
        assert!(matches);
        tx.commit().unwrap();
    }
    #[test]
    fn failed_scope_can_be_caught_without_losing_later_updates() {
        let (mut conn, hash) = setup();
        let tx = conn.transaction().unwrap();
        let result: rusqlite::Result<()> = run(&tx, || {
            tx.execute(include_str!("sql/metadata_batch/failed_scope_can_be_caught_without_losing_later_updates_update.sql"), [])?;
            Err(rusqlite::Error::InvalidQuery)
        });
        assert!(result.is_err());
        assert_eq!(tx.query_row(include_str!("sql/metadata_batch/panic_resets_connection_flag_and_rolls_back_select_2.sql"), [], |r| r.get::<_, String>(0)).unwrap(), "Initial");
        assert_eq!(count(&tx), 0);
        tx.execute(include_str!("sql/metadata_batch/failed_scope_can_be_caught_without_losing_later_updates_update_2.sql"), [hash.as_str()]).unwrap();
        assert_eq!(count(&tx), 1);
        tx.commit().unwrap();
        assert_eq!(get_sync_metadata(&conn, "metadata_batch_active").unwrap(), None);
        assert_eq!(conn.query_row(include_str!("sql/metadata_batch/failed_scope_can_be_caught_without_losing_later_updates_select_2.sql"), [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    }
    #[test]
    fn remote_batch_does_not_echo() {
        let (mut conn, _) = setup();
        let tx = conn.transaction().unwrap();
        tx.set_sync_metadata("change_origin", "remote").unwrap();
        run(&tx, || {
            tx.execute(include_str!("sql/metadata_batch/remote_batch_does_not_echo_update.sql"), [])?;
            Ok(())
        })
        .unwrap();
        assert_eq!(count(&tx), 0);
        tx.commit().unwrap();
    }

    #[test]
    fn caught_nested_failure_rolls_back_only_inner_scope() {
        let (mut conn, _) = setup();
        let tx = conn.transaction().unwrap();
        run(&tx, || {
            tx.execute(include_str!("sql/metadata_batch/caught_nested_failure_rolls_back_only_inner_scope_update.sql"), [])?;
            let failed: rusqlite::Result<()> = run(&tx, || {
                tx.execute(include_str!("sql/metadata_batch/caught_nested_failure_rolls_back_only_inner_scope_update_2.sql"), [])?;
                Err(rusqlite::Error::InvalidQuery)
            });
            assert!(failed.is_err());
            assert_eq!(tx.query_row(include_str!("sql/metadata_batch/panic_resets_connection_flag_and_rolls_back_select_2.sql"), [], |r| r.get::<_, String>(0))?, "Outer");
            assert_eq!(count(&tx), 0);
            Ok(())
        })
        .unwrap();
        assert_eq!(count(&tx), 1);
        assert!(!tx.query_row(include_str!("sql/metadata_batch/run_select.sql"), [], |r| r.get::<_, bool>(0)).unwrap());
        tx.commit().unwrap();
    }
    #[test]
    fn empty_nested_batch_writes_nothing() {
        let (mut conn, _) = setup();
        let before = conn.total_changes();
        let tx = conn.transaction().unwrap();
        run(&tx, || run(&tx, || Ok(()))).unwrap();
        tx.commit().unwrap();
        assert_eq!(conn.total_changes(), before);
    }
    #[test]
    fn panic_resets_connection_flag_and_rolls_back() {
        let (mut conn, _) = setup();
        let tx = conn.transaction().unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _: rusqlite::Result<()> = run(&tx, || {
                tx.execute(include_str!("sql/metadata_batch/panic_resets_connection_flag_and_rolls_back_update.sql"), [])?;
                panic!("test unwind");
            });
        }));
        assert!(result.is_err());
        assert!(!tx.query_row(include_str!("sql/metadata_batch/run_select.sql"), [], |r| r.get::<_, bool>(0)).unwrap());
        assert_eq!(tx.query_row(include_str!("sql/metadata_batch/panic_resets_connection_flag_and_rolls_back_select_2.sql"), [], |r| r.get::<_, String>(0)).unwrap(), "Initial");
        tx.commit().unwrap();
    }

    #[test]
    #[ignore = "manual release microbenchmark; no external services"]
    fn benchmark_metadata_batch() {
        let mut times = [Vec::new(), Vec::new()];
        for _ in 0..7 {
            for batched in [false, true] {
                let (mut conn, _) = setup();
                let tx = conn.transaction().unwrap();
                for i in 0..31 {
                    super::super::metadata::insert_scanned_book(&tx, test_support::fixture_content_hash(1000 + i), "Initial", "Author").unwrap();
                }
                tx.commit().unwrap();
                conn.execute_batch(include_str!("sql/metadata_batch/benchmark_metadata_batch_delete.sql")).unwrap();
                let start = std::time::Instant::now();
                let tx = crate::schema::library_write_transaction(&mut conn).unwrap();
                let write = || {
                    tx.execute(include_str!("sql/metadata_batch/benchmark_metadata_batch_update.sql"), [])?;
                    tx.execute(include_str!("sql/metadata_batch/benchmark_metadata_batch_insert.sql"), [])?;
                    tx.execute(include_str!("sql/metadata_batch/benchmark_metadata_batch_update_2.sql"), [])?;
                    Ok(())
                };
                if batched {
                    run(&tx, write).unwrap();
                } else {
                    write().unwrap();
                }
                tx.commit().unwrap();
                times[usize::from(batched)].push(start.elapsed());
                assert_eq!(count(&conn), if batched { 32 } else { 64 });
            }
        }
        for values in &mut times {
            values.sort();
        }
        eprintln!("32-book transaction median: immediate={:?}, batched={:?}; metadata payloads 64 -> 32, plus 32 independent descriptions", times[0][3], times[1][3]);
    }

    #[test]
    #[ignore = "manual desktop disk-WAL contention benchmark; temporary fixtures only"]
    fn benchmark_desktop_writer_wait() {
        use std::time::{Duration, Instant};
        let mut holds = [Vec::new(), Vec::new()];
        let mut waits = [Vec::new(), Vec::new()];
        for _ in 0..9 {
            for batched in [false, true] {
                let root = tempfile::tempdir().unwrap();
                let path = root.path().join("library.db");
                let mut conn = Connection::open(&path).unwrap();
                init_db(&conn).unwrap();
                let tx = conn.transaction().unwrap();
                for i in 0..32 {
                    super::super::metadata::insert_scanned_book(&tx, test_support::fixture_content_hash(6000 + i), "Initial", "Author").unwrap();
                }
                tx.commit().unwrap();
                let other = Connection::open(&path).unwrap();
                crate::schema::configure_connection(&other).unwrap();
                other.busy_timeout(Duration::ZERO).unwrap();
                let tx = crate::schema::library_write_transaction(&mut conn).unwrap();
                let (send, receive) = std::sync::mpsc::sync_channel(0);
                let writer = std::thread::spawn(move || {
                    // Confirm actual contention before starting the measured workload.
                    let start = Instant::now();
                    let error = other.execute(include_str!("sql/metadata_batch/benchmark_desktop_writer_wait_update_3.sql"), []).unwrap_err();
                    assert_eq!(error.sqlite_error_code(), Some(rusqlite::ErrorCode::DatabaseBusy));
                    send.send(()).unwrap();
                    other.busy_timeout(Duration::from_secs(30)).unwrap();
                    other.execute(include_str!("sql/metadata_batch/benchmark_desktop_writer_wait_update_3.sql"), []).unwrap();
                    start.elapsed()
                });
                receive.recv().unwrap();
                let start = Instant::now();
                let write = || {
                    tx.execute(include_str!("sql/metadata_batch/benchmark_metadata_batch_update.sql"), [])?;
                    tx.execute(include_str!("sql/metadata_batch/benchmark_metadata_batch_insert.sql"), [])?;
                    tx.execute(include_str!("sql/metadata_batch/benchmark_metadata_batch_update_2.sql"), [])?;
                    Ok(())
                };
                if batched {
                    run(&tx, write).unwrap();
                } else {
                    write().unwrap();
                }
                tx.commit().unwrap();
                holds[usize::from(batched)].push(start.elapsed());
                waits[usize::from(batched)].push(writer.join().unwrap());
            }
        }
        for i in 0..2 {
            holds[i].sort();
            waits[i].sort();
            eprintln!(
                "disk WAL, 32 books, 9 trials, batched={}: write+commit median={:?} max={:?}; competing writer wait median={:?} max={:?} (SQLite busy-timeout backoff included)",
                i == 1,
                holds[i][4],
                holds[i][8],
                waits[i][4],
                waits[i][8]
            );
        }
    }
}
