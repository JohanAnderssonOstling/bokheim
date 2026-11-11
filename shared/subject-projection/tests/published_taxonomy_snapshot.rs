#![cfg(not(target_arch = "wasm32"))]

mod support;

use rusqlite::Connection;
use subject_projection::{installed, loaded_unified_taxonomy_revision_id, UnifiedTaxonomy};

#[test]
fn downloading_workers_prepare_published_source_snapshots_before_matching() {
    let (image, definitions) = support::selected_snapshot();
    let expected = UnifiedTaxonomy::from_concepts(definitions.clone()).unwrap();
    let mut db = Connection::open_in_memory().unwrap();
    db.deserialize_read_exact("main", image.as_slice(), image.len(), false).unwrap();
    // Published sources carry definitions, without client-prepared lookups.
    let tables = db.prepare("SELECT name FROM sqlite_schema WHERE type='table' AND name GLOB 'taxonomy_match_*'").unwrap().query_map([], |row| row.get::<_, String>(0)).unwrap().collect::<Result<Vec<_>, _>>().unwrap();
    for table in tables {
        db.execute_batch(&format!("DROP TABLE {table}")).unwrap();
    }
    db.execute_batch("DROP VIEW unified_concept_paths; DROP TABLE unified_concept_route; DELETE FROM taxonomy_meta WHERE key IN ('client_revision','client_matcher_version')").unwrap();
    let snapshot = db.serialize("main").unwrap().to_vec();

    assert!(installed::install_snapshot(&snapshot).unwrap_err().to_string().contains("must be prepared"));
    assert!(loaded_unified_taxonomy_revision_id().is_none());
    installed::install_published_snapshot(&snapshot).unwrap();
    assert_eq!(loaded_unified_taxonomy_revision_id().as_deref(), Some(expected.revision_id()));
    support::assert_selected_matches(&expected, &definitions);

    // The bundled/prepared-image entry point remains strict and usable.
    let versions = db.query_row("SELECT (SELECT value FROM taxonomy_meta WHERE key='release_id'),(SELECT value FROM taxonomy_meta WHERE key='format_version')", [], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))).unwrap();
    let prepared = installed::prepare_image(&snapshot, (versions.0.parse().unwrap(), versions.1.parse().unwrap()), "test:published").unwrap();
    installed::install_snapshot(&prepared).unwrap();
    support::assert_selected_matches(&expected, &definitions);
}
