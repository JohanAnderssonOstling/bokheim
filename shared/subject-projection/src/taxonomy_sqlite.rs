use crate::{UnifiedCodeResolution, UnifiedConceptDefinition, DEFAULT_UNIFIED_TAXONOMY_SQLITE};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

fn split_lcc_range(selector: &str) -> Result<(&str, &str), String> {
    let (start, end) = selector.split_once("..").ok_or_else(|| format!("invalid LCC range {selector}"))?;
    crate::lcc::lcc_span(&format!("{start}-{end}")).ok_or_else(|| format!("invalid LCC range {selector}"))?;
    Ok((start, end))
}

fn upgrade_lcc_ranges(connection: &Connection) -> Result<(), String> {
    let version: String = connection.query_row("SELECT value FROM taxonomy_meta WHERE key='format_version'", [], |r| r.get(0)).map_err(|e| e.to_string())?;
    if version == "7" {
        connection.execute_batch(include_str!("../data/migrations/20260907_lcc_range_endpoints_v8.sql")).map_err(|e| format!("failed to migrate LCC ranges: {e}"))?;
    }
    connection.execute_batch("CREATE TABLE IF NOT EXISTS ddc_selector(selector TEXT NOT NULL CHECK(length(selector)=3 AND selector GLOB '[0-9][0-9][0-9]'),concept_id INTEGER NOT NULL REFERENCES concept(concept_id) ON DELETE CASCADE,PRIMARY KEY(selector)) STRICT, WITHOUT ROWID; CREATE INDEX IF NOT EXISTS ddc_selector_concept_idx ON ddc_selector(concept_id);").map_err(|e| format!("failed to initialize DDC selectors: {e}"))?;
    Ok(())
}

pub(crate) fn read_embedded_unified_taxonomy() -> Result<Vec<UnifiedConceptDefinition>, String> {
    let mut connection = Connection::open_in_memory().map_err(|error| format!("failed to open embedded taxonomy database: {error}"))?;
    connection.deserialize_bytes("main", DEFAULT_UNIFIED_TAXONOMY_SQLITE).map_err(|error| format!("failed to open embedded taxonomy database image: {error}"))?;
    read_unified_taxonomy_connection(&connection, "embedded unified taxonomy")
}

pub fn read_unified_taxonomy_sqlite(path: &Path) -> Result<Vec<UnifiedConceptDefinition>, String> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|error| format!("failed to open unified taxonomy database {}: {error}", path.display()))?;
    read_unified_taxonomy_connection(&connection, &path.display().to_string())
}

pub fn reset_unified_taxonomy_connection(connection: &mut Connection) -> Result<(), String> {
    let mut seed = Connection::open_in_memory().map_err(|error| format!("failed to open embedded taxonomy database: {error}"))?;
    seed.deserialize_bytes("main", DEFAULT_UNIFIED_TAXONOMY_SQLITE).map_err(|error| format!("failed to open embedded taxonomy database image: {error}"))?;
    let backup = rusqlite::backup::Backup::new(&seed, connection).map_err(|error| format!("failed to initialize taxonomy cache: {error}"))?;
    loop {
        match backup.step(-1).map_err(|error| format!("failed to initialize taxonomy cache: {error}"))? {
            rusqlite::backup::StepResult::Done => break,
            rusqlite::backup::StepResult::More => continue,
            rusqlite::backup::StepResult::Busy | rusqlite::backup::StepResult::Locked => return Err("taxonomy cache is busy while installing its bundled seed".to_owned()),
            _ => return Err("taxonomy cache returned an unknown backup state while installing its bundled seed".to_owned()),
        }
    }
    Ok(())
}

/// Checks whether the device-local copy contains server-provided concepts.
/// The bundled image has no such marker, allowing startup to avoid reading all
/// taxonomy rows merely to discover that it is identical to the embedded one.
pub fn has_unified_taxonomy_overlay(path: &Path) -> Result<bool, String> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|error| format!("failed to open unified taxonomy database {}: {error}", path.display()))?;
    has_unified_taxonomy_overlay_in(&connection, &path.display().to_string())
}

