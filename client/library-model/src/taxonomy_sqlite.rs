use crate::subjects::{UnifiedConceptDefinition, DEFAULT_UNIFIED_TAXONOMY_SQLITE};
use rusqlite::{params, Connection, OpenFlags};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub(crate) fn read_embedded_unified_taxonomy() -> Result<Vec<UnifiedConceptDefinition>, String> {
    let mut connection = Connection::open_in_memory().map_err(|error| format!("failed to open embedded taxonomy database: {error}"))?;
    connection.deserialize_bytes("main", DEFAULT_UNIFIED_TAXONOMY_SQLITE).map_err(|error| format!("failed to open embedded taxonomy database image: {error}"))?;
    read_connection(&connection, "embedded unified taxonomy")
}

pub fn read_unified_taxonomy_sqlite(path: &Path) -> Result<Vec<UnifiedConceptDefinition>, String> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|error| format!("failed to open unified taxonomy database {}: {error}", path.display()))?;
    read_connection(&connection, &path.display().to_string())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnifiedCodeResolution {
    system_id: String,
    code: String,
    matched_concept_ids: Vec<String>,
}

impl UnifiedCodeResolution {
    pub fn new(system_id: String, code: String, matched_concept_ids: Vec<String>) -> Self {
        Self { system_id, code, matched_concept_ids }
    }

    pub fn system_id(&self) -> &str {
        &self.system_id
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn matched_concept_ids(&self) -> &[String] {
        &self.matched_concept_ids
    }
}

/// Returns precise system/code pairs previously resolved by the taxonomy
/// server. A row with no matched concepts is still a completed resolution.
pub fn read_resolved_unified_taxonomy_codes(path: &Path, candidates: &[(String, String)]) -> Result<BTreeSet<(String, String)>, String> {
    if candidates.is_empty() {
        return Ok(BTreeSet::new());
    }
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|error| format!("failed to open local unified taxonomy database {}: {error}", path.display()))?;
    let exists = connection
        .query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='taxonomy_code_resolution')", [], |row| row.get::<_, bool>(0))
        .map_err(|error| format!("failed to inspect local taxonomy resolution ledger {}: {error}", path.display()))?;
    if !exists {
        return Ok(BTreeSet::new());
    }
    let mut statement =
        connection.prepare("SELECT EXISTS(SELECT 1 FROM taxonomy_code_resolution WHERE system_id=?1 AND code=?2)").map_err(|error| format!("failed to prepare local taxonomy resolution lookup {}: {error}", path.display()))?;
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
pub fn merge_unified_taxonomy_sqlite(path: &Path, server_release_id: &str, concepts: &[UnifiedConceptDefinition]) -> Result<(), String> {
    merge_unified_taxonomy_sqlite_with_resolutions(path, server_release_id, concepts, &[])
}

