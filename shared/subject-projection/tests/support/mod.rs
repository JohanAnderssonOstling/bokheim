use rusqlite::{Connection, OpenFlags};
use subject_projection::{UnifiedTaxonomy, read_unified_taxonomy_connection, unified_lcc_match_report};

pub fn selected_snapshot() -> (Vec<u8>, UnifiedTaxonomy, String) {
    let mut db = Connection::open_in_memory().unwrap();
    let image = std::fs::read(subject_projection::bundled::path().unwrap()).unwrap();
    db.deserialize_read_exact("main", image.as_slice(), image.len(), false).unwrap();
    // Distinguish the selected image without invalidating its compiled path indexes.
    let revision = "fixture:installed-snapshot".to_owned();
    db.execute("UPDATE taxonomy_meta SET value=?1 WHERE key='client_revision'", [&revision]).unwrap();
    let source = Connection::open_with_flags(concat!(env!("CARGO_MANIFEST_DIR"), "/data/unified-taxonomy-v2.sqlite3"), OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let definitions = read_unified_taxonomy_connection(&source, "authoring fixture").unwrap();
    let expected = UnifiedTaxonomy::from_concepts(definitions).unwrap();
    let image = db.serialize("main").unwrap().to_vec();
    (image, expected, revision)
}

pub fn assert_selected_matches(expected: &UnifiedTaxonomy) {
    for code in ["QA76.76", "D551", "ZZZZ999"] {
        assert_eq!(unified_lcc_match_report(code), expected.lcc_match_report(code), "{code}");
    }
}
