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
use std::fmt::Write as _;
use std::path::PathBuf;

const CLIENT_TAXONOMY_DEPTH: u8 = 2;

fn main() {
    println!("cargo:rerun-if-changed=data/unified-taxonomy-v2.sqlite3");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_AUTHORITATIVE_TAXONOMY");
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set"));
    let database_path = manifest_dir.join("data/unified-taxonomy-v2.sqlite3");
    let connection = Connection::open(&database_path).unwrap_or_else(|error| panic!("failed to open {}: {error}", database_path.display()));
    let integrity: String = connection.query_row("PRAGMA quick_check", [], |row| row.get(0)).expect("check taxonomy database integrity");
    assert_eq!(integrity, "ok", "taxonomy database failed its integrity check");
    let foreign_key_violations: i64 = connection.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| row.get(0)).expect("check taxonomy database foreign keys");
    assert_eq!(foreign_key_violations, 0, "taxonomy database contains invalid relationships");
    let format_version: String = connection.query_row("SELECT value FROM taxonomy_meta WHERE key='format_version'", [], |row| row.get(0)).expect("taxonomy database has a format version");
    assert_eq!(format_version, "2", "unsupported taxonomy database format");

    let output_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set"));
    let client_database_path = output_dir.join("unified-taxonomy-client-v2.sqlite3");
    if env::var_os("CARGO_FEATURE_AUTHORITATIVE_TAXONOMY").is_some() {
        std::fs::copy(&database_path, &client_database_path).unwrap_or_else(|error| panic!("failed to copy authoritative taxonomy to {}: {error}", client_database_path.display()));
    } else {
        build_client_database(&database_path, &client_database_path);
    }
    let client_connection = Connection::open(&client_database_path).unwrap_or_else(|error| panic!("failed to open {}: {error}", client_database_path.display()));
    let concepts = read_concepts(&client_connection);
    let release_id = release_id(&concepts);
    let release_path = output_dir.join("unified-taxonomy-release-id.txt");
    std::fs::write(&release_path, release_id).unwrap_or_else(|error| panic!("failed to write {}: {error}", release_path.display()));

    if env::var("TARGET").expect("Cargo sets TARGET").starts_with("wasm32") {
        let snapshot = bincode::encode_to_vec(concepts, bincode::config::standard()).expect("encode taxonomy snapshot");
        let output_path = output_dir.join("unified-taxonomy-v2.bin");
        std::fs::write(&output_path, snapshot).unwrap_or_else(|error| panic!("failed to write {}: {error}", output_path.display()));
    }
}

fn read_concepts(connection: &Connection) -> Vec<UnifiedConceptSnapshotRow> {
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
    rows.map(|row| {
        let row = row.expect("read taxonomy concept");
        UnifiedConceptSnapshotRow { concept_id: row.0, preferred_label: row.1, parent_ids: row.2, parent_sort_orders: row.3, bisac_codes: row.4, ddc_codes: row.5, lcc_codes: row.6, source_labels: row.7 }
    })
    .collect()
}

fn release_id(concepts: &[UnifiedConceptSnapshotRow]) -> String {
    let mut fingerprint = String::new();
    for concept in concepts {
        write!(&mut fingerprint, "{}\u{1f}{}\u{1f}{}\u{1f}{}", concept.concept_id, concept.preferred_label, concept.parent_ids, concept.parent_sort_orders).unwrap();
        for (system, selectors) in [("bisac", &concept.bisac_codes), ("ddc", &concept.ddc_codes), ("lcc", &concept.lcc_codes)] {
            if !selectors.is_empty() {
                write!(&mut fingerprint, "\u{1f}{system}={selectors}").unwrap();
            }
        }
        writeln!(&mut fingerprint, "\u{1f}{}", concept.source_labels).unwrap();
    }
    const NAMESPACE: uuid::Uuid = uuid::Uuid::from_bytes([0xa7, 0xed, 0x9f, 0x7d, 0xa1, 0x2a, 0x59, 0xf5, 0xb5, 0x3d, 0x67, 0x4d, 0xe0, 0x80, 0x7c, 0x66]);
    const UNIFIED_VERSION: &str = "6";
    format!("{UNIFIED_VERSION}:{}", uuid::Uuid::new_v5(&NAMESPACE, fingerprint.as_bytes()))
}

