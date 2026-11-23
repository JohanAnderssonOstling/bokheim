#![cfg(not(target_arch = "wasm32"))]

mod support;

use rusqlite::Connection;
use subject_projection::{installed, loaded_unified_taxonomy_revision_id, unified_taxonomy_revision_id};

#[test]
fn selected_file_stays_pinned_when_a_new_reference_is_prepared() {
    let (image, expected, revision) = support::selected_snapshot();
    let directory = tempfile::tempdir().unwrap();
    let selected = directory.path().join("selected taxonomy #1?.sqlite3");
    std::fs::write(&selected, &image).unwrap();
    installed::validate(&selected, installed::metadata(&selected).unwrap()).unwrap();
    installed::activate(&selected).unwrap();
    assert!(loaded_unified_taxonomy_revision_id().is_none());

    let reader = Connection::open_in_memory().unwrap();
    reader.execute("ATTACH ?1 AS curated", [installed::read_only_uri(&selected).unwrap()]).unwrap();
    let replacement = directory.path().join("next.sqlite3");
    std::fs::copy(&selected, &replacement).unwrap();
    Connection::open(&replacement).unwrap().execute("UPDATE taxonomy_meta SET value='fixture:next-reference' WHERE key='client_revision'", []).unwrap();
    installed::validate(&replacement, installed::metadata(&replacement).unwrap()).unwrap();
    assert!(installed::activate(&replacement).is_err());
    assert!(loaded_unified_taxonomy_revision_id().is_none());
    let pinned_revision: String = reader.query_row("SELECT value FROM curated.taxonomy_meta WHERE key='client_revision'", [], |row| row.get(0)).unwrap();
    assert_eq!(pinned_revision, revision);
    assert!(reader.execute("DELETE FROM curated.concept", []).is_err());

    assert_eq!(unified_taxonomy_revision_id(), revision);
    support::assert_selected_matches(&expected);
}
