#[path = "src/selector_format.rs"]
mod selector_format;
struct UnifiedConceptSnapshotRow {
    concept_id: i64,
    preferred_label: String,
    parent_ids: String,
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
    println!("cargo:rerun-if-changed=data/master-taxonomy-v2.sqlite3");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_AUTHORITATIVE_TAXONOMY");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_MASTER_TAXONOMY");
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set"));
    let database_path = if env::var_os("CARGO_FEATURE_MASTER_TAXONOMY").is_some() { manifest_dir.join("data/master-taxonomy-v2.sqlite3") } else { manifest_dir.join("data/unified-taxonomy-v2.sqlite3") };
    let connection = Connection::open(&database_path).unwrap_or_else(|error| panic!("failed to open {}: {error}", database_path.display()));
    let integrity: String = connection.query_row("PRAGMA quick_check", [], |row| row.get(0)).expect("check taxonomy database integrity");
    assert_eq!(integrity, "ok", "taxonomy database failed its integrity check");
    let foreign_key_violations: i64 = connection.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| row.get(0)).expect("check taxonomy database foreign keys");
    assert_eq!(foreign_key_violations, 0, "taxonomy database contains invalid relationships");
    let format_version: String = connection.query_row("SELECT value FROM taxonomy_meta WHERE key='format_version'", [], |row| row.get(0)).expect("taxonomy database has a format version");
    assert_eq!(format_version, "8", "unsupported taxonomy database format");
    let published_release_id: u64 =
        connection.query_row("SELECT value FROM taxonomy_meta WHERE key='release_id'", [], |row| row.get::<_, String>(0)).expect("taxonomy database has a release identifier").parse().expect("taxonomy release identifier is an integer");
    assert!(published_release_id > 0, "taxonomy release identifier must be positive");

    let output_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set"));
    let client_database_path = output_dir.join("unified-taxonomy-client-v2.sqlite3");
    if env::var_os("CARGO_FEATURE_AUTHORITATIVE_TAXONOMY").is_some() {
        std::fs::copy(&database_path, &client_database_path).unwrap_or_else(|error| panic!("failed to copy authoritative taxonomy to {}: {error}", client_database_path.display()));
    } else {
        build_client_database(&database_path, &client_database_path);
    }
    let client_connection = Connection::open(&client_database_path).unwrap_or_else(|error| panic!("failed to open {}: {error}", client_database_path.display()));
    // Navigation is compiled once into the shipped artifact, never rebuilt in a library.
    client_connection
        .execute_batch(
            "DROP TABLE IF EXISTS unified_concept_route;
         CREATE TABLE unified_concept_route(path TEXT PRIMARY KEY, concept_id INTEGER NOT NULL, parent_path TEXT NOT NULL, label TEXT NOT NULL) WITHOUT ROWID;
         INSERT INTO unified_concept_route
         WITH RECURSIVE routes(path,concept_id,parent_path,label) AS (
             SELECT c.preferred_label,c.concept_id,'Subject',c.preferred_label FROM concept c
             WHERE NOT EXISTS(SELECT 1 FROM concept_parent p WHERE p.concept_id=c.concept_id)
               AND (EXISTS(SELECT 1 FROM concept_parent p WHERE p.parent_concept_id=c.concept_id)
                 OR EXISTS(SELECT 1 FROM lcc_selector s WHERE s.concept_id=c.concept_id)
                 OR EXISTS(SELECT 1 FROM lcc_range s WHERE s.concept_id=c.concept_id)
                 OR EXISTS(SELECT 1 FROM bisac_selector s WHERE s.concept_id=c.concept_id)
                 OR EXISTS(SELECT 1 FROM ddc_selector s WHERE s.concept_id=c.concept_id))
             UNION
             SELECT r.path || ' / ' || c.preferred_label,c.concept_id,r.path,c.preferred_label
             FROM routes r JOIN concept_parent p ON p.parent_concept_id=r.concept_id
             JOIN concept c ON c.concept_id=p.concept_id
         ) SELECT * FROM routes;
         CREATE INDEX idx_unified_concept_route_parent ON unified_concept_route(parent_path,label,path);
         CREATE INDEX idx_unified_concept_route_concept ON unified_concept_route(concept_id,path);",
        )
        .expect("compile curated navigation routes");
    let concepts = read_concepts(&client_connection);
    let revision_id = revision_id(&concepts);
    let revision_path = output_dir.join("unified-taxonomy-revision-id.txt");
    std::fs::write(&revision_path, revision_id).unwrap_or_else(|error| panic!("failed to write {}: {error}", revision_path.display()));
}

