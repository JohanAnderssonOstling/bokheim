use crate::lcc::lcc_endpoint;
#[cfg(not(target_arch = "wasm32"))]
use crate::DEFAULT_UNIFIED_TAXONOMY_SQLITE;
use crate::{UnifiedCodeResolution, UnifiedConceptDefinition};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use std::collections::BTreeSet;
use std::path::Path;

fn split_lcc_range(selector: &str) -> Result<(&str, &str), String> {
    let (start, end) = selector.split_once("..").ok_or_else(|| format!("invalid LCC range {selector}"))?;
    let start_ep = lcc_endpoint(start, false).ok_or_else(|| format!("invalid LCC range start {start}"))?;
    let end_ep = lcc_endpoint(end, true).ok_or_else(|| format!("invalid LCC range end {end}"))?;
    if crate::lcc::compare_lcc_key(&start_ep.letters, start_ep.number, &end_ep.letters, end_ep.number).is_gt() {
        return Err(format!("invalid LCC range {selector}"));
    }
    Ok((start, end))
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn read_embedded_unified_taxonomy() -> Result<Vec<UnifiedConceptDefinition>, String> {
    let mut connection = Connection::open_in_memory().map_err(|error| format!("failed to open embedded taxonomy database: {error}"))?;
    connection.deserialize_bytes("main", DEFAULT_UNIFIED_TAXONOMY_SQLITE).map_err(|error| format!("failed to open embedded taxonomy database image: {error}"))?;
    read_unified_taxonomy_connection(&connection, "embedded unified taxonomy")
}

pub fn read_unified_taxonomy_sqlite(path: &Path) -> Result<Vec<UnifiedConceptDefinition>, String> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|error| format!("failed to open unified taxonomy database {}: {error}", path.display()))?;
    read_unified_taxonomy_connection(&connection, &path.display().to_string())
}

#[cfg(not(target_arch = "wasm32"))]
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
    if format_version != "1" {
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
    let value: String = connection.query_row("SELECT value FROM taxonomy_meta WHERE key='release_id'", [], |row| row.get(0)).map_err(|error| format!("failed to read taxonomy release from {}: {error}", path.display()))?;
    let release_id = value.parse::<u64>().map_err(|error| format!("invalid taxonomy release {value:?} in {}: {error}", path.display()))?;
    if release_id == 0 {
        return Err(format!("invalid taxonomy release 0 in {}", path.display()));
    }
    Ok(release_id)
}

pub fn unified_taxonomy_server_release_id_in(connection: &Connection, source: &str) -> Result<Option<u64>, String> {
    let value = connection.query_row("SELECT value FROM taxonomy_meta WHERE key='server_release_id'", [], |row| row.get::<_, String>(0)).optional().map_err(|error| format!("failed to read taxonomy server release {source}: {error}"))?;
    value.map(|value| value.parse::<u64>().map_err(|error| format!("invalid taxonomy server release {value:?} in {source}: {error}"))).transpose()
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
    let format_version = connection.query_row("SELECT value FROM taxonomy_meta WHERE key='format_version'", [], |row| row.get::<_, String>(0)).map_err(|error| format!("failed to read local taxonomy format {source}: {error}"))?;
    if format_version != "1" {
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
                "INSERT INTO concept(concept_id,preferred_label,normalized_label) VALUES(?1,?2,?3)
                 ON CONFLICT(concept_id) DO UPDATE SET preferred_label=excluded.preferred_label,normalized_label=excluded.normalized_label",
                params![concept.concept_id(), concept.preferred_label(), super::runtime_index::normalize_source_label(concept.preferred_label())],
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
        for (ordinal, parent) in concept.parent_ids().iter().enumerate() {
            transaction
                .execute("INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(?1,?2,?3)", params![concept.concept_id(), parent, ordinal as i64])
                .map_err(|error| format!("failed to merge taxonomy parent for {}: {error}", concept.concept_id()))?;
        }
        for system in ["bisac", "ddc", "lcc"] {
            for selector in concept.source_selectors(system) {
                if system == "lcc" && selector.contains("..") {
                    let (start_code, end_code) = split_lcc_range(selector)?;
                    super::range_storage::insert_range(&transaction, concept.concept_id(), start_code, end_code)?;
                } else {
                    let table = match system {
                        "lcc" => "lcc_selector",
                        "ddc" => "ddc_selector",
                        _ => "bisac_selector",
                    };
                    transaction
                        .execute(&format!("INSERT INTO {table}(concept_id,selector) VALUES(?1,?2)"), params![concept.concept_id(), selector])
                        .map_err(|error| format!("failed to merge taxonomy selector for {}: {error}", concept.concept_id()))?;
                }
            }
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

pub use super::taxonomy_reader::read_unified_taxonomy_connection;

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;


    #[test]
    fn server_slice_is_persisted_as_an_ancestor_closed_local_overlay() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("taxonomy.sqlite3");
        std::fs::write(&path, DEFAULT_UNIFIED_TAXONOMY_SQLITE).unwrap();
        assert!(!has_unified_taxonomy_overlay(&path).unwrap());
        let existing = read_unified_taxonomy_sqlite(&path).unwrap();
        // Persistence is independent of the curated root names.
        let parent = existing.iter().find(|concept| concept.parent_ids().is_empty()).unwrap();
        let fetched = UnifiedConceptDefinition::new(999999, "Server Detail".to_owned(), vec![parent.concept_id()], BTreeMap::from([("lcc".to_owned(), vec!["ZZ999..ZZ999".to_owned()])]));
        merge_unified_taxonomy_sqlite(&path, 1, &[fetched]).unwrap();
        assert!(has_unified_taxonomy_overlay(&path).unwrap());
        let merged = read_unified_taxonomy_sqlite(&path).unwrap();
        let inserted = merged.iter().find(|concept| concept.concept_id() == 999999).unwrap();
        assert_eq!(inserted.parent_ids(), &[parent.concept_id()]);
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
}