pub fn has_unified_taxonomy_overlay_in(connection: &Connection, source: &str) -> Result<bool, String> {
    let format_version = connection.query_row("SELECT value FROM taxonomy_meta WHERE key='format_version'", [], |row| row.get::<_, String>(0)).map_err(|error| format!("failed to read taxonomy database format {source}: {error}"))?;
    if format_version != "7" && format_version != "8" {
        return Err(format!("unsupported unified taxonomy database format {format_version} in {source}"));
    }
    connection.query_row("SELECT EXISTS(SELECT 1 FROM taxonomy_meta WHERE key='server_release_id')", [], |row| row.get::<_, bool>(0)).map_err(|error| format!("failed to inspect taxonomy overlay marker {source}: {error}"))
}

pub fn is_unified_taxonomy_client_seed(path: &Path) -> Result<bool, String> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|error| format!("failed to open unified taxonomy database {}: {error}", path.display()))?;
    is_unified_taxonomy_client_seed_in(&connection, &path.display().to_string())
}

pub fn is_unified_taxonomy_client_seed_in(connection: &Connection, source: &str) -> Result<bool, String> {
    connection
        .query_row("SELECT value='2' FROM taxonomy_meta WHERE key='client_seed_depth'", [], |row| row.get::<_, bool>(0))
        .optional()
        .map(|value| value.unwrap_or(false))
        .map_err(|error| format!("failed to inspect taxonomy client seed marker {source}: {error}"))
}

/// Returns the authoritative server release associated with the cached
/// overlay. A seed-only database has no server release.
pub fn unified_taxonomy_server_release_id(path: &Path) -> Result<Option<u64>, String> {
    if !path.is_file() {
        return Ok(None);
    }
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|error| format!("failed to open local unified taxonomy database {}: {error}", path.display()))?;
    unified_taxonomy_server_release_id_in(&connection, &path.display().to_string())
}

/// Returns the monotonically increasing release assigned to an authoritative
/// taxonomy snapshot when it is published.
pub fn published_taxonomy_release_id(path: &Path) -> Result<u64, String> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|error| format!("failed to open taxonomy database {}: {error}", path.display()))?;
    let value: String = connection
        .query_row("SELECT value FROM taxonomy_meta WHERE key='release_id'", [], |row| row.get(0))
        .map_err(|error| format!("failed to read taxonomy release from {}: {error}", path.display()))?;
    let release_id = value.parse::<u64>().map_err(|error| format!("invalid taxonomy release {value:?} in {}: {error}", path.display()))?;
    if release_id == 0 {
        return Err(format!("invalid taxonomy release 0 in {}", path.display()));
    }
    Ok(release_id)
}

pub fn unified_taxonomy_server_release_id_in(connection: &Connection, source: &str) -> Result<Option<u64>, String> {
    let value = connection
        .query_row("SELECT value FROM taxonomy_meta WHERE key='server_release_id'", [], |row| row.get::<_, String>(0))
        .optional()
        .map_err(|error| format!("failed to read taxonomy server release {source}: {error}"))?;
    value
        .map(|value| value.parse::<u64>().map_err(|error| format!("invalid taxonomy server release {value:?} in {source}: {error}")))
        .transpose()
}

/// Returns precise system/code pairs previously resolved by the taxonomy
/// server. A row with no matched concepts is still a completed resolution.
pub fn read_resolved_unified_taxonomy_codes(path: &Path, candidates: &[(String, String)]) -> Result<BTreeSet<(String, String)>, String> {
    if candidates.is_empty() || !path.is_file() {
        return Ok(BTreeSet::new());
    }
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|error| format!("failed to open local unified taxonomy database {}: {error}", path.display()))?;
    read_resolved_unified_taxonomy_codes_in(&connection, candidates, &path.display().to_string())
}

pub fn read_resolved_unified_taxonomy_codes_in(connection: &Connection, candidates: &[(String, String)], source: &str) -> Result<BTreeSet<(String, String)>, String> {
    if candidates.is_empty() {
        return Ok(BTreeSet::new());
    }
    let exists = connection
        .query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='taxonomy_code_resolution')", [], |row| row.get::<_, bool>(0))
        .map_err(|error| format!("failed to inspect local taxonomy resolution ledger {source}: {error}"))?;
    if !exists {
        return Ok(BTreeSet::new());
    }
    let mut statement = connection.prepare("SELECT EXISTS(SELECT 1 FROM taxonomy_code_resolution WHERE system_id=?1 AND code=?2)").map_err(|error| format!("failed to prepare local taxonomy resolution lookup {source}: {error}"))?;
    let mut resolved = BTreeSet::new();
    for (system_id, code) in candidates {
        let exists = statement.query_row(params![system_id, code], |row| row.get::<_, bool>(0)).map_err(|error| format!("failed to read local taxonomy resolution {system_id}:{code}: {error}"))?;
        if exists {
            resolved.insert((system_id.clone(), code.clone()));
        }
    }
    Ok(resolved)
}

