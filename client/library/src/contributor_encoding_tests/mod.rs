//! Synthetic release comparison of contributor identity lookup strategies.
use crate::metadata;
use crate::schema::init_db;
use crate::schema::library_write_transaction;
use crate::sync_state::SyncSql;
use crate::sync_state::*;
use book_model::AgentId;
use book_model::BookFormat;
use book_model::BookRecord;
use book_model::Contributor;
use rusqlite::functions::Context as FunctionContext;
use rusqlite::functions::FunctionFlags;
use rusqlite::Connection;
use std::time::Instant;

fn joined_encoder(context: &FunctionContext<'_>) -> rusqlite::Result<Vec<u8>> {
    let book_row_id = context.get::<i64>(0)?;
    // SAFETY: SQLite owns this context and its connection for the duration of
    // the executing statement, including nested trigger invocations.
    let connection = unsafe { context.get_connection()? };
    // LEFT JOIN keeps corrupt/missing identities visible as errors instead of
    // silently omitting the credit from the serialized mutation.
    let mut statement = connection.prepare_cached(include_str!("sql/contributor_encoding_tests/joined_encoder_select.sql"))?;
    let mut rows = statement.query([book_row_id])?;
    let mut contributors = Vec::new();
    while let Some(row) = rows.next()? {
        let stable_id: String = row.get(0)?;
        let name: String = row.get(1)?;
        let sort_name: Option<String> = row.get(2)?;
        let role_bytes: Vec<u8> = row.get(3)?;
        let role_array: [u8; 3] = role_bytes.as_slice().try_into().map_err(|_| function_error("stored contributor role must be exactly 3 bytes"))?;
        let role = book_model::MarcRelatorCode(role_array);
        let contributor_id = AgentId::parse_str(&stable_id).map_err(|error| function_error(error.to_string()))?;
        contributors.push(Contributor::with_id(contributor_id, name, sort_name, role).map_err(|error| function_error(error.to_string()))?);
    }
    sync_common::wire::encode(&contributors).map_err(|error| function_error(error.to_string()))
}

fn legacy_encoder(context: &FunctionContext<'_>) -> rusqlite::Result<Vec<u8>> {
    let book_row_id = context.get::<i64>(0)?;
    // SAFETY: `contributors_for_book` is only invoked from within triggers
    // firing on the same connection that owns this function registration;
    // sqlite guarantees the context's db handle is that connection while the
    // triggering statement is executing.
    let connection = unsafe { context.get_connection()? };
    let mut statement = connection.prepare_cached(include_str!("sql/contributor_encoding_tests/legacy_encoder_select.sql"))?;
    let mut rows = statement.query([book_row_id])?;
    let mut contributors = Vec::new();
    while let Some(row) = rows.next()? {
        let author_identity_id: i64 = row.get(0)?;
        let name: String = row.get(1)?;
        let sort_name: Option<String> = row.get(2)?;
        let role_bytes: Vec<u8> = row.get(3)?;
        let role_array: [u8; 3] = role_bytes.as_slice().try_into().map_err(|_| function_error("stored contributor role must be exactly 3 bytes"))?;
        let role = book_model::MarcRelatorCode(role_array);
        let stable_id: String = connection.query_row(include_str!("sql/contributor_encoding_tests/legacy_encoder_select_2.sql"), [author_identity_id], |identity_row| identity_row.get(0))?;
        let contributor_id = AgentId::parse_str(&stable_id).map_err(|error| function_error(error.to_string()))?;
        contributors.push(Contributor::with_id(contributor_id, name, sort_name, role).map_err(|error| function_error(error.to_string()))?);
    }
    sync_common::wire::encode(&contributors).map_err(|error| function_error(error.to_string()))
}

fn state(count: usize) -> BookRecord {
    BookRecord {
        title: "Initial".into(),
        contributors: (0..count).map(|i| Contributor::new(&format!("Contributor {i}"), Some(format!("Sort {i}")), book_model::MarcRelatorCode(if i % 3 == 2 { *b"trl" } else { *b"aut" })).unwrap()).collect(),
        description: "Fixture description".into(),
        book: Default::default(),
    }
}
fn seed(conn: &mut Connection, metadata: &BookRecord) {
    init_db(conn).unwrap();
    let tx = library_write_transaction(conn).unwrap();
    for id in 1..=32 {
        metadata::insert_scanned_book_metadata(&tx, test_support::fixture_content_hash(id), metadata, BookFormat::Epub).unwrap();
    }
    tx.execute(include_str!("sql/contributor_encoding_tests/seed_delete.sql"), []).unwrap();
    tx.commit().unwrap();
}

