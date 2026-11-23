#![cfg(not(target_arch = "wasm32"))]

use rusqlite::Connection;
use subject_projection::{installed, loaded_unified_taxonomy_revision_id};

#[test]
fn downloading_workers_require_prepared_runtime_snapshots() {
    let image = std::fs::read(subject_projection::bundled::path().unwrap()).unwrap();
    let mut db = Connection::open_in_memory().unwrap();
    db.deserialize_read_exact("main", image.as_slice(), image.len(), false).unwrap();
    // A missing compiled index must be rejected rather than rebuilt by the client.
    let tables = db.prepare("SELECT name FROM sqlite_schema WHERE type='table' AND name GLOB 'taxonomy_match_*'").unwrap().query_map([], |row| row.get::<_, String>(0)).unwrap().collect::<Result<Vec<_>, _>>().unwrap();
    for table in tables {
        db.execute_batch(&format!("DROP TABLE {table}")).unwrap();
    }
    db.execute_batch("DROP VIEW unified_concept_paths; DROP TABLE unified_concept_route; DELETE FROM taxonomy_meta WHERE key IN ('client_revision','client_matcher_version')").unwrap();
    let snapshot = db.serialize("main").unwrap().to_vec();

    assert!(installed::install_snapshot(&snapshot).unwrap_err().to_string().contains("must be prepared"));
    assert!(loaded_unified_taxonomy_revision_id().is_none());
}