/// Transactionally overlays a complete server slice onto the device-local
/// taxonomy. The slice must be ancestor-closed, allowing foreign keys to be
/// checked without temporarily inventing placeholder concepts.
pub fn merge_unified_taxonomy_sqlite(path: &Path, server_release_id: u64, concepts: &[UnifiedConceptDefinition]) -> Result<(), String> {
    merge_unified_taxonomy_sqlite_with_resolutions(path, server_release_id, concepts, &[])
}

pub fn merge_unified_taxonomy_sqlite_with_resolutions(path: &Path, server_release_id: u64, concepts: &[UnifiedConceptDefinition], resolutions: &[UnifiedCodeResolution]) -> Result<(), String> {
    let mut connection = Connection::open(path).map_err(|error| format!("failed to open local unified taxonomy database {}: {error}", path.display()))?;
    merge_unified_taxonomy_connection(&mut connection, server_release_id, concepts, resolutions, &path.display().to_string())
}

pub fn merge_unified_taxonomy_connection(connection: &mut Connection, server_release_id: u64, concepts: &[UnifiedConceptDefinition], resolutions: &[UnifiedCodeResolution], source: &str) -> Result<(), String> {
    connection.pragma_update(None, "foreign_keys", true).map_err(|error| format!("failed to enable local taxonomy constraints {source}: {error}"))?;
    upgrade_lcc_ranges(connection)?;
    let format_version = connection.query_row("SELECT value FROM taxonomy_meta WHERE key='format_version'", [], |row| row.get::<_, String>(0)).map_err(|error| format!("failed to read local taxonomy format {source}: {error}"))?;
    if format_version != "7" && format_version != "8" {
        return Err(format!("unsupported local unified taxonomy format {format_version} in {source}"));
    }
    let transaction = connection.transaction().map_err(|error| format!("failed to start local taxonomy merge {source}: {error}"))?;
    transaction
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS taxonomy_code_resolution(
                 system_id TEXT NOT NULL,
                 code TEXT NOT NULL,
                 server_release_id TEXT NOT NULL,
                 PRIMARY KEY(system_id,code)
             );
             CREATE TABLE IF NOT EXISTS taxonomy_code_resolution_concept(
                 system_id TEXT NOT NULL,
                 code TEXT NOT NULL,
                 concept_id INTEGER NOT NULL REFERENCES concept(concept_id),
                 ordinal INTEGER NOT NULL,
                 PRIMARY KEY(system_id,code,concept_id),
                 FOREIGN KEY(system_id,code) REFERENCES taxonomy_code_resolution(system_id,code) ON DELETE CASCADE
             );",
        )
        .map_err(|error| format!("failed to initialize local taxonomy resolution ledger {source}: {error}"))?;
    let server_release_id = server_release_id.to_string();
    let has_stale_resolutions = transaction
        .query_row("SELECT EXISTS(SELECT 1 FROM taxonomy_code_resolution WHERE server_release_id<>?1)", [&server_release_id], |row| row.get::<_, bool>(0))
        .map_err(|error| format!("failed to inspect taxonomy resolution releases {source}: {error}"))?;
    if has_stale_resolutions {
        transaction.execute("DELETE FROM taxonomy_code_resolution", []).map_err(|error| format!("failed to invalidate stale taxonomy resolutions {source}: {error}"))?;
    }
    for concept in concepts {
        transaction
            .execute(
                "INSERT INTO concept(concept_id,preferred_label) VALUES(?1,?2)
                 ON CONFLICT(concept_id) DO UPDATE SET preferred_label=excluded.preferred_label",
                params![concept.concept_id(), concept.preferred_label()],
            )
            .map_err(|error| format!("failed to merge taxonomy concept {}: {error}", concept.concept_id()))?;
    }
    for resolution in resolutions {
        if resolution.system_id() == "lcc" && resolution.matched_concept_ids().iter().collect::<std::collections::BTreeSet<_>>().len() > 1 {
            return Err(format!("ambiguous LCC resolution {} must not assign multiple concepts", resolution.code()));
        }
        if !matches!(resolution.system_id(), "bisac" | "ddc" | "lcc") || resolution.code().trim().is_empty() {
            return Err(format!("invalid taxonomy code resolution {}:{}", resolution.system_id(), resolution.code()));
        }
        transaction
            .execute(
                "INSERT INTO taxonomy_code_resolution(system_id,code,server_release_id) VALUES(?1,?2,?3)
                 ON CONFLICT(system_id,code) DO UPDATE SET server_release_id=excluded.server_release_id",
                params![resolution.system_id(), resolution.code(), &server_release_id],
            )
            .map_err(|error| format!("failed to record taxonomy resolution {}:{}: {error}", resolution.system_id(), resolution.code()))?;
        transaction
            .execute("DELETE FROM taxonomy_code_resolution_concept WHERE system_id=?1 AND code=?2", params![resolution.system_id(), resolution.code()])
            .map_err(|error| format!("failed to replace taxonomy resolution {}:{}: {error}", resolution.system_id(), resolution.code()))?;
        for (ordinal, concept_id) in resolution.matched_concept_ids().iter().enumerate() {
            transaction
                .execute("INSERT INTO taxonomy_code_resolution_concept(system_id,code,concept_id,ordinal) VALUES(?1,?2,?3,?4)", params![resolution.system_id(), resolution.code(), concept_id, ordinal as i64])
                .map_err(|error| format!("failed to link taxonomy resolution {}:{} to {concept_id}: {error}", resolution.system_id(), resolution.code()))?;
        }
    }
    for concept in concepts {
        transaction.execute("DELETE FROM concept_parent WHERE concept_id=?1", [concept.concept_id()]).map_err(|error| format!("failed to replace taxonomy parents for {}: {error}", concept.concept_id()))?;
        transaction.execute("DELETE FROM lcc_selector WHERE concept_id=?1", [concept.concept_id()]).map_err(|error| format!("failed to replace taxonomy LCC selectors for {}: {error}", concept.concept_id()))?;
        transaction.execute("DELETE FROM bisac_selector WHERE concept_id=?1", [concept.concept_id()]).map_err(|error| format!("failed to replace taxonomy BISAC selectors for {}: {error}", concept.concept_id()))?;
        transaction.execute("DELETE FROM ddc_selector WHERE concept_id=?1", [concept.concept_id()]).map_err(|error| format!("failed to replace taxonomy DDC selectors for {}: {error}", concept.concept_id()))?;
        transaction.execute("DELETE FROM lcc_range WHERE concept_id=?1", [concept.concept_id()]).map_err(|error| format!("failed to replace taxonomy ranges for {}: {error}", concept.concept_id()))?;
        transaction.execute("DELETE FROM source_label WHERE concept_id=?1", [concept.concept_id()]).map_err(|error| format!("failed to replace taxonomy labels for {}: {error}", concept.concept_id()))?;
        for (ordinal, parent) in concept.parent_ids().iter().enumerate() {
            transaction
                .execute("INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(?1,?2,?3)", params![concept.concept_id(), parent, ordinal as i64])
                .map_err(|error| format!("failed to merge taxonomy parent for {}: {error}", concept.concept_id()))?;
        }
        for system in ["bisac", "ddc", "lcc"] {
            for selector in concept.source_selectors(system) {
                if system == "lcc" && selector.contains("..") {
                    let (start_code, end_code) = split_lcc_range(selector)?;
                    transaction.execute(
                        "INSERT INTO lcc_range(start_code,end_code,concept_id) VALUES(?1,?2,?3)",
                        params![start_code,end_code,concept.concept_id()],
                    ).map_err(|error| format!("failed to merge taxonomy range for {}: {error}", concept.concept_id()))?;
                } else {
                    let table = match system { "lcc" => "lcc_selector", "ddc" => "ddc_selector", _ => "bisac_selector" };
                    transaction
                        .execute(&format!("INSERT INTO {table}(concept_id,selector) VALUES(?1,?2)"), params![concept.concept_id(), selector])
                        .map_err(|error| format!("failed to merge taxonomy selector for {}: {error}", concept.concept_id()))?;
                }
            }
        }
        for (ordinal, label) in concept.source_labels().iter().enumerate() {
            transaction
                .execute("INSERT INTO source_label(concept_id,label,ordinal) VALUES(?1,?2,?3)", params![concept.concept_id(), label, ordinal as i64])
                .map_err(|error| format!("failed to merge taxonomy source label for {}: {error}", concept.concept_id()))?;
        }
    }
    transaction
        .execute(
            "INSERT INTO taxonomy_meta(key,value) VALUES('server_release_id',?1)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [&server_release_id],
        )
        .map_err(|error| format!("failed to record taxonomy server release: {error}"))?;
    transaction.commit().map_err(|error| format!("failed to commit local taxonomy merge {source}: {error}"))?;
    let violations = connection.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| row.get::<_, i64>(0)).map_err(|error| format!("failed to verify local taxonomy merge {source}: {error}"))?;
    if violations != 0 {
        return Err(format!("local taxonomy merge left {violations} foreign-key violations in {source}"));
    }
    Ok(())
}

