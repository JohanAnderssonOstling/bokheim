//! Explicit, file-backed reducer diagnostics. Instrumentation is test-only.
use super::*;
use rusqlite::{
    Connection,
    trace::{TraceEvent, TraceEventCodes},
};
use std::{cell::RefCell, collections::BTreeMap, time::Instant};

#[derive(Default, serde::Serialize)]
struct Metrics {
    phases_ms: BTreeMap<&'static str, f64>,
    phase_calls: BTreeMap<&'static str, usize>,
    sql_calls: BTreeMap<String, usize>,
    trigger_calls: BTreeMap<String, usize>,
    sql_step_ms: BTreeMap<String, f64>,
}
thread_local! { static ACTIVE: RefCell<Option<Metrics>> = const { RefCell::new(None) }; }

pub(super) struct Span(Option<(&'static str, Instant)>);
impl Span {
    pub(super) fn new(name: &'static str) -> Self {
        Self(ACTIVE.with(|value| value.borrow().is_some().then(|| (name, Instant::now()))))
    }
}
impl Drop for Span {
    fn drop(&mut self) {
        if let Some((name, start)) = self.0 {
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            ACTIVE.with(|value| {
                if let Some(metrics) = value.borrow_mut().as_mut() {
                    *metrics.phases_ms.entry(name).or_default() += elapsed;
                    *metrics.phase_calls.entry(name).or_default() += 1;
                }
            });
        }
    }
}
fn trace(event: TraceEvent<'_>) {
    ACTIVE.with(|value| {
        let mut value = value.borrow_mut();
        let Some(metrics) = value.as_mut() else { return };
        match event {
            TraceEvent::Stmt(statement, text) => {
                if text.starts_with("-- TRIGGER ") {
                    *metrics.trigger_calls.entry(text.to_owned()).or_default() += 1;
                } else if !text.starts_with("--") {
                    *metrics.sql_calls.entry(statement.sql().into_owned()).or_default() += 1;
                }
            }
            TraceEvent::Profile(statement, duration) => {
                *metrics.sql_step_ms.entry(statement.sql().into_owned()).or_default() += duration.as_secs_f64() * 1000.0;
            }
            _ => {}
        }
    });
}

fn fixture() -> (tempfile::TempDir, Connection) {
    let root = tempfile::tempdir().unwrap();
    let mut conn = Connection::open(root.path().join("library.db")).unwrap();
    crate::schema::configure_connection(&conn).unwrap();
    crate::schema::init_db(&conn).unwrap();
    let tx = crate::schema::library_write_transaction(&mut conn).unwrap();
    crate::sync_state::set_sync_metadata(&tx, "change_origin", "remote").unwrap();
    for index in 0..1000 {
        let hash = format!("{index:064x}");
        tx.execute(include_str!("sql/profiling/book.sql"), [&hash]).unwrap();
        tx.execute(include_str!("sql/profiling/placement.sql"), rusqlite::params![hash, format!("{index}.epub")]).unwrap();
    }
    tx.execute_batch(include_str!("sql/profiling/settle.sql")).unwrap();
    tx.commit().unwrap();
    (root, conn)
}

#[test]
#[ignore = "explicit release per-record reducer and trigger profile"]
fn benchmark_reducer_record_costs() {
    assert!(!cfg!(debug_assertions), "use cargo test --release");
    let changes = (1..=1000)
        .map(|revision| ServerMutation {
            mutation: library_replica::StateMutation {
                mutation_id: sync_common::MutationId::new(),
                replica_seq: sync_common::ReplicaSeq::new(revision).unwrap(),
                changed_at: 10,
                body: MutationBody::BookLifecycle {
                    content_hash: test_support::fixture_content_hash(100_000 + revision),
                    value: library_model::BookLifecycleState::Present {
                        metadata: library_model::SyncBookMetadata { title: format!("Remote {revision}"), contributors: Vec::new(), publication: Default::default() },
                        added_at: 10,
                        format: BookFormat::Epub,
                    },
                },
            }
            .to_wire()
            .unwrap(),
            replica_id: test_support::fixture_replica_id("measurement-server"),
            revision: sync_common::LibraryRevision::new(revision).unwrap(),
        })
        .collect::<Vec<_>>();
    for (triggers, tracing, samples) in [(true, false, 3), (false, false, 3), (true, true, 1)] {
        for sample in 0..samples {
            let (_root, mut conn) = fixture();
            // Counterfactual only: derived state is intentionally incomplete.
            if !triggers {
                conn.set_db_config(rusqlite::config::DbConfig::SQLITE_DBCONFIG_ENABLE_TRIGGER, false).unwrap();
            }
            if tracing {
                conn.trace_v2(TraceEventCodes::SQLITE_TRACE_STMT | TraceEventCodes::SQLITE_TRACE_PROFILE, Some(trace));
            }
            ACTIVE.with(|value| *value.borrow_mut() = Some(Metrics::default()));
            let start = Instant::now();
            for page in changes.chunks(100) {
                let prepared = prepare_remote_changes(page).unwrap();
                let tx = crate::schema::library_write_transaction(&mut conn).unwrap();
                let response = sync_common::PullStateResponse {
                    mutations: page.to_vec(),
                    next_cursor: sync_common::SyncCursor { state_revision: page.last().map(|change| change.revision), reading_revision: None },
                    has_more: page.last().unwrap().revision.get() < 1000,
                };
                crate::sync_state::apply_prepared_pull_page_in_tx(&tx, &response, &prepared).unwrap();
                tx.commit().unwrap();
            }
            let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
            let metrics = ACTIVE.with(|value| value.borrow_mut().take().unwrap());
            assert_eq!(metrics.phase_calls["version_check"], 1000);
            assert_eq!(metrics.phase_calls["metadata_batch_write"], 10);

            assert_eq!(crate::sync_state::get_pull_cursor(&conn).unwrap().state_revision.unwrap().get(), 1000);
            assert!(crate::assets::queued_book_asset_state(&conn, &test_support::fixture_content_hash(101_000)).unwrap().is_some());
            eprintln!("REDUCER_PROFILE {}", serde_json::json!({"triggers":triggers,"tracing":tracing,"sample":sample,"elapsed_ms":elapsed_ms,"metrics":metrics}));
        }
    }
}

#[test]
#[ignore = "explicit release statement-preparation diagnostic"]
fn benchmark_remote_book_statement_preparation() {
    assert!(!cfg!(debug_assertions), "use cargo test --release");
    // Read the real owning feature's SQL rather than maintaining a benchmark copy.
    let sql = include_str!("../books/sql/apply_remote_book_added.sql");
    for triggers in [true, false] {
        for cached in [false, true] {
            for sample in 0..3 {
                let (_root, conn) = fixture();
                if !triggers {
                    conn.set_db_config(rusqlite::config::DbConfig::SQLITE_DBCONFIG_ENABLE_TRIGGER, false).unwrap();
                }
                let started = Instant::now();
                for _ in 0..1000 {
                    if cached {
                        std::hint::black_box(conn.prepare_cached(&sql).unwrap());
                    } else {
                        std::hint::black_box(conn.prepare(&sql).unwrap());
                    }
                }
                eprintln!("PREPARE_PROFILE {}", serde_json::json!({"triggers":triggers,"cached":cached,"sample":sample,"iterations":1000,"elapsed_ms":started.elapsed().as_secs_f64()*1000.0}));
            }
        }
    }
}

#[test]
#[ignore = "explicit release metadata-edit profile"]
fn benchmark_metadata_edits() {
    assert!(!cfg!(debug_assertions));
    let changes = (1..=20)
        .map(|revision| ServerMutation {
            mutation: library_replica::StateMutation {
                mutation_id: sync_common::MutationId::new(),
                replica_seq: sync_common::ReplicaSeq::new(revision).unwrap(),
                changed_at: 10,
                body: MutationBody::Metadata {
                    content_hash: ContentHash::new(&format!("{:064x}", revision - 1)),
                    value: library_model::SyncBookMetadata {
                        title: format!("Edited {revision}"),
                        contributors: vec![library_model::Contributor::new("Example Author", None, library_model::AUTHOR_MARC_RELATOR_CODE).unwrap()],
                        publication: Default::default(),
                    },
                },
            }
            .to_wire()
            .unwrap(),
            replica_id: test_support::fixture_replica_id("measurement-server"),
            revision: sync_common::LibraryRevision::new(revision).unwrap(),
        })
        .collect::<Vec<_>>();
    for tracing in [false, true] {
        for sample in 0..if tracing { 1 } else { 5 } {
            let (_root, mut conn) = fixture();
            if tracing {
                conn.trace_v2(TraceEventCodes::SQLITE_TRACE_STMT | TraceEventCodes::SQLITE_TRACE_PROFILE, Some(trace));
            }
            ACTIVE.with(|v| *v.borrow_mut() = Some(Metrics::default()));
            let start = Instant::now();
            let prepared = prepare_remote_changes(&changes).unwrap();
            let tx = crate::schema::library_write_transaction(&mut conn).unwrap();
            apply_prepared_remote_changes_in_tx(&tx, &prepared).unwrap();
            tx.commit().unwrap();
            let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
            let metrics = ACTIVE.with(|v| v.borrow_mut().take().unwrap());
            eprintln!("EDIT_PROFILE {}", serde_json::json!({"tracing":tracing,"sample":sample,"elapsed_ms":elapsed_ms,"metrics":metrics}));
        }
    }
}
