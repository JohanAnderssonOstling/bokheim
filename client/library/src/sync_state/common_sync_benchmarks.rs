//! Local storage/encoding costs, not network or storage-queue latency.
use super::*;
use crate::remote_reducer;
use crate::sync_state::SyncSql;
use library_replica::ReadingPositionState;
use library_replica::SyncBookMetadata;
use library_replica::MutationBody;
use library_replica::StateMutation;
use std::time::{Duration, Instant};
use sync_common::ServerMutation;
use sync_common::{MutationId, ReplicaSeq};

fn fixture(conn: &mut Connection, books: i32, description_bytes: usize) {
    init_db(conn).unwrap();
    let tx = library_write_transaction(conn).unwrap();
    for id in 1..=books {
        tx.execute(include_str!("sql/common_sync_benchmarks/fixture_insert.sql"), [format!("{id:064x}")]).unwrap();
    }
    tx.execute(include_str!("sql/common_sync_benchmarks/fixture_insert_2.sql"), ["x".repeat(description_bytes)]).unwrap();
    tx.execute(include_str!("sql/common_sync_benchmarks/fixture_update.sql"), []).unwrap();
    tx.execute(include_str!("sql/common_sync_benchmarks/fixture_delete.sql"), []).unwrap();
    tx.commit().unwrap();
}

fn median(mut values: Vec<Duration>) -> Duration {
    values.sort();
    values[values.len() / 2]
}

#[test]
fn prepared_remote_page_rechecks_versions_after_local_edit() {
    let mut conn = Connection::open_in_memory().unwrap();
    fixture(&mut conn, 1, 0);
    let changed_at = unix_millis().unwrap() as u64 + 10000;
    let incoming = ServerMutation {
        mutation: StateMutation {
            mutation_id: MutationId::new(),
            body: MutationBody::Metadata { content_hash: ContentHash::new(&format!("{:064x}", 1)), value: SyncBookMetadata { title: "Remote stale title".into(), ..Default::default() } },
            changed_at,
            replica_seq: ReplicaSeq::new(1).unwrap(),
        }
        .to_wire()
        .unwrap(),
        replica_id: test_support::fixture_replica_id("prepared"),
        revision: sync_common::LibraryRevision::new(1).unwrap(),
    };
    let changes = [incoming];
    let prepared = crate::remote_reducer::prepare_remote_changes(&changes).unwrap();
    conn.execute(include_str!("sql/common_sync_benchmarks/prepared_remote_page_rechecks_versions_after_local_edit_update.sql"), []).unwrap();
    conn.execute(include_str!("sql/common_sync_benchmarks/prepared_remote_page_rechecks_versions_after_local_edit_update_2.sql"), [(changed_at + 1) as i64]).unwrap();
    let tx = library_write_transaction(&mut conn).unwrap();
    crate::remote_reducer::apply_prepared_remote_changes_in_tx(&tx, &prepared).unwrap();
    tx.commit().unwrap();
    assert_eq!(conn.query_row(include_str!("sql/common_sync_benchmarks/prepared_remote_page_rechecks_versions_after_local_edit_select.sql"), [], |row| row.get::<_, String>(0)).unwrap(), "New local title");
    assert_eq!(get_sync_metadata(&conn, "change_origin").unwrap().as_deref(), Some("local"));
}