pub fn read_unified_taxonomy_connection(connection: &Connection, source: &str) -> Result<Vec<UnifiedConceptDefinition>, String> {
    connection.pragma_update(None, "foreign_keys", true).map_err(|error| format!("failed to enable taxonomy database constraints {source}: {error}"))?;
    let integrity = connection.query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0)).map_err(|error| format!("failed to verify taxonomy database {source}: {error}"))?;
    if integrity != "ok" {
        return Err(format!("unified taxonomy database failed its integrity check {source}: {integrity}"));
    }
    let foreign_key_violations = connection.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| row.get::<_, i64>(0)).map_err(|error| format!("failed to verify taxonomy database relationships {source}: {error}"))?;
    if foreign_key_violations != 0 {
        return Err(format!("unified taxonomy database contains {foreign_key_violations} invalid relationships: {source}"));
    }
    let format_version = connection.query_row("SELECT value FROM taxonomy_meta WHERE key='format_version'", [], |row| row.get::<_, String>(0)).map_err(|error| format!("failed to read taxonomy database format {source}: {error}"))?;
    if format_version != "7" && format_version != "8" {
        return Err(format!("unsupported unified taxonomy database format {format_version} in {source}"));
    }

    let mut parents = BTreeMap::<i64, Vec<i64>>::new();
    {
        let mut statement = connection.prepare("SELECT concept_id,parent_concept_id FROM concept_parent ORDER BY concept_id,ordinal").map_err(|error| format!("failed to prepare taxonomy parents {source}: {error}"))?;
        let rows = statement.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))).map_err(|error| format!("failed to query taxonomy parents {source}: {error}"))?;
        for row in rows {
            let (concept_id, parent_id) = row.map_err(|error| format!("failed to read taxonomy parent {source}: {error}"))?;
            parents.entry(concept_id).or_default().push(parent_id);
        }
    }
    let mut selectors = BTreeMap::<i64, BTreeMap<String, Vec<String>>>::new();
    for (system_id, table) in [("bisac", "bisac_selector"), ("ddc", "ddc_selector"), ("lcc", "lcc_selector")] {
        let mut statement = connection.prepare(&format!("SELECT concept_id,selector FROM {table} ORDER BY concept_id,selector")).map_err(|error| format!("failed to prepare taxonomy {system_id} selectors {source}: {error}"))?;
        let rows = statement.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))).map_err(|error| format!("failed to query taxonomy {system_id} selectors {source}: {error}"))?;
        for row in rows {
            let (concept_id, selector) = row.map_err(|error| format!("failed to read taxonomy {system_id} selector {source}: {error}"))?;
            selectors.entry(concept_id).or_default().entry(system_id.to_owned()).or_default().push(selector);
        }
    }
    {
        let query = if format_version == "7" {
            "SELECT concept_id,class_letters || start_value,class_letters || end_value FROM lcc_range ORDER BY concept_id,class_letters,start_value,end_value"
        } else { "SELECT concept_id,start_code,end_code FROM lcc_range ORDER BY concept_id,start_code,end_code" };
        let mut statement = connection.prepare(query).map_err(|e| e.to_string())?;
        let rows = statement.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))).map_err(|e| e.to_string())?;
        for row in rows {
            let (concept_id, start, end) = row.map_err(|e| e.to_string())?;
            selectors.entry(concept_id).or_default().entry("lcc".to_owned()).or_default().push(format!("{start}..{end}"));
        }
    }

    let mut labels = BTreeMap::<i64, Vec<String>>::new();
    {
        let mut statement = connection.prepare("SELECT concept_id,label FROM source_label ORDER BY concept_id,ordinal").map_err(|error| format!("failed to prepare taxonomy source labels {source}: {error}"))?;
        let rows = statement.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))).map_err(|error| format!("failed to query taxonomy source labels {source}: {error}"))?;
        for row in rows {
            let (concept_id, label) = row.map_err(|error| format!("failed to read taxonomy source label {source}: {error}"))?;
            labels.entry(concept_id).or_default().push(label);
        }
    }

    let mut statement = connection.prepare("SELECT concept_id,preferred_label FROM concept ORDER BY rowid").map_err(|error| format!("failed to prepare taxonomy concepts {source}: {error}"))?;
    let rows = statement.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))).map_err(|error| format!("failed to query taxonomy concepts {source}: {error}"))?;
    let mut concepts = Vec::new();
    for row in rows {
        let (concept_id, preferred_label) = row.map_err(|error| format!("failed to read taxonomy concept {source}: {error}"))?;
        let parent_ids = parents.remove(&concept_id).unwrap_or_default();
        concepts.push(UnifiedConceptDefinition::new(
            concept_id.clone(),
            preferred_label,
            parent_ids,
            selectors.remove(&concept_id).unwrap_or_default(),
            labels.remove(&concept_id).unwrap_or_default(),
        ));
    }
    Ok(concepts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selector_text_order_round_trips_without_changing_subject_order() {
        let mut connection = Connection::open_in_memory().unwrap();
        reset_unified_taxonomy_connection(&mut connection).unwrap();
        let parent = read_unified_taxonomy_connection(&connection, "test").unwrap()
            .into_iter().find(|concept| concept.parent_ids().is_empty()).unwrap().concept_id();
        let child = UnifiedConceptDefinition::new(
            999990, "Selector order test".into(), vec![parent],
            BTreeMap::from([("lcc".into(), vec!["QA9".into(), "QA1".into()])]), vec![],
        );
        merge_unified_taxonomy_connection(&mut connection, 1, &[child.clone()], &[], "test").unwrap();
        let restored = read_unified_taxonomy_connection(&connection, "test").unwrap()
            .into_iter().find(|concept| concept.concept_id() == child.concept_id()).unwrap();
        assert_eq!(restored, child);
        assert_eq!(restored.source_selectors("lcc"), &["QA1", "QA9"]);
        let display_columns: i64 = connection.query_row("SELECT count(*) FROM pragma_table_info('concept_parent') WHERE name='display_order'", [], |row| row.get(0)).unwrap();
        assert_eq!(display_columns, 0);
        let ordinal_columns: i64 = connection.query_row(
            "SELECT count(*) FROM pragma_table_info('lcc_selector') WHERE name='ordinal'", [], |row| row.get(0)
        ).unwrap();
        assert_eq!(ordinal_columns, 0);
    }

    #[test]
    fn taxonomy_relationships_store_integer_subject_ids() {
        let mut connection = Connection::open_in_memory().unwrap();
        reset_unified_taxonomy_connection(&mut connection).unwrap();
        for (table, column) in [("concept", "concept_id"), ("concept_parent", "concept_id"),
            ("concept_parent", "parent_concept_id"), ("lcc_selector", "concept_id"), ("bisac_selector", "concept_id"), ("ddc_selector", "concept_id"), ("source_label", "concept_id")] {
            let count: i64 = connection.query_row(&format!("SELECT count(*) FROM {table} WHERE typeof({column}) <> 'integer'"), [], |row| row.get(0)).unwrap();
            assert_eq!(count, 0, "{table}.{column}");
        }
        assert!(connection.execute("INSERT INTO concept(concept_id,preferred_label) VALUES('old:string-id','Invalid')", []).is_err());
    }

    #[test]
    fn lcc_ranges_are_structural_and_have_one_owner() {
        let mut connection = Connection::open_in_memory().unwrap();
        reset_unified_taxonomy_connection(&mut connection).unwrap();
        let encoded_ranges: i64 = connection.query_row(
            "SELECT count(*) FROM lcc_selector WHERE instr(selector,'..')>0",
            [], |row| row.get(0),
        ).unwrap();
        assert_eq!(encoded_ranges, 0);
        let (start_code,end_code,owner): (String,String,i64) = connection.query_row(
            "SELECT start_code,end_code,concept_id FROM lcc_range LIMIT 1",
            [], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?)),
        ).unwrap();
        let other = connection.query_row("SELECT concept_id FROM concept WHERE concept_id<>?1 LIMIT 1", [owner], |row| row.get::<_,i64>(0)).unwrap();
        assert!(connection.execute(
            "INSERT INTO lcc_range(start_code,end_code,concept_id) VALUES(?1,?2,?3)",
            params![start_code,end_code,other],
        ).is_err());
    }

    #[cfg(not(feature = "authoritative-taxonomy"))]
    #[test]
    fn embedded_client_taxonomy_is_the_bounded_second_level_seed() {
        let concepts = read_embedded_unified_taxonomy().unwrap();
        assert!((1_000..=2_000).contains(&concepts.len()));
        assert!(DEFAULT_UNIFIED_TAXONOMY_SQLITE.len() < 2 * 1024 * 1024);
    }

    #[cfg(feature = "authoritative-taxonomy")]
    #[test]
    fn authoritative_build_embeds_the_complete_taxonomy() {
        let concepts = read_embedded_unified_taxonomy().unwrap();
        assert!(concepts.len() > 2_000);
    }

    #[test]
    fn server_slice_is_persisted_as_an_ancestor_closed_local_overlay() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("taxonomy.sqlite3");
        std::fs::write(&path, DEFAULT_UNIFIED_TAXONOMY_SQLITE).unwrap();
        assert!(!has_unified_taxonomy_overlay(&path).unwrap());
        let existing = read_unified_taxonomy_sqlite(&path).unwrap();
        let humanities = existing.iter().find(|concept| concept.preferred_label() == "Humanities" && concept.parent_ids().is_empty()).unwrap();
        let fetched = UnifiedConceptDefinition::new(
            999999,
            "Server Detail".to_owned(),
            vec![humanities.concept_id().to_owned()],
            BTreeMap::from([("lcc".to_owned(), vec!["ZZ999..ZZ999@5".to_owned()])]),
            vec!["Server detail source caption".to_owned()],
        );
        merge_unified_taxonomy_sqlite(&path, 1, &[fetched]).unwrap();
        assert!(has_unified_taxonomy_overlay(&path).unwrap());
        let merged = read_unified_taxonomy_sqlite(&path).unwrap();
        let inserted = merged.iter().find(|concept| concept.concept_id() == 999999).unwrap();
        assert_eq!(inserted.parent_ids(), &[humanities.concept_id()]);
        assert_eq!(inserted.source_selectors("lcc"), &["ZZ999..ZZ999"]);
        let connection = Connection::open(path).unwrap();
        assert_eq!(unified_taxonomy_server_release_id_in(&connection, "test").unwrap(), Some(1));
    }

    #[test]
    fn ambiguous_lcc_server_resolution_is_rejected_atomically() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("taxonomy.sqlite3");
        std::fs::write(&path, DEFAULT_UNIFIED_TAXONOMY_SQLITE).unwrap();
        let concepts = read_unified_taxonomy_sqlite(&path).unwrap();
        let ids = concepts.iter().take(2).map(|concept| concept.concept_id().to_owned()).collect();
        let resolution = UnifiedCodeResolution::new("lcc".to_owned(), "HD69.C6".to_owned(), ids);
        let error = merge_unified_taxonomy_sqlite_with_resolutions(&path, 1, &[], &[resolution]).unwrap_err();
        assert!(error.contains("ambiguous LCC resolution"));
        assert!(read_resolved_unified_taxonomy_codes(&path, &[("lcc".into(), "HD69.C6".into())]).unwrap().is_empty());
    }

    #[test]
    fn exact_and_negative_server_resolutions_are_persisted() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("taxonomy.sqlite3");
        std::fs::write(&path, DEFAULT_UNIFIED_TAXONOMY_SQLITE).unwrap();
        let concepts = read_unified_taxonomy_sqlite(&path).unwrap();
        let matched = concepts.iter().find(|concept| !concept.parent_ids().is_empty()).unwrap();
        let resolutions = vec![UnifiedCodeResolution::new("lcc".to_owned(), "HF5601".to_owned(), vec![matched.concept_id().to_owned()]), UnifiedCodeResolution::new("lcc".to_owned(), "ZZZZ999".to_owned(), Vec::new())];
        merge_unified_taxonomy_sqlite_with_resolutions(&path, 1, &[], &resolutions).unwrap();

        let candidates = vec![("lcc".to_owned(), "HF5601".to_owned()), ("lcc".to_owned(), "ZZZZ999".to_owned()), ("bisac".to_owned(), "SCI000000".to_owned())];
        let resolved = read_resolved_unified_taxonomy_codes(&path, &candidates).unwrap();
        assert!(resolved.contains(&("lcc".to_owned(), "HF5601".to_owned())));
        assert!(resolved.contains(&("lcc".to_owned(), "ZZZZ999".to_owned())));
        assert!(!resolved.contains(&("bisac".to_owned(), "SCI000000".to_owned())));
    }

    #[test]
    fn resolutions_from_an_obsolete_server_release_are_invalidated() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("taxonomy.sqlite3");
        std::fs::write(&path, DEFAULT_UNIFIED_TAXONOMY_SQLITE).unwrap();
        merge_unified_taxonomy_sqlite_with_resolutions(&path, 1, &[], &[UnifiedCodeResolution::new("lcc".to_owned(), "ZZZZ999".to_owned(), Vec::new())]).unwrap();
        merge_unified_taxonomy_sqlite_with_resolutions(&path, 2, &[], &[UnifiedCodeResolution::new("bisac".to_owned(), "SCI000000".to_owned(), Vec::new())]).unwrap();

        let candidates = vec![("lcc".to_owned(), "ZZZZ999".to_owned()), ("bisac".to_owned(), "SCI000000".to_owned())];
        let resolved = read_resolved_unified_taxonomy_codes(&path, &candidates).unwrap();
        assert!(!resolved.contains(&("lcc".to_owned(), "ZZZZ999".to_owned())));
        assert!(resolved.contains(&("bisac".to_owned(), "SCI000000".to_owned())));
    }
    #[test]
    fn version_seven_overlay_upgrades_and_round_trips_complete_endpoints() {
        let mut db = Connection::open_in_memory().unwrap();
        reset_unified_taxonomy_connection(&mut db).unwrap();
        // A minimal v7 range table exercises the device cache migration.
        db.execute_batch("DROP TABLE lcc_range;
            CREATE TABLE lcc_range(class_letters TEXT,start_value TEXT,end_value TEXT,concept_id INTEGER REFERENCES concept(concept_id),PRIMARY KEY(class_letters,start_value,end_value));
            UPDATE taxonomy_meta SET value='7' WHERE key='format_version';").unwrap();
        let parent = read_unified_taxonomy_connection(&db,"v7").unwrap().into_iter().find(|c| c.parent_ids().is_empty()).unwrap().concept_id();
        db.execute("INSERT INTO lcc_range VALUES('QA','1','10',?1)",[parent]).unwrap();
        let before = read_unified_taxonomy_connection(&db,"v7").unwrap();
        let child = UnifiedConceptDefinition::new(999980,"Cross subclass test".into(),vec![parent],
            BTreeMap::from([("lcc".into(),vec!["D1..DX301".into(),"R..RZ".into(),"QB1.A1..QB1.C5".into()])]),vec![]);
        merge_unified_taxonomy_connection(&mut db, 99, &[child.clone()], &[], "v7").unwrap();
        let after = read_unified_taxonomy_connection(&db,"v8").unwrap();
        for original in before { assert!(after.contains(&original)); }
        assert!(after.contains(&child));
        assert_eq!(db.query_row("SELECT value FROM taxonomy_meta WHERE key='format_version'",[],|r|r.get::<_,String>(0)).unwrap(),"8");
        assert_eq!(db.query_row("SELECT count(*) FROM pragma_foreign_key_check",[],|r|r.get::<_,i64>(0)).unwrap(),0);
    }

}