pub fn merge_unified_taxonomy_sqlite_with_resolutions(path: &Path, server_release_id: &str, concepts: &[UnifiedConceptDefinition], resolutions: &[UnifiedCodeResolution]) -> Result<(), String> {
    let mut connection = Connection::open(path).map_err(|error| format!("failed to open local unified taxonomy database {}: {error}", path.display()))?;
    connection.pragma_update(None, "foreign_keys", true).map_err(|error| format!("failed to enable local taxonomy constraints {}: {error}", path.display()))?;
    let format_version = connection.query_row("SELECT value FROM taxonomy_meta WHERE key='format_version'", [], |row| row.get::<_, String>(0)).map_err(|error| format!("failed to read local taxonomy format {}: {error}", path.display()))?;
    if format_version != "2" {
        return Err(format!("unsupported local unified taxonomy format {format_version} in {}", path.display()));
    }
    let transaction = connection.transaction().map_err(|error| format!("failed to start local taxonomy merge {}: {error}", path.display()))?;
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
                 concept_id TEXT NOT NULL REFERENCES concept(concept_id),
                 ordinal INTEGER NOT NULL,
                 PRIMARY KEY(system_id,code,concept_id),
                 FOREIGN KEY(system_id,code) REFERENCES taxonomy_code_resolution(system_id,code) ON DELETE CASCADE
             );",
        )
        .map_err(|error| format!("failed to initialize local taxonomy resolution ledger {}: {error}", path.display()))?;
    let has_stale_resolutions = transaction
        .query_row("SELECT EXISTS(SELECT 1 FROM taxonomy_code_resolution WHERE server_release_id<>?1)", [server_release_id], |row| row.get::<_, bool>(0))
        .map_err(|error| format!("failed to inspect taxonomy resolution releases {}: {error}", path.display()))?;
    if has_stale_resolutions {
        transaction.execute("DELETE FROM taxonomy_code_resolution", []).map_err(|error| format!("failed to invalidate stale taxonomy resolutions {}: {error}", path.display()))?;
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
        if !matches!(resolution.system_id(), "bisac" | "ddc" | "lcc") || resolution.code().trim().is_empty() {
            return Err(format!("invalid taxonomy code resolution {}:{}", resolution.system_id(), resolution.code()));
        }
        transaction
            .execute(
                "INSERT INTO taxonomy_code_resolution(system_id,code,server_release_id) VALUES(?1,?2,?3)
                 ON CONFLICT(system_id,code) DO UPDATE SET server_release_id=excluded.server_release_id",
                params![resolution.system_id(), resolution.code(), server_release_id],
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
        transaction.execute("DELETE FROM source_selector WHERE concept_id=?1", [concept.concept_id()]).map_err(|error| format!("failed to replace taxonomy selectors for {}: {error}", concept.concept_id()))?;
        transaction.execute("DELETE FROM source_label WHERE concept_id=?1", [concept.concept_id()]).map_err(|error| format!("failed to replace taxonomy labels for {}: {error}", concept.concept_id()))?;
        for (ordinal, (parent, display_order)) in concept.parent_ids().iter().zip(concept.parent_sort_orders()).enumerate() {
            transaction
                .execute("INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES(?1,?2,?3,?4)", params![concept.concept_id(), parent, ordinal as i64, display_order])
                .map_err(|error| format!("failed to merge taxonomy parent for {}: {error}", concept.concept_id()))?;
        }
        for system in ["bisac", "ddc", "lcc"] {
            for (ordinal, selector) in concept.source_selectors(system).iter().enumerate() {
                transaction
                    .execute("INSERT INTO source_selector(concept_id,system_id,selector,ordinal) VALUES(?1,?2,?3,?4)", params![concept.concept_id(), system, selector, ordinal as i64])
                    .map_err(|error| format!("failed to merge taxonomy selector for {}: {error}", concept.concept_id()))?;
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
            [server_release_id],
        )
        .map_err(|error| format!("failed to record taxonomy server release: {error}"))?;
    transaction.commit().map_err(|error| format!("failed to commit local taxonomy merge {}: {error}", path.display()))?;
    let violations = connection.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| row.get::<_, i64>(0)).map_err(|error| format!("failed to verify local taxonomy merge {}: {error}", path.display()))?;
    if violations != 0 {
        return Err(format!("local taxonomy merge left {violations} foreign-key violations in {}", path.display()));
    }
    Ok(())
}

fn read_connection(connection: &Connection, source: &str) -> Result<Vec<UnifiedConceptDefinition>, String> {
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
    if format_version != "2" {
        return Err(format!("unsupported unified taxonomy database format {format_version} in {source}"));
    }

    let mut parents = BTreeMap::<String, Vec<(String, i64)>>::new();
    {
        let mut statement = connection.prepare("SELECT concept_id,parent_concept_id,display_order FROM concept_parent ORDER BY concept_id,ordinal").map_err(|error| format!("failed to prepare taxonomy parents {source}: {error}"))?;
        let rows = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?))).map_err(|error| format!("failed to query taxonomy parents {source}: {error}"))?;
        for row in rows {
            let (concept_id, parent_id, display_order) = row.map_err(|error| format!("failed to read taxonomy parent {source}: {error}"))?;
            parents.entry(concept_id).or_default().push((parent_id, display_order));
        }
    }

    let mut selectors = BTreeMap::<String, BTreeMap<String, Vec<String>>>::new();
    {
        let mut statement = connection.prepare("SELECT concept_id,system_id,selector FROM source_selector ORDER BY concept_id,system_id,ordinal").map_err(|error| format!("failed to prepare taxonomy selectors {source}: {error}"))?;
        let rows = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))).map_err(|error| format!("failed to query taxonomy selectors {source}: {error}"))?;
        for row in rows {
            let (concept_id, system_id, selector) = row.map_err(|error| format!("failed to read taxonomy selector {source}: {error}"))?;
            selectors.entry(concept_id).or_default().entry(system_id).or_default().push(selector);
        }
    }

    let mut labels = BTreeMap::<String, Vec<String>>::new();
    {
        let mut statement = connection.prepare("SELECT concept_id,label FROM source_label ORDER BY concept_id,ordinal").map_err(|error| format!("failed to prepare taxonomy source labels {source}: {error}"))?;
        let rows = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))).map_err(|error| format!("failed to query taxonomy source labels {source}: {error}"))?;
        for row in rows {
            let (concept_id, label) = row.map_err(|error| format!("failed to read taxonomy source label {source}: {error}"))?;
            labels.entry(concept_id).or_default().push(label);
        }
    }

    let mut statement = connection.prepare("SELECT concept_id,preferred_label FROM concept ORDER BY rowid").map_err(|error| format!("failed to prepare taxonomy concepts {source}: {error}"))?;
    let rows = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))).map_err(|error| format!("failed to query taxonomy concepts {source}: {error}"))?;
    let mut concepts = Vec::new();
    for row in rows {
        let (concept_id, preferred_label) = row.map_err(|error| format!("failed to read taxonomy concept {source}: {error}"))?;
        let (parent_ids, parent_sort_orders) = parents.remove(&concept_id).unwrap_or_default().into_iter().unzip();
        concepts.push(UnifiedConceptDefinition::new_with_parent_sort_orders(
            concept_id.clone(),
            preferred_label,
            parent_ids,
            parent_sort_orders,
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
    fn server_slice_is_persisted_as_an_ancestor_closed_local_overlay() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("taxonomy.sqlite3");
        std::fs::write(&path, DEFAULT_UNIFIED_TAXONOMY_SQLITE).unwrap();
        let existing = read_unified_taxonomy_sqlite(&path).unwrap();
        let humanities = existing.iter().find(|concept| concept.preferred_label() == "Humanities" && concept.parent_ids().is_empty()).unwrap();
        let fetched = UnifiedConceptDefinition::new(
            "test:server-detail".to_owned(),
            "Server Detail".to_owned(),
            vec![humanities.concept_id().to_owned()],
            BTreeMap::from([("lcc".to_owned(), vec!["ZZ999..ZZ999@5".to_owned()])]),
            vec!["Server detail source caption".to_owned()],
        );
        merge_unified_taxonomy_sqlite(&path, "server-release", &[fetched]).unwrap();
        let merged = read_unified_taxonomy_sqlite(&path).unwrap();
        let inserted = merged.iter().find(|concept| concept.concept_id() == "test:server-detail").unwrap();
        assert_eq!(inserted.parent_ids(), &[humanities.concept_id()]);
        assert_eq!(inserted.source_selectors("lcc"), &["ZZ999..ZZ999@5"]);
        let connection = Connection::open(path).unwrap();
        assert_eq!(connection.query_row("SELECT value FROM taxonomy_meta WHERE key='server_release_id'", [], |row| row.get::<_, String>(0)).unwrap(), "server-release");
    }

    #[test]
    fn exact_and_negative_server_resolutions_are_persisted() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("taxonomy.sqlite3");
        std::fs::write(&path, DEFAULT_UNIFIED_TAXONOMY_SQLITE).unwrap();
        let concepts = read_unified_taxonomy_sqlite(&path).unwrap();
        let matched = concepts.iter().find(|concept| !concept.parent_ids().is_empty()).unwrap();
        let resolutions = vec![UnifiedCodeResolution::new("lcc".to_owned(), "HF5601".to_owned(), vec![matched.concept_id().to_owned()]), UnifiedCodeResolution::new("lcc".to_owned(), "ZZZZ999".to_owned(), Vec::new())];
        merge_unified_taxonomy_sqlite_with_resolutions(&path, "server-release", &[], &resolutions).unwrap();

        let candidates = vec![("lcc".to_owned(), "HF5601".to_owned()), ("lcc".to_owned(), "ZZZZ999".to_owned()), ("ddc".to_owned(), "515".to_owned())];
        let resolved = read_resolved_unified_taxonomy_codes(&path, &candidates).unwrap();
        assert!(resolved.contains(&("lcc".to_owned(), "HF5601".to_owned())));
        assert!(resolved.contains(&("lcc".to_owned(), "ZZZZ999".to_owned())));
        assert!(!resolved.contains(&("ddc".to_owned(), "515".to_owned())));
    }

    #[test]
    fn resolutions_from_an_obsolete_server_release_are_invalidated() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("taxonomy.sqlite3");
        std::fs::write(&path, DEFAULT_UNIFIED_TAXONOMY_SQLITE).unwrap();
        merge_unified_taxonomy_sqlite_with_resolutions(&path, "old-release", &[], &[UnifiedCodeResolution::new("lcc".to_owned(), "ZZZZ999".to_owned(), Vec::new())]).unwrap();
        merge_unified_taxonomy_sqlite_with_resolutions(&path, "new-release", &[], &[UnifiedCodeResolution::new("ddc".to_owned(), "515".to_owned(), Vec::new())]).unwrap();

        let candidates = vec![("lcc".to_owned(), "ZZZZ999".to_owned()), ("ddc".to_owned(), "515".to_owned())];
        let resolved = read_resolved_unified_taxonomy_codes(&path, &candidates).unwrap();
        assert!(!resolved.contains(&("lcc".to_owned(), "ZZZZ999".to_owned())));
        assert!(resolved.contains(&("ddc".to_owned(), "515".to_owned())));
    }
}