#[test]
#[ignore = "manual release incoming decode lock-duration comparison"]
fn benchmark_prepared_remote_writer() {
    let roots = [tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap()];
    let mut conns: Vec<_> = roots
        .iter()
        .map(|root| {
            let mut conn = Connection::open(root.path().join("library.db")).unwrap();
            fixture(&mut conn, 128, 10240);
            conn.pragma_update(None, "synchronous", "FULL").unwrap();
            conn
        })
        .collect();
    let base = unix_millis().unwrap() as u64 + 10000;
    for (case, count, bytes, stale) in [(0, 32, 1024, false), (1, 32, 65536, false), (2, 128, 65536, true)] {
        let mut totals = [Vec::new(), Vec::new()];
        let mut holds = [Vec::new(), Vec::new()];
        let mut preparations = Vec::new();
        for trial in 0..11 {
            let changes: Vec<_> = (1..=count)
                .map(|id| {
                    let mut value = SyncBookMetadata { title: format!("Prepared {case}-{trial}"), ..Default::default() };
                    value.book.rights.push(format!("{trial}{}", "x".repeat(bytes)));
                    let mutation = StateMutation {
                        mutation_id: MutationId::new(),
                        body: MutationBody::Metadata { content_hash: ContentHash::new(&format!("{id:064x}")), value },
                        changed_at: if stale { 1 } else { base + case * 100 + trial },
                        replica_seq: ReplicaSeq::new(id as u64).unwrap(),
                    };
                    ServerMutation { mutation: mutation.to_wire().unwrap(), replica_id: test_support::fixture_replica_id("prepared-bench"), revision: sync_common::LibraryRevision::new(id as u64).unwrap() }
                })
                .collect();
            for mode in if trial % 2 == 0 { [0, 1] } else { [1, 0] } {
                let conn = &mut conns[mode];
                let start = Instant::now();
                let prepared = if mode == 1 { Some(crate::remote_reducer::prepare_remote_changes(&changes).unwrap()) } else { None };
                if mode == 1 {
                    preparations.push(start.elapsed());
                }
                let tx = library_write_transaction(conn).unwrap();
                let held = Instant::now();
                if let Some(prepared) = &prepared {
                    crate::remote_reducer::apply_prepared_remote_changes_in_tx(&tx, prepared).unwrap();
                } else {
                    crate::remote_reducer::apply_remote_changes_in_tx(&tx, &changes).unwrap();
                }
                set_pull_cursor(&tx, sync_common::SyncCursor { state_revision: Some(changes.last().unwrap().revision), reading_revision: None }).unwrap();
                tx.commit().unwrap();
                holds[mode].push(held.elapsed());
                totals[mode].push(start.elapsed());
                assert!(crate::sync_state::get_pending_mutations(conn).unwrap().is_empty());
            }
            let snapshot = |conn: &Connection| {
                conn.prepare(include_str!("sql/common_sync_benchmarks/benchmark_prepared_remote_writer_select.sql"))
                    .unwrap()
                    .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<Vec<u8>>>(2)?)))
                    .unwrap()
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap()
            };
            assert_eq!(snapshot(&conns[0]), snapshot(&conns[1]));
        }
        eprintln!(
            "prepared count={count} rights_bytes={bytes} stale={stale} before_writer={:?} after_writer={:?} preparation={:?} before_total={:?} after_total={:?}",
            median(holds[0].clone()),
            median(holds[1].clone()),
            median(preparations),
            median(totals[0].clone()),
            median(totals[1].clone())
        );
    }
}