#[test]
fn joined_encoder_matches_previous_bytes_and_rejects_missing_identity() {
    let mut conn = Connection::open_in_memory().unwrap();
    let mut metadata = state(4);
    metadata.contributors.push(metadata.contributors[0].clone());
    seed(&mut conn, &metadata);
    conn.create_scalar_function("contributors_for_book", 1, FunctionFlags::SQLITE_UTF8, joined_encoder).unwrap();
    conn.create_scalar_function("legacy_contributors", 1, FunctionFlags::SQLITE_UTF8, legacy_encoder).unwrap();
    let pairs = conn
        .prepare(include_str!("sql/contributor_encoding_tests/joined_encoder_matches_previous_bytes_and_rejects_missing_identity_select.sql"))
        .unwrap()
        .query_map([], |r| Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, Vec<u8>>(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    for (current, old) in pairs {
        assert_eq!(current, old);
        let credits: Vec<Contributor> = sync_common::wire::decode(&current, usize::MAX).unwrap();
        assert_eq!(credits, metadata.contributors);
    }
    conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
    set_sync_metadata(&conn, "change_origin", "remote").unwrap();
    conn.execute(include_str!("sql/contributor_encoding_tests/joined_encoder_matches_previous_bytes_and_rejects_missing_identity_update.sql"), []).unwrap();
    assert!(conn.query_row(include_str!("sql/contributor_encoding_tests/joined_encoder_matches_previous_bytes_and_rejects_missing_identity_select_2.sql"), [], |r| r.get::<_, Vec<u8>>(0)).is_err());
    assert!(conn.query_row(include_str!("sql/contributor_encoding_tests/joined_encoder_matches_previous_bytes_and_rejects_missing_identity_select_3.sql"), [], |r| r.get::<_, Vec<u8>>(0)).is_err());
}

#[test]
#[ignore = "manual release outgoing metadata contributor join benchmark"]
fn benchmark_outgoing_contributor_join() {
    for count in [0, 1, 4, 8] {
        let roots = [tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap()];
        let metadata = state(count);
        let mut conns = Vec::new();
        for (mode, root) in roots.iter().enumerate() {
            let mut conn = Connection::open(root.path().join("library.db")).unwrap();
            seed(&mut conn, &metadata);
            if mode == 0 {
                conn.create_scalar_function("contributors_for_book", 1, FunctionFlags::SQLITE_UTF8, legacy_encoder).unwrap();
            } else {
                conn.create_scalar_function("contributors_for_book", 1, FunctionFlags::SQLITE_UTF8, joined_encoder).unwrap();
            }
            conns.push(conn);
        }
        let mut encode = [Vec::new(), Vec::new()];
        let mut commits = [Vec::new(), Vec::new()];
        for trial in 0..15 {
            let mut payloads = [Vec::new(), Vec::new()];
            for mode in if trial % 2 == 0 { [0, 1] } else { [1, 0] } {
                let conn = &mut conns[mode];
                let start = Instant::now();
                payloads[mode] =
                    conn.prepare(include_str!("sql/contributor_encoding_tests/benchmark_outgoing_contributor_join_select.sql")).unwrap().query_map([], |r| r.get::<_, Vec<u8>>(0)).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap();
                encode[mode].push(start.elapsed());
                let start = Instant::now();
                let tx = library_write_transaction(conn).unwrap();
                tx.execute(include_str!("sql/contributor_encoding_tests/benchmark_outgoing_contributor_join_update.sql"), [format!("Changed {trial}")]).unwrap();
                tx.commit().unwrap();
                commits[mode].push(start.elapsed());
                assert_eq!(conn.query_row(include_str!("sql/contributor_encoding_tests/benchmark_outgoing_contributor_join_select_2.sql"), [], |r| r.get::<_, i64>(0)).unwrap(), 32);
            }
            assert_eq!(payloads[0], payloads[1]);
        }
        for values in &mut encode {
            values.sort();
        }
        for values in &mut commits {
            values.sort();
        }
        eprintln!("outgoing books=32 contributors={count} before_encode={:?} after_encode={:?} before_edit_commit={:?} after_edit_commit={:?} trials=15", encode[0][7], encode[1][7], commits[0][7], commits[1][7]);
    }
}
