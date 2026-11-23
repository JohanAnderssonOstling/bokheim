//! The immutable runtime artifact contract, shared by build-time and release compilation.
use rusqlite::{Connection, params};

pub const FORMAT_VERSION: u32 = 2;
pub const PROJECTION_VERSION: i64 = 153;
pub const DOWNLOAD_FORMATS: &[u32] = &[FORMAT_VERSION];

/// Finish a private compiled copy. Authoring databases never pass this boundary.
pub fn finish(db: &Connection) -> Result<(), String> {
    let tx = db.unchecked_transaction().map_err(|e| e.to_string())?;
    tx.execute_batch(
        "CREATE TABLE taxonomy_path_alias(path TEXT PRIMARY KEY,concept_id INTEGER NOT NULL) WITHOUT ROWID;
        CREATE TABLE taxonomy_policy(key TEXT PRIMARY KEY,value TEXT NOT NULL) WITHOUT ROWID;
        DROP TABLE IF EXISTS concept_parent; DROP TABLE IF EXISTS lcc_selector;
        DROP TABLE IF EXISTS bisac_selector; DROP TABLE IF EXISTS ddc_selector;
        DROP TABLE IF EXISTS lcc_range; DROP TABLE IF EXISTS lcc_subclass;
        DROP INDEX IF EXISTS concept_label_lookup_idx;",
    )
    .map_err(|e| e.to_string())?;
    // The source's normalized label exists only for authoring lookups.
    let normalized: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('concept') WHERE name='normalized_label')", [], |r| r.get(0)).map_err(|e| e.to_string())?;
    if normalized {
        tx.execute_batch("ALTER TABLE concept DROP COLUMN normalized_label").map_err(|e| e.to_string())?;
    }
    let aliases: serde_json::Value = serde_json::from_str(include_str!("../data/reviewed-literature-path-aliases.json")).map_err(|e| e.to_string())?;
    for alias in aliases["aliases"].as_array().ok_or("missing reviewed aliases")? {
        let path = alias["path"].as_str().ok_or("invalid alias path")?;
        let concept = alias["concept_id"].as_i64().ok_or("invalid alias identity")?;
        tx.execute("INSERT INTO taxonomy_path_alias VALUES(?1,?2)", params![path, concept]).map_err(|e| e.to_string())?;
    }
    let mut headings: serde_json::Value = serde_json::from_str(include_str!("../data/reviewed-source-headings.json")).map_err(|e| e.to_string())?;
    for rule in headings.as_array_mut().ok_or("invalid source-heading policy")? {
        rule.as_object_mut().ok_or("invalid source-heading rule")?.remove("destination");
    }
    let headings = serde_json::to_string(&headings).map_err(|e| e.to_string())?;
    tx.execute("INSERT INTO taxonomy_policy VALUES('source_headings',?1)", [&headings]).map_err(|e| e.to_string())?;
    tx.execute("INSERT INTO taxonomy_meta(key,value) VALUES('format_version',?1),('minimum_projection_version',?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![FORMAT_VERSION.to_string(), PROJECTION_VERSION.to_string()])
        .map_err(|e| e.to_string())?;
    // Reviewed mappings are taxonomy data too: changing them must refresh book assignments.
    let base: String = tx.query_row("SELECT value FROM taxonomy_meta WHERE key='client_revision'", [], |r| r.get(0)).map_err(|e| e.to_string())?;
    let mut identity = blake3::Hasher::new();
    identity.update(base.as_bytes());
    {
        let mut aliases = tx.prepare("SELECT path,concept_id FROM taxonomy_path_alias ORDER BY path").map_err(|e| e.to_string())?;
        for alias in aliases.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))).map_err(|e| e.to_string())? {
            let (path, id) = alias.map_err(|e| e.to_string())?;
            identity.update(&(path.len() as u64).to_le_bytes());
            identity.update(path.as_bytes());
            identity.update(&id.to_le_bytes());
        }
    }
    identity.update(headings.as_bytes());
    tx.execute("UPDATE taxonomy_meta SET value=?1 WHERE key='client_revision'", [format!("runtime:{}", identity.finalize().to_hex())]).map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM taxonomy_meta WHERE key IN ('source','client_seed_depth')", []).map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    db.execute_batch("VACUUM; PRAGMA journal_mode=DELETE;").map_err(|e| e.to_string())
}

/// Validate the same runtime contract during packaging, staging and loading.
pub fn validate(db: &Connection) -> Result<(), String> {
    let format: u32 = db.query_row("SELECT value FROM taxonomy_meta WHERE key='format_version'", [], |r| r.get::<_, String>(0)).map_err(|e| e.to_string())?.parse().map_err(|e: std::num::ParseIntError| e.to_string())?;
    if format != FORMAT_VERSION {
        return Err(format!("unsupported taxonomy runtime format {format}"));
    }
    let minimum: i64 = db.query_row("SELECT value FROM taxonomy_meta WHERE key='minimum_projection_version'", [], |r| r.get::<_, String>(0)).map_err(|e| e.to_string())?.parse().map_err(|e: std::num::ParseIntError| e.to_string())?;
    if minimum > PROJECTION_VERSION {
        return Err("taxonomy requires a newer projection policy".into());
    }
    let policy: bool =
        db.query_row("SELECT EXISTS(SELECT 1 FROM taxonomy_policy WHERE key='source_headings') AND EXISTS(SELECT 1 FROM sqlite_schema WHERE name='taxonomy_path_alias' AND type='table')", [], |r| r.get(0)).map_err(|e| e.to_string())?;
    if !policy {
        return Err("taxonomy runtime policy data is missing".into());
    }
    let prepared: bool = db
            .query_row(
                "SELECT (SELECT count(*) FROM sqlite_schema WHERE type='table' AND name IN ('taxonomy_match_exact','taxonomy_match_prefix','taxonomy_match_subclass','taxonomy_match_order','taxonomy_match_range','taxonomy_match_interval','taxonomy_match_route','concept','unified_concept_route'))=9 AND EXISTS(SELECT 1 FROM taxonomy_meta WHERE key='client_matcher_version' AND value=?1)",
                [crate::matcher_storage::UNIFIED_MATCHER_FORMAT_VERSION.to_string()],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
    if !prepared {
        return Err(format!("taxonomy must be prepared for matcher format version {}", crate::matcher_storage::UNIFIED_MATCHER_FORMAT_VERSION));
    }
    Ok(())
}