#[test]
#[ignore = "manual release incoming optimization candidates; no production changes"]
fn benchmark_incoming_candidates() {
    fn changes(books: usize, repeats: usize, timestamp: u64) -> Vec<ServerMutation> {
        (0..repeats)
            .flat_map(|repeat| {
                (1..=books).map(move |id| {
                    let mut book = book_model::BookMetadata::default();
                    book.languages.push(book_model::LanguageTag::parse(if repeat % 2 == 0 { "en" } else { "sv" }).unwrap());
                    let mutation = StateMutation {
                        mutation_id: MutationId::new(),
                        body: MutationBody::Metadata { content_hash: ContentHash::new(&format!("{id:064x}")), value: SyncBookMetadata { title: format!("Revision {timestamp}-{repeat}"), book, ..Default::default() } },
                        changed_at: timestamp + repeat as u64,
                        replica_seq: ReplicaSeq::new((repeat * books + id) as u64).unwrap(),
                    };
                    ServerMutation { mutation: mutation.to_wire().unwrap(), replica_id: test_support::fixture_replica_id("candidates"), revision: sync_common::LibraryRevision::new((repeat * books + id) as u64).unwrap() }
                })
            })
            .collect()
    }
    let roots = [tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap()];
    let mut conns: Vec<_> = roots
        .iter()
        .map(|root| {
            let mut conn = Connection::open(root.path().join("library.db")).unwrap();
            fixture(&mut conn, 1000, 10240);
            conn.pragma_update(None, "synchronous", "FULL").unwrap();
            assert_eq!(conn.pragma_query_value(None, "journal_mode", |row| row.get::<_, String>(0)).unwrap(), "wal");
            conn
        })
        .collect();
    let base = unix_millis().unwrap() as u64 + 10000;
    for (case, books, repeats) in [("four_pages", 128, 1), ("four_revisions", 32, 4), ("distinct_books_control", 128, 1)] {
        let mut times = [Vec::new(), Vec::new()];
        let mut held = [Vec::new(), Vec::new()];
        for trial in 0..11 {
            let timestamp =
                base + match case {
                    "four_pages" => 0,
                    "four_revisions" => 1000,
                    _ => 2000,
                } + trial * 10;
            let incoming = changes(books, repeats, timestamp);
            for mode in if trial % 2 == 0 { [0, 1] } else { [1, 0] } {
                let conn = &mut conns[mode];
                let selected = if mode == 1 && repeats > 1 { &incoming[incoming.len() - books..] } else { &incoming[..] };
                let chunk_size = if mode == 0 && case == "four_pages" { 32 } else { selected.len() };
                let start = Instant::now();
                let mut longest = Duration::ZERO;
                for chunk in selected.chunks(chunk_size) {
                    let tx = library_write_transaction(conn).unwrap();
                    let acquired = Instant::now();
                    tx.pragma_update(None, "defer_foreign_keys", true).unwrap();
                    remote_reducer::apply_remote_changes_in_tx(&tx, chunk).unwrap();
                    set_pull_cursor(&tx, sync_common::SyncCursor { state_revision: Some(chunk.last().unwrap().revision), reading_revision: None }).unwrap();
                    tx.commit().unwrap();
                    longest = longest.max(acquired.elapsed());
                }
                times[mode].push(start.elapsed());
                held[mode].push(longest);
                let title: String = conn.query_row(include_str!("sql/common_sync_benchmarks/changes_select.sql"), [format!("{:064x}", 1)], |row| row.get(0)).unwrap();
                assert_eq!(title, format!("Revision {timestamp}-{}", repeats - 1));
                assert!(crate::sync_state::get_pending_mutations(conn).unwrap().is_empty());
            }
            for sql in [include_str!("sql/common_sync_benchmarks/changes_select_2.sql"), include_str!("sql/common_sync_benchmarks/changes_select_3.sql"), include_str!("sql/common_sync_benchmarks/changes_select_4.sql")] {
                let snapshot = |conn: &Connection| conn.prepare(sql).unwrap().query_map([format!("{books:064x}")], |row| row.get::<_, String>(0)).unwrap().collect::<Result<Vec<_>, _>>().unwrap();
                assert_eq!(snapshot(&conns[0]), snapshot(&conns[1]));
            }
            assert_eq!(get_pull_cursor(&conns[0]).unwrap(), get_pull_cursor(&conns[1]).unwrap());
        }
        eprintln!("candidate={case} before={:?} after={:?} longest_writer_before={:?} longest_writer_after={:?}", median(times[0].clone()), median(times[1].clone()), median(held[0].clone()), median(held[1].clone()));
    }
    // Read-only ID/purge-state resolution: this excludes reducer/version logic.
    let lookup_tx = library_write_transaction(&mut conns[0]).unwrap();
    let conn = &lookup_tx;
    for count in [1, 32, 128, 512] {
        let hashes: Vec<_> = (1..=count).map(|id| format!("{id:064x}")).collect();
        let mut times = [Vec::new(), Vec::new()];
        for trial in 0..101 {
            for mode in if trial % 2 == 0 { [0, 1] } else { [1, 0] } {
                let start = Instant::now();
                let mut rows = Vec::<(i64, bool)>::new();
                if mode == 0 {
                    let mut query = conn.prepare_cached(include_str!("sql/common_sync_benchmarks/changes_select_5.sql")).unwrap();
                    for hash in &hashes {
                        rows.push(query.query_row([hash], |row| Ok((row.get(0)?, row.get(1)?))).unwrap());
                    }
                } else {
                    let keys = serde_json::to_string(&hashes).unwrap();
                    rows = conn.prepare_cached(include_str!("sql/common_sync_benchmarks/changes_select_6.sql")).unwrap().query_map([keys], |row| Ok((row.get(0)?, row.get(1)?))).unwrap().collect::<Result<_, _>>().unwrap();
                }
                times[mode].push(start.elapsed());
                assert_eq!(rows.len(), count);
                rows.sort();
                assert_eq!(rows, (1..=count).map(|id| (id as i64, false)).collect::<Vec<_>>());
            }
        }
        eprintln!("candidate=book_lookup count={count} cached_individual={:?} json_batch={:?}", median(times[0].clone()), median(times[1].clone()));
    }
}

