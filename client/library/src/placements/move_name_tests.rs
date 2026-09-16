use super::*;

fn fixture() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    crate::schema::configure_connection(&conn).unwrap();
    // Isolate destination naming from projection, sync and filesystem work.
    conn.execute_batch(include_str!("sql/move_name_tests/fixture_create.sql")).unwrap();
    conn
}

fn legacy(conn: &Connection, folder: bool, requested: &str) -> String {
    let sql = if folder { include_str!("sql/move_name_tests/legacy_select.sql") } else { include_str!("sql/move_name_tests/legacy_select_2.sql") };
    let names = conn.prepare(sql).unwrap().query_map([], |row| row.get::<_, String>(0)).unwrap().collect::<Result<Vec<_>, _>>().unwrap();
    if folder { library_model::unique_folder_name(requested, names.iter().map(String::as_str)) } else { library_model::unique_file_name(requested, names.iter().map(String::as_str)) }
}

fn candidate(conn: &Connection, folder: bool, requested: &str) -> String {
    if folder { choose_move_folder_name(conn, "dest", "self", requested).unwrap() } else { choose_move_file_name(conn, "dest", requested).unwrap() }
}

#[test]
fn move_names_preserve_portable_collisions_and_deleted_entries() {
    let conn = fixture();
    for (i, name) in ["CAFÉ.epub", "Cafe\u{301} 2.epub", "Ｃａｆé 4.epub", "Other", "Other 2"].iter().enumerate() {
        conn.execute(include_str!("sql/move_name_tests/benchmark_move_destination_names_insert.sql"), rusqlite::params![i.to_string(), name]).unwrap();
        conn.execute(include_str!("sql/move_name_tests/benchmark_move_destination_names_insert_2.sql"), rusqlite::params![i as i64, name]).unwrap();
    }
    conn.execute_batch(include_str!("sql/move_name_tests/move_names_preserve_portable_collisions_and_deleted_entries_insert_3.sql")).unwrap();
    for folder in [false, true] {
        for requested in ["café.epub", "Other", "Own", "Available", "Fresh", "ＣＡＦÉ.epub"] {
            assert_eq!(candidate(&conn, folder, requested), legacy(&conn, folder, requested));
        }
    }
    assert_eq!(candidate(&conn, false, "café.epub"), "café 3.epub");
    for number in 1..=12 {
        let name = if number == 1 { "Repeated".to_owned() } else { format!("Repeated {number}") };
        conn.execute(include_str!("sql/move_name_tests/benchmark_move_destination_names_insert.sql"), rusqlite::params![format!("repeat-{number}"), name]).unwrap();
        conn.execute(include_str!("sql/move_name_tests/move_names_preserve_portable_collisions_and_deleted_entries_insert_5.sql"), rusqlite::params![200 + number, name]).unwrap();
    }
    for folder in [false, true] {
        assert_eq!(candidate(&conn, folder, "Repeated"), "Repeated 13");
    }
}

#[test]
fn move_name_queries_use_normalized_indexes() {
    let conn = fixture();
    for (sql, index) in [
        (include_str!("sql/move_name_tests/move_name_queries_use_normalized_indexes_explain.sql"), "idx_book_dir_live_portable_name"),
        (include_str!("sql/move_name_tests/move_name_queries_use_normalized_indexes_explain_2.sql"), "idx_dir_live_path"),
    ] {
        let plans = conn.prepare(sql).unwrap().query_map([], |row| row.get::<_, String>(3)).unwrap().collect::<Result<Vec<_>, _>>().unwrap();
        assert!(plans.iter().any(|plan| plan.contains(index)), "{plans:?}");
    }
}

#[test]
fn moves_preserve_existing_placements_and_collision_names() {
    let mut conn = Connection::open_in_memory().unwrap();
    crate::schema::init_db(&conn).unwrap();
    let tx = conn.transaction().unwrap();
    let source = DirId::new_v4();
    let destination = DirId::new_v4();
    let moving = DirId::new_v4();
    let occupied = DirId::new_v4();
    for (id, parent, name) in [(source, sync_common::ROOT_DIR_ID, "Source"), (destination, sync_common::ROOT_DIR_ID, "Destination"), (moving, source, "Café"), (occupied, destination, "CAFÉ")] {
        upsert_dir(&tx, &id, &parent, &name.to_owned()).unwrap();
    }
    assert_eq!(move_directory_record(&tx, &moving, Some(&destination), None).unwrap(), "Café 2");
    assert_eq!(move_directory_record(&tx, &moving, None, None).unwrap(), "Café 2");
    let hash = test_support::fixture_content_hash(1);
    let other = test_support::fixture_content_hash(2);
    for hash in [&hash, &other] {
        insert_book(&tx, hash.clone(), BookFormat::Epub).unwrap();
    }
    insert_local_book_dir(&tx, &source, hash.clone(), &"Café.epub".to_owned(), 1, 1).unwrap();
    insert_local_book_dir(&tx, &destination, other, &"CAFÉ.epub".to_owned(), 1, 1).unwrap();
    assert_eq!(transfer_book_placement_record(&tx, &hash, &source, &destination, false).unwrap(), "Café 2.epub");
    assert_eq!(transfer_book_placement_record(&tx, &hash, &source, &destination, true).unwrap(), "Café 2.epub");
    assert!(transfer_book_placement_record(&tx, &hash, &source, &destination, true).is_err());
    assert!(transfer_book_placement_record(&tx, &hash, &destination, &destination, true).is_err());
    tx.commit().unwrap();
}

#[test]
#[ignore = "release destination-name microbenchmark; excludes commit and filesystem work"]
fn benchmark_move_destination_names() {
    use std::time::Instant;
    for size in [10, 1000, 10000] {
        let mut conn = fixture();
        let tx = conn.transaction().unwrap();
        for i in 0..size {
            let name = format!("Book {i}.epub");
            tx.execute(include_str!("sql/move_name_tests/benchmark_move_destination_names_insert.sql"), rusqlite::params![i.to_string(), name]).unwrap();
            tx.execute(include_str!("sql/move_name_tests/benchmark_move_destination_names_insert_2.sql"), rusqlite::params![i, name]).unwrap();
        }
        tx.commit().unwrap();
        for folder in [false, true] {
            for requested in ["Free.epub", "Book 0.epub"] {
                let mut before = Vec::new();
                let mut after = Vec::new();
                for trial in 0..31 {
                    for optimized in if trial % 2 == 0 { [false, true] } else { [true, false] } {
                        let start = Instant::now();
                        let result = if optimized { candidate(&conn, folder, requested) } else { legacy(&conn, folder, requested) };
                        std::hint::black_box(result);
                        let elapsed = start.elapsed().as_secs_f64() * 1e6;
                        if optimized {
                            after.push(elapsed);
                        } else {
                            before.push(elapsed);
                        }
                    }
                }
                before.sort_by(f64::total_cmp);
                after.sort_by(f64::total_cmp);
                eprintln!("move_name folder={folder} siblings={size} collision={} before_us={:.2} after_us={:.2}", requested == "Book 0.epub", before[15], after[15]);
            }
        }
    }
}