fn read_concepts(connection: &Connection) -> Vec<UnifiedConceptSnapshotRow> {
    let mut statement = connection
        .prepare(
            "SELECT c.concept_id, c.preferred_label,
                    COALESCE((SELECT group_concat(parent_concept_id, '|') FROM (SELECT parent_concept_id FROM concept_parent WHERE concept_id=c.concept_id ORDER BY ordinal)), ''),
                    COALESCE((SELECT group_concat(selector, '|') FROM (SELECT selector FROM bisac_selector WHERE concept_id=c.concept_id ORDER BY selector)), ''),
                    COALESCE((SELECT group_concat(selector, '|') FROM (SELECT selector FROM ddc_selector WHERE concept_id=c.concept_id ORDER BY selector)), ''),
                    COALESCE((SELECT group_concat(selector, '|') FROM (
                        SELECT selector FROM lcc_selector WHERE concept_id=c.concept_id
                        UNION ALL
                        SELECT start_code || '..' || end_code
                        FROM lcc_range WHERE concept_id=c.concept_id
                        ORDER BY selector
                    )), ''),
                    COALESCE((SELECT group_concat(label, '|') FROM (SELECT label FROM source_label WHERE concept_id=c.concept_id ORDER BY ordinal)), '')
             FROM concept c ORDER BY c.rowid",
        )
        .expect("prepare taxonomy export");
    let rows = statement.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?, row.get::<_, String>(4)?, row.get::<_, String>(5)?, row.get::<_, String>(6)?))).expect("query taxonomy concepts");
    rows.map(|row| {
        let row = row.expect("read taxonomy concept");
        UnifiedConceptSnapshotRow {
            concept_id: row.0,
            preferred_label: row.1,
            parent_ids: row.2,
            bisac_codes: row.3,
            ddc_codes: row.4,
            lcc_codes: row.5.split('|').map(selector_format::canonical_lcc_selector).collect::<std::collections::BTreeSet<_>>().into_iter().collect::<Vec<_>>().join("|"),
            source_labels: row.6,
        }
    })
    .collect()
}