#[test]
#[ignore = "manual release cached acknowledgment comparison"]
fn benchmark_ack_statement_cache() {
    let root = tempfile::tempdir().unwrap();
    let mut conn = Connection::open(root.path().join("library.db")).unwrap();
    fixture(&mut conn, 1000, 10240);
    for count in [1, 32, 256] {
        let mut total = [Vec::new(), Vec::new()];
        let mut sql = [Vec::new(), Vec::new()];
        for trial in 0..9 {
            for cached in if trial % 2 == 0 { [0, 1] } else { [1, 0] } {
                conn.execute(include_str!("sql/common_sync_benchmarks/benchmark_ack_statement_cache_update.sql"), rusqlite::params![format!("Ack {count} {trial} {cached}"), count]).unwrap();
                let ids = crate::sync_state::get_pending_mutations(&conn).unwrap().iter().map(|m| m.mutation_id).collect::<Vec<_>>();
                assert_eq!(ids.len(), count as usize);
                let start = Instant::now();
                let tx = library_write_transaction(&mut conn).unwrap();
                let sql_start = Instant::now();
                if cached == 0 {
                    for id in &ids {
                        tx.acknowledge_mutation(&id.to_string()).unwrap();
                    }
                } else {
                    crate::sync_state::acknowledge_mutations(&tx, &ids).unwrap();
                }
                sql[cached].push(sql_start.elapsed());
                tx.commit().unwrap();
                total[cached].push(start.elapsed());
                assert!(crate::sync_state::get_pending_mutations(&conn).unwrap().is_empty());
            }
        }
        eprintln!("ack cache count={count} before_sql={:?} after_sql={:?} before_commit_total={:?} after_commit_total={:?}", median(sql[0].clone()), median(sql[1].clone()), median(total[0].clone()), median(total[1].clone()));
    }
}

#[test]
#[ignore = "manual release common sync paths; temporary disk WAL fixtures"]
fn benchmark_common_sync_paths() {
    let root = tempfile::tempdir().unwrap();
    let mut conn = Connection::open(root.path().join("library.db")).unwrap();
    fixture(&mut conn, 1000, 10240);
    let mut idle = Vec::new();
    for _ in 0..7 {
        let start = Instant::now();
        for _ in 0..1000 {
            std::hint::black_box(get_library_sync_details(&conn).unwrap());
            std::hint::black_box(get_pull_cursor(&conn).unwrap());
            assert!(crate::sync_state::get_pending_mutations(&conn).unwrap().is_empty());
        }
        idle.push(start.elapsed() / 1000);
    }
    eprintln!("common idle status+cursor+empty_outbox median={:?}", median(idle));
    for (kind, count) in [("reading_position", 1), ("metadata", 1), ("metadata", 32), ("description", 32)] {
        let mut writes = Vec::new();
        let mut encodes = Vec::new();
        let mut acks = Vec::new();
        let mut body_bytes = 0;
        for trial in 0..7 {
            let start = Instant::now();
            let tx = library_write_transaction(&mut conn).unwrap();
            match kind {
                "reading_position" => {
                    tx.execute(include_str!("sql/common_sync_benchmarks/benchmark_common_sync_paths_update.sql"), rusqlite::params![format!("epubcfi(/6/{})", 2 * (trial + 1)), trial as f64 / 10.0]).unwrap();
                }
                "metadata" => {
                    tx.execute(include_str!("sql/common_sync_benchmarks/benchmark_ack_statement_cache_update.sql"), rusqlite::params![format!("Title {count} {trial}"), count]).unwrap();
                }
                _ => {
                    tx.execute(include_str!("sql/common_sync_benchmarks/benchmark_common_sync_paths_update_3.sql"), rusqlite::params![format!("{trial}{}", "x".repeat(10239)), count]).unwrap();
                }
            }
            tx.commit().unwrap();
            writes.push(start.elapsed());
            let start = Instant::now();
            let changes = crate::sync_state::get_pending_mutations(&conn).unwrap();
            assert_eq!(changes.len(), count as usize);
            let ids: Vec<_> = changes.iter().map(|c| c.mutation_id).collect();
            let mut batch = library_replica::prepare_push(library_replica::coalesce_mutations_for_push(changes)).unwrap();
            let mut sent = 0;
            while !batch.is_empty() {
                sent += batch.next_batch().len();
            }
            assert_eq!(sent, count as usize);
            encodes.push(start.elapsed());
            body_bytes = conn.query_row(include_str!("sql/common_sync_benchmarks/benchmark_common_sync_paths_select.sql"), [], |r| r.get::<_, i64>(0)).unwrap();
            let start = Instant::now();
            let tx = library_write_transaction(&mut conn).unwrap();
            crate::sync_state::acknowledge_mutations(&tx, &ids).unwrap();
            tx.commit().unwrap();
            acks.push(start.elapsed());
        }
        eprintln!("common local kind={kind} count={count} write_commit={:?} read_encode={:?} ack_commit={:?} body_bytes={body_bytes}", median(writes), median(encodes), median(acks));
    }
}

