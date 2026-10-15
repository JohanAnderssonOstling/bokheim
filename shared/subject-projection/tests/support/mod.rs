use rusqlite::{params, Connection};
use subject_projection::{read_unified_taxonomy_connection, unified_concept_id, unified_lcc_match_report, UnifiedConceptDefinition, UnifiedTaxonomy};

pub fn selected_snapshot() -> (Vec<u8>, Vec<UnifiedConceptDefinition>) {
    let mut db = Connection::open_in_memory().unwrap();
    let image = std::fs::read(subject_projection::bundled::path().unwrap()).unwrap();
    db.deserialize_read_exact("main", image.as_slice(), image.len(), false).unwrap();
    let id: i64 = db.query_row("SELECT min(concept_id) FROM concept", [], |row| row.get(0)).unwrap();
    db.execute("UPDATE concept SET preferred_label=?1 WHERE concept_id=?2", params!["Installed snapshot test label", id]).unwrap();
    let definitions = read_unified_taxonomy_connection(&db, "test snapshot").unwrap();
    let image = db.serialize("main").unwrap().to_vec();
    (image, definitions)
}

pub fn assert_selected_matches(expected: &UnifiedTaxonomy, definitions: &[UnifiedConceptDefinition]) {
    let selected = definitions.iter().find(|concept| concept.preferred_label() == "Installed snapshot test label").unwrap();
    assert_eq!(unified_concept_id(selected.preferred_label()), Some(selected.concept_id()));
    for code in ["QA76.76", "D551", "ZZZZ999"] {
        assert_eq!(unified_lcc_match_report(code), expected.lcc_match_report(code), "{code}");
    }
}