fn revision_id(concepts: &[UnifiedConceptSnapshotRow]) -> String {
    let mut fingerprint = String::new();
    for concept in concepts {
        write!(&mut fingerprint, "{}\u{1f}{}\u{1f}{}", concept.concept_id, concept.preferred_label, concept.parent_ids).unwrap();
        for (system, selectors) in [("bisac", &concept.bisac_codes), ("ddc", &concept.ddc_codes), ("lcc", &concept.lcc_codes)] {
            if !selectors.is_empty() {
                write!(&mut fingerprint, "\u{1f}{system}={selectors}").unwrap();
            }
        }
        writeln!(&mut fingerprint, "\u{1f}{}", concept.source_labels).unwrap();
    }
    const NAMESPACE: uuid::Uuid = uuid::Uuid::from_bytes([0xa7, 0xed, 0x9f, 0x7d, 0xa1, 0x2a, 0x59, 0xf5, 0xb5, 0x3d, 0x67, 0x4d, 0xe0, 0x80, 0x7c, 0x66]);
    const UNIFIED_VERSION: &str = "9";
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
             CREATE TABLE concept(concept_id INTEGER PRIMARY KEY,preferred_label TEXT NOT NULL CHECK(length(trim(preferred_label))>0)) STRICT;
             CREATE TABLE concept_parent(
                 concept_id INTEGER NOT NULL REFERENCES concept(concept_id) ON DELETE CASCADE,
                 parent_concept_id INTEGER NOT NULL REFERENCES concept(concept_id),
                 ordinal INTEGER NOT NULL CHECK(ordinal>=0),
                 PRIMARY KEY(concept_id,parent_concept_id),UNIQUE(concept_id,ordinal),CHECK(concept_id<>parent_concept_id)
             ) STRICT, WITHOUT ROWID;
             CREATE TABLE lcc_selector(
                 concept_id INTEGER NOT NULL REFERENCES concept(concept_id) ON DELETE CASCADE,
                 selector TEXT NOT NULL CHECK(length(trim(selector))>0),
                 PRIMARY KEY(concept_id,selector)
             ) STRICT, WITHOUT ROWID;
             CREATE TABLE bisac_selector(
                 concept_id INTEGER NOT NULL REFERENCES concept(concept_id) ON DELETE CASCADE,
                 selector TEXT NOT NULL CHECK(length(trim(selector))>0),
                 PRIMARY KEY(concept_id,selector)
             ) STRICT, WITHOUT ROWID;
             CREATE TABLE ddc_selector(
                 selector TEXT NOT NULL CHECK(length(selector)=3 AND selector GLOB '[0-9][0-9][0-9]'),
                 concept_id INTEGER NOT NULL REFERENCES concept(concept_id) ON DELETE CASCADE,
                 PRIMARY KEY(selector)
             ) STRICT, WITHOUT ROWID;
             CREATE TABLE lcc_range(
                 start_code TEXT NOT NULL CHECK(length(trim(start_code))>0),
                 end_code TEXT NOT NULL CHECK(length(trim(end_code))>0),
                 concept_id INTEGER NOT NULL REFERENCES concept(concept_id) ON DELETE CASCADE,
                 PRIMARY KEY(start_code,end_code)
             ) STRICT, WITHOUT ROWID;
             CREATE TABLE source_label(
                 concept_id INTEGER NOT NULL REFERENCES concept(concept_id) ON DELETE CASCADE,
                 label TEXT NOT NULL CHECK(length(trim(label))>0),ordinal INTEGER NOT NULL CHECK(ordinal>=0),
                 PRIMARY KEY(concept_id,label),UNIQUE(concept_id,ordinal)
             ) STRICT;
             CREATE INDEX concept_parent_parent_idx ON concept_parent(parent_concept_id,ordinal,concept_id);
             CREATE INDEX lcc_selector_lookup_idx ON lcc_selector(selector,concept_id);
             CREATE INDEX bisac_selector_lookup_idx ON bisac_selector(selector,concept_id);
             CREATE INDEX ddc_selector_concept_idx ON ddc_selector(concept_id);
             CREATE INDEX lcc_range_concept_idx ON lcc_range(concept_id);",
        )
        .expect("create client taxonomy schema");
    client.execute("ATTACH DATABASE ?1 AS authority", [authority_path.to_string_lossy().as_ref()]).expect("attach authoritative taxonomy");
    client
        .execute_batch(&format!(
            "CREATE TEMP TABLE client_concept(concept_id INTEGER PRIMARY KEY);
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
             INSERT INTO concept_parent SELECT concept_id,parent_concept_id,ordinal FROM authority.concept_parent WHERE concept_id IN client_concept ORDER BY concept_id,ordinal;
             INSERT INTO lcc_selector SELECT concept_id,selector FROM authority.lcc_selector WHERE concept_id IN client_concept ORDER BY concept_id,selector;
             INSERT INTO bisac_selector SELECT concept_id,selector FROM authority.bisac_selector WHERE concept_id IN client_concept ORDER BY concept_id,selector;
             INSERT INTO ddc_selector SELECT selector,concept_id FROM authority.ddc_selector WHERE concept_id IN client_concept ORDER BY selector;
             INSERT INTO lcc_range SELECT start_code,end_code,concept_id FROM authority.lcc_range WHERE concept_id IN client_concept ORDER BY start_code,end_code;
             INSERT INTO source_label SELECT concept_id,label,ordinal FROM authority.source_label WHERE concept_id IN client_concept ORDER BY concept_id,ordinal;"
        ))
        .expect("populate client taxonomy seed");
    client.execute_batch("DETACH DATABASE authority; VACUUM; PRAGMA optimize;").expect("finalize client taxonomy seed");
    let concept_count: i64 = client.query_row("SELECT count(*) FROM concept", [], |row| row.get(0)).expect("count client concepts");
    assert!((1..=2_000).contains(&concept_count), "client taxonomy seed unexpectedly contains {concept_count} concepts");
}