#[test]
#[ignore = "manual release common incoming sync paths"]
fn benchmark_common_incoming_sync() {
    let roots = [tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap()];
    let mut conns = Vec::new();
    for root in &roots {
        let mut conn = Connection::open(root.path().join("library.db")).unwrap();
        fixture(&mut conn, 1000, 10240);
        conns.push(conn);
    }
    let base = unix_millis().unwrap() as u64 + 10000;
    for (case, (kind, count)) in [("reading_position", 1), ("metadata", 32), ("description", 32), ("stale_metadata", 32)].into_iter().enumerate() {
        let mut times = [Vec::new(), Vec::new()];
        for trial in 0..7 {
            let mut changes = Vec::new();
            for id in 1..=count {
                let hash = ContentHash::new(&format!("{id:064x}"));
                let body = match kind {
                    "reading_position" => MutationBody::ReadingPosition {
                        content_hash: hash,
                        value: ReadingPositionState { location: book_model::ReadingPosition::parse(&format!("epubcfi(/6/{})", 2 * (trial + 1))).unwrap(), progress: trial as f32 / 10.0 },
                    },
                    "description" => MutationBody::Description { content_hash: hash, value: format!("{trial}{}", "x".repeat(10239)) },
                    _ => MutationBody::Metadata { content_hash: hash, value: SyncBookMetadata { title: format!("Remote {trial}"), ..Default::default() } },
                };
                let mutation = StateMutation {
                    mutation_id: MutationId::new(),
                    body,
                    changed_at: if kind == "stale_metadata" { 1 } else { base + (case * 100 + trial) as u64 },
                    replica_seq: ReplicaSeq::new((case * 1000 + trial * 32 + id) as u64).unwrap(),
                };
                changes.push(ServerMutation {
                    mutation: mutation.to_wire().unwrap(),
                    replica_id: test_support::fixture_replica_id("benchmark-remote"),
                    revision: sync_common::LibraryRevision::new((case * 1000 + trial * 32 + id) as u64).unwrap(),
                });
            }
            for mode in if trial % 2 == 0 { [0, 1] } else { [1, 0] } {
                let conn = &mut conns[mode];
                let start = Instant::now();
                let tx = library_write_transaction(conn).unwrap();
                tx.pragma_update(None, "defer_foreign_keys", "ON").unwrap();
                if mode == 0 {
                    for change in &changes {
                        remote_reducer::apply_remote_change_in_tx(&tx, change).unwrap();
                    }
                } else {
                    remote_reducer::apply_remote_changes_in_tx(&tx, &changes).unwrap();
                }
                set_pull_cursor(&tx, sync_common::SyncCursor { state_revision: Some(changes.last().unwrap().revision), reading_revision: None }).unwrap();
                tx.commit().unwrap();
                times[mode].push(start.elapsed());
                assert!(crate::sync_state::get_pending_mutations(conn).unwrap().is_empty());
            }
        }
        eprintln!("common incoming kind={kind} count={count} per_mutation_guard={:?} per_page_guard={:?}", median(times[0].clone()), median(times[1].clone()));
    }
}