fn build_client_database(authority_path: &PathBuf, client_path: &PathBuf) {
    if client_path.exists() {
        std::fs::remove_file(client_path).unwrap_or_else(|error| panic!("failed to replace {}: {error}", client_path.display()));
    }
    let client = Connection::open(client_path).unwrap_or_else(|error| panic!("failed to create {}: {error}", client_path.display()));
    client
        .execute_batch(
            "PRAGMA foreign_keys=ON;
             CREATE TABLE taxonomy_meta(key TEXT PRIMARY KEY,value TEXT NOT NULL) STRICT;
             CREATE TABLE concept(concept_id TEXT PRIMARY KEY,preferred_label TEXT NOT NULL CHECK(length(trim(preferred_label))>0)) STRICT;
             CREATE TABLE concept_parent(
                 concept_id TEXT NOT NULL REFERENCES concept(concept_id) ON DELETE CASCADE,
                 parent_concept_id TEXT NOT NULL REFERENCES concept(concept_id),
                 ordinal INTEGER NOT NULL CHECK(ordinal>=0),
                 display_order INTEGER NOT NULL DEFAULT 0,
                 PRIMARY KEY(concept_id,parent_concept_id),UNIQUE(concept_id,ordinal),CHECK(concept_id<>parent_concept_id)
             ) STRICT;
             CREATE TABLE source_selector(
                 concept_id TEXT NOT NULL REFERENCES concept(concept_id) ON DELETE CASCADE,
                 system_id TEXT NOT NULL CHECK(system_id IN ('bisac','ddc','lcc')),
                 selector TEXT NOT NULL CHECK(length(trim(selector))>0),ordinal INTEGER NOT NULL CHECK(ordinal>=0),
                 PRIMARY KEY(concept_id,system_id,selector),UNIQUE(concept_id,system_id,ordinal)
             ) STRICT;
             CREATE TABLE source_label(
                 concept_id TEXT NOT NULL REFERENCES concept(concept_id) ON DELETE CASCADE,
                 label TEXT NOT NULL CHECK(length(trim(label))>0),ordinal INTEGER NOT NULL CHECK(ordinal>=0),
                 PRIMARY KEY(concept_id,label),UNIQUE(concept_id,ordinal)
             ) STRICT;
             CREATE INDEX concept_parent_parent_idx ON concept_parent(parent_concept_id,ordinal,concept_id);
             CREATE INDEX source_selector_lookup_idx ON source_selector(system_id,selector,concept_id);",
        )
        .expect("create client taxonomy schema");
    client.execute("ATTACH DATABASE ?1 AS authority", [authority_path.to_string_lossy().as_ref()]).expect("attach authoritative taxonomy");
    client
        .execute_batch(&format!(
            "CREATE TEMP TABLE client_concept(concept_id TEXT PRIMARY KEY);
             INSERT INTO client_concept
             WITH RECURSIVE selected(concept_id,depth) AS (
                 SELECT c.concept_id,0 FROM authority.concept c
                 WHERE NOT EXISTS(SELECT 1 FROM authority.concept_parent p WHERE p.concept_id=c.concept_id)
                 UNION
                 SELECT edge.concept_id,selected.depth+1
                 FROM selected JOIN authority.concept_parent edge ON edge.parent_concept_id=selected.concept_id
                 WHERE selected.depth<{CLIENT_TAXONOMY_DEPTH}
             ), ancestors(concept_id) AS (
                 SELECT concept_id FROM selected
                 UNION
                 SELECT edge.parent_concept_id
                 FROM authority.concept_parent edge JOIN ancestors ON edge.concept_id=ancestors.concept_id
             )
             SELECT DISTINCT concept_id FROM ancestors;
             INSERT INTO taxonomy_meta SELECT key,value FROM authority.taxonomy_meta;
             INSERT INTO taxonomy_meta(key,value) VALUES('client_seed_depth','{CLIENT_TAXONOMY_DEPTH}');
             INSERT INTO concept SELECT concept_id,preferred_label FROM authority.concept WHERE concept_id IN client_concept ORDER BY rowid;
             INSERT INTO concept_parent SELECT concept_id,parent_concept_id,ordinal,display_order FROM authority.concept_parent WHERE concept_id IN client_concept ORDER BY concept_id,ordinal;
             INSERT INTO source_selector SELECT concept_id,system_id,selector,ordinal FROM authority.source_selector WHERE concept_id IN client_concept ORDER BY concept_id,system_id,ordinal;
             INSERT INTO source_label SELECT concept_id,label,ordinal FROM authority.source_label WHERE concept_id IN client_concept ORDER BY concept_id,ordinal;"
        ))
        .expect("populate client taxonomy seed");
    client.execute_batch("DETACH DATABASE authority; VACUUM; PRAGMA optimize;").expect("finalize client taxonomy seed");
    let concept_count: u64 = client.query_row("SELECT count(*) FROM concept", [], |row| row.get(0)).expect("count client concepts");
    assert!((1..=2_000).contains(&concept_count), "client taxonomy seed unexpectedly contains {concept_count} concepts");
}
