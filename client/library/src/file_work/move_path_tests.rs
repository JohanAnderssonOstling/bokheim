use super::*;

fn fixture(indexed: bool) -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    crate::schema::init_db(&conn).unwrap();
    if !indexed {
        conn.execute_batch(include_str!("sql/move_path_tests/fixture_drop.sql")).unwrap();
    }
    conn
}

fn legacy(tx: &Transaction<'_>, old: &str, new: &str) -> usize {
    tx.execute(include_str!("sql/move_path_tests/legacy_update.sql"), rusqlite::params![format!("/{}/", old.trim_matches('/')), format!("/{}/", new.trim_matches('/'))]).unwrap()
}

#[test]
fn indexed_path_moves_match_legacy_and_roll_back() {
    let paths = ["/Café_%/a.epub", "/Café_%/子/b.epub", "/Café_%Extra/c.epub", "/café_%/d.epub", "/Café_%0/e.epub", "/Café_%", "/Other/x.epub", "/Café_%/Z/another.epub"];
    let mut before = fixture(false);
    let mut after = fixture(true);
    for conn in [&before, &after] {
        for (i, path) in paths.iter().enumerate() {
            conn.execute(include_str!("sql/move_path_tests/benchmark_move_projection_paths_insert.sql"), rusqlite::params![i.to_string(), path]).unwrap();
        }
    }
    fn snapshot(conn: &Connection) -> Vec<String> {
        conn.prepare(include_str!("sql/move_path_tests/snapshot_select.sql")).unwrap().query_map([], |row| row.get(0)).unwrap().collect::<Result<_, _>>().unwrap()
    }
    let original = snapshot(&after);
    for (old, new) in [("/Café_%/", "Moved/東京"), ("/Café_%/", "Café_%/Z"), ("Missing", "Other")] {
        let a = before.transaction().unwrap();
        let b = after.transaction().unwrap();
        assert_eq!(legacy(&a, old, new), move_file_projection_paths(&b, old, new).unwrap());
        assert_eq!(snapshot(&a), snapshot(&b));
        a.rollback().unwrap();
        b.rollback().unwrap();
        assert_eq!(snapshot(&after), original);
    }
    let plan: String = after.query_row(include_str!("sql/move_path_tests/snapshot_explain.sql"), [], |row| row.get(3)).unwrap();
    assert!(plan.contains("SEARCH") && plan.contains("idx_local_file_projection_path"), "{plan}");
}

#[test]
#[ignore = "manual release path update benchmark, excludes disk commit and filesystem rename"]
fn benchmark_move_projection_paths() {
    use std::time::Instant;
    for (total, moved) in [(1000, 10), (100000, 10), (100000, 1000), (10000, 10000)] {
        let mut before = fixture(false);
        let mut after = fixture(true);
        for conn in [&mut before, &mut after] {
            let tx = conn.transaction().unwrap();
            {
                let mut insert = tx.prepare(include_str!("sql/move_path_tests/benchmark_move_projection_paths_insert.sql")).unwrap();
                for i in 0..total {
                    let parent = if i < moved { "Moved" } else { "Other" };
                    insert.execute(rusqlite::params![i.to_string(), format!("/{parent}/{i}.epub")]).unwrap();
                }
            }
            tx.commit().unwrap();
        }
        let mut samples = [Vec::new(), Vec::new()];
        for trial in 0..21 {
            for optimized in if trial % 2 == 0 { [false, true] } else { [true, false] } {
                let conn = if optimized { &mut after } else { &mut before };
                let tx = conn.transaction().unwrap();
                let start = Instant::now();
                let count = if optimized { move_file_projection_paths(&tx, "Moved", "Destination").unwrap() } else { legacy(&tx, "Moved", "Destination") };
                samples[usize::from(optimized)].push(start.elapsed().as_secs_f64() * 1000.0);
                assert_eq!(count, moved);
                tx.rollback().unwrap();
            }
        }
        for sample in &mut samples {
            sample.sort_by(f64::total_cmp);
        }
        eprintln!("projection_move total={total} moved={moved} before_ms={:.3} after_ms={:.3}", samples[0][10], samples[1][10]);
    }
}
