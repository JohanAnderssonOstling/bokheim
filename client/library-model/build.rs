#[derive(bincode::Encode)]
struct UnifiedConceptSnapshotRow {
    concept_id: String,
    preferred_label: String,
    parent_ids: String,
    parent_sort_orders: String,
    bisac_codes: String,
    ddc_codes: String,
    lcc_codes: String,
    source_labels: String,
}

use rusqlite::Connection;
use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=data/unified-taxonomy-v2.sqlite3");
    if !env::var("TARGET").expect("Cargo sets TARGET").starts_with("wasm32") {
        return;
    }

    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set"));
    let database_path = manifest_dir.join("data/unified-taxonomy-v2.sqlite3");
    let connection = Connection::open(&database_path).unwrap_or_else(|error| panic!("failed to open {}: {error}", database_path.display()));
    let integrity: String = connection.query_row("PRAGMA quick_check", [], |row| row.get(0)).expect("check taxonomy database integrity");
    assert_eq!(integrity, "ok", "taxonomy database failed its integrity check");
    let foreign_key_violations: i64 = connection.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| row.get(0)).expect("check taxonomy database foreign keys");
    assert_eq!(foreign_key_violations, 0, "taxonomy database contains invalid relationships");
    let format_version: String = connection.query_row("SELECT value FROM taxonomy_meta WHERE key='format_version'", [], |row| row.get(0)).expect("taxonomy database has a format version");
    assert_eq!(format_version, "2", "unsupported taxonomy database format");

    let mut statement = connection
        .prepare(
            "SELECT c.concept_id, c.preferred_label,
                    COALESCE((SELECT group_concat(parent_concept_id, '|') FROM (SELECT parent_concept_id FROM concept_parent WHERE concept_id=c.concept_id ORDER BY ordinal)), ''),
                    COALESCE((SELECT group_concat(display_order, '|') FROM (SELECT display_order FROM concept_parent WHERE concept_id=c.concept_id ORDER BY ordinal)), ''),
                    COALESCE((SELECT group_concat(selector, '|') FROM (SELECT selector FROM source_selector WHERE concept_id=c.concept_id AND system_id='bisac' ORDER BY ordinal)), ''),
                    COALESCE((SELECT group_concat(selector, '|') FROM (SELECT selector FROM source_selector WHERE concept_id=c.concept_id AND system_id='ddc' ORDER BY ordinal)), ''),
                    COALESCE((SELECT group_concat(selector, '|') FROM (SELECT selector FROM source_selector WHERE concept_id=c.concept_id AND system_id='lcc' ORDER BY ordinal)), ''),
                    COALESCE((SELECT group_concat(label, '|') FROM (SELECT label FROM source_label WHERE concept_id=c.concept_id ORDER BY ordinal)), '')
             FROM concept c ORDER BY c.rowid",
        )
        .expect("prepare taxonomy export");
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?, row.get::<_, String>(4)?, row.get::<_, String>(5)?, row.get::<_, String>(6)?, row.get::<_, String>(7)?))
        })
        .expect("query taxonomy concepts");
    let concepts = rows
        .map(|row| {
            let row = row.expect("read taxonomy concept");
            UnifiedConceptSnapshotRow { concept_id: row.0, preferred_label: row.1, parent_ids: row.2, parent_sort_orders: row.3, bisac_codes: row.4, ddc_codes: row.5, lcc_codes: row.6, source_labels: row.7 }
        })
        .collect::<Vec<_>>();
    let snapshot = bincode::encode_to_vec(concepts, bincode::config::standard()).expect("encode taxonomy snapshot");
    let output_path = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set")).join("unified-taxonomy-v2.bin");
    std::fs::write(&output_path, snapshot).unwrap_or_else(|error| panic!("failed to write {}: {error}", output_path.display()));
}
