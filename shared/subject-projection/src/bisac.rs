use super::bisac_database::with_reference;
use super::{canonical_subject_system, has_usable_lcc_classification, normalize_path, normalize_term, split_path, BISAC_SYSTEM_ID, UNIFIED_SYSTEM_ID};
use book_model::BookSubject;
use rusqlite::{Connection, OptionalExtension, Row};
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet, HashMap};

struct SubjectNode {
    path: String,
    parent_path: Option<String>,
    name: String,
    code: Option<String>,
    normalized_components: Vec<String>,
}

impl SubjectNode {
    fn parent_path(&self) -> Option<&str> {
        self.parent_path.as_deref()
    }
}

struct Reference<'a> {
    database: Option<&'a Connection>,
    failed: Cell<bool>,
}

struct BundledNode {
    path: &'static str,
    code: Option<&'static str>,
    normalized_path: &'static str,
    tail_component: &'static str,
    component_count: usize,
}

const BUNDLED_NODES: &[BundledNode] = include!(concat!(env!("OUT_DIR"), "/bisac-reference-nodes.rs"));

impl BundledNode {
    fn node(&self) -> SubjectNode {
        node(self.path.to_owned(), self.code.map(str::to_owned))
    }
}

fn read_node(row: &Row<'_>) -> rusqlite::Result<SubjectNode> {
    let path: String = row.get(0)?;
    Ok(node(path, row.get(1)?))
}

fn node(path: String, code: Option<String>) -> SubjectNode {
    // The compiler joins CSV components with this exact separator. Recovering
    // the parent and last label avoids storing both again for every node.
    let (parent_path, name) = match path.rsplit_once(" / ") {
        Some((parent, name)) => (Some(parent.to_owned()), name.to_owned()),
        None => (None, path.clone()),
    };
    SubjectNode { path, parent_path, name, code, normalized_components: Vec::new() }
}

impl<'a> Reference<'a> {
    fn new(database: Option<&'a Connection>) -> Self {
        Self { database, failed: Cell::new(false) }
    }

    fn query<T>(&self, read: impl FnOnce(&Connection) -> Result<T, String>) -> Option<T> {
        let db = self.database.filter(|_| !self.failed.get())?;
        match read(db) {
            Ok(value) => Some(value),
            Err(error) => {
                // Finish this lookup with the identical compiled reference.
                // Do not treat a broken cache as missing subject evidence.
                self.failed.set(true);
                eprintln!("BISAC cache query failed; using compiled reference nodes: {error}");
                None
            }
        }
    }

    fn node_by_path(&self, path: &str) -> Option<SubjectNode> {
        let path = normalize_path(path);
        self.query(|db| db.prepare_cached("SELECT path,code FROM bisac_node WHERE normalized_path=?1 ORDER BY id DESC LIMIT 1").and_then(|mut query| query.query_row([&path], read_node).optional()).map_err(|error| error.to_string()))
            .unwrap_or_else(|| BUNDLED_NODES.iter().rev().find(|node| node.normalized_path == path.as_str()).map(BundledNode::node))
    }

    fn node_by_code(&self, code: &str) -> Option<SubjectNode> {
        let code = code.trim().to_ascii_uppercase();
        self.query(|db| db.prepare_cached("SELECT path,code FROM bisac_node WHERE code=?1 ORDER BY id DESC LIMIT 1").and_then(|mut query| query.query_row([&code], read_node).optional()).map_err(|error| error.to_string()))
            .unwrap_or_else(|| BUNDLED_NODES.iter().rev().find(|node| node.code == Some(code.as_str())).map(BundledNode::node))
    }

    fn candidates(&self, components: &HashMap<String, BTreeSet<usize>>, minimum_components: usize) -> Vec<SubjectNode> {
        if components.is_empty() {
            return Vec::new();
        }
        self.query(|db| {
            let keys = serde_json::to_string(&components.keys().collect::<Vec<_>>()).map_err(|error| error.to_string())?;
            let mut query = db
                .prepare_cached(
                    "SELECT n.path,n.code
                 FROM bisac_node n
                 WHERE n.id IN (SELECT n.id FROM json_each(?1) selected JOIN bisac_node n ON n.tail_component=selected.value)
                   AND n.component_count>=?2 ORDER BY n.id",
                )
                .map_err(|error| error.to_string())?;
            let rows = query
                .query_map(rusqlite::params![keys, i64::try_from(minimum_components).unwrap_or(i64::MAX)], |row| {
                    let mut node = read_node(row)?;
                    node.normalized_components = node.path.split(" / ").map(normalize_term).collect();
                    Ok(node)
                })
                .map_err(|error| error.to_string())?;
            rows.collect::<rusqlite::Result<Vec<_>>>().map_err(|error| error.to_string())
        })
        .unwrap_or_else(|| {
            BUNDLED_NODES
                .iter()
                .filter(|node| node.component_count >= minimum_components && components.contains_key(node.tail_component))
                .map(|bundled| {
                    let mut node = bundled.node();
                    node.normalized_components = node.path.split(" / ").map(normalize_term).collect();
                    node
                })
                .collect()
        })
    }
}

pub fn bisac_code_for_path(path: &str) -> Option<String> {
    with_reference(|db| {
        let reference = Reference::new(db);
        reference.node_by_path(path).and_then(|node| node.code).or_else(|| reference.node_by_path(&format!("{path} / General")).and_then(|node| node.code))
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubjectAssignment {
    node_path: String,
    evidence_positions: Vec<usize>,
}

impl SubjectAssignment {
    pub fn node_path(&self) -> &str {
        &self.node_path
    }
    pub fn evidence_positions(&self) -> &[usize] {
        &self.evidence_positions
    }
}

/// Matches actual BISAC paths whose every component is supported by the
/// supplied subjects. Separate labels may supply different components;
/// structured LCSH headings must match independently as complete paths.
pub fn match_bisac_subjects(subjects: &[BookSubject]) -> Vec<SubjectAssignment> {
    match_bisac_subjects_with_minimum_adjacent(subjects, 0)
}

/// Requires every component of an actual BISAC path, and at least
/// `minimum_components` components. Explicit codes bypass the length requirement.
pub fn match_bisac_subjects_with_minimum_adjacent(subjects: &[BookSubject], minimum_components: usize) -> Vec<SubjectAssignment> {
    let preferred_lcc = has_usable_lcc_classification(subjects);
    with_reference(|db| {
        let system = Reference::new(db);
        let mut matches = BTreeMap::<String, BTreeSet<usize>>::new();
        let mut component_evidence = HashMap::<String, BTreeSet<usize>>::new();
        for (position, subject) in subjects.iter().enumerate() {
            if preferred_lcc {
                // Explicit curator choices remain effective. Every automatic
                // BISAC code and inferred heading is fallback evidence.
                if subject.source().starts_with("manual:") && subject.authority().and_then(canonical_subject_system) == Some(BISAC_SYSTEM_ID) {
                    if let Some(node) = subject.code().and_then(|code| system.node_by_code(code)) {
                        matches.entry(node.path.clone()).or_default().insert(position);
                    }
                }
                continue;
            }
            let authority = subject.authority().map(str::trim).filter(|authority| !authority.is_empty());
            // Unified assignments and exclusions already name exact navigation
            // concepts. Their display labels are not independent BISAC evidence.
            if authority.is_some_and(|authority| authority.eq_ignore_ascii_case(UNIFIED_SYSTEM_ID)) {
                continue;
            }
            let authority_system = authority.and_then(canonical_subject_system);
            let lcsh_style = authority_system == Some("lcsh") || (subject.source().trim().eq_ignore_ascii_case("dc:subject") && subject.name().contains("--"));
            if authority_system.is_some() && !matches!(authority_system, Some(BISAC_SYSTEM_ID | "lcsh")) {
                continue;
            }
            if split_path(subject.name()).len() >= minimum_components {
                if let Some(node) = system.node_by_path(subject.name()) {
                    matches.entry(node.path.clone()).or_default().insert(position);
                }
            }
            if !lcsh_style {
                for component in split_path(subject.name()) {
                    component_evidence.entry(normalize_term(&component)).or_default().insert(position);
                }
                let declared_code = subject.code().or_else(|| (authority.is_none() || authority_system == Some(BISAC_SYSTEM_ID)).then_some(subject.name()));
                if let Some(node) = declared_code.and_then(|code| system.node_by_code(code)) {
                    matches.entry(node.path.clone()).or_default().insert(position);
                }
            }
        }
        for node in system.candidates(&component_evidence, minimum_components) {
            if node.normalized_components.len() < minimum_components {
                continue;
            }
            let mut evidence = BTreeSet::<usize>::new();
            if node.normalized_components.iter().all(|component| {
                component_evidence.get(component).is_some_and(|positions| {
                    evidence.extend(positions);
                    true
                })
            }) {
                matches.entry(node.path.clone()).or_default().extend(evidence);
            }
        }
        let candidate_paths = matches.keys().cloned().collect::<Vec<_>>();
        matches
            .into_iter()
            .filter(|(path, _)| !candidate_paths.iter().any(|candidate| candidate.starts_with(&format!("{path} / "))))
            .filter(|(path, _)| {
                let Some(node) = system.node_by_path(path) else { return true };
                if !node.name.eq_ignore_ascii_case("general") {
                    return true;
                }
                let Some(parent) = node.parent_path() else { return true };
                !candidate_paths.iter().any(|candidate| candidate != path && candidate.starts_with(&format!("{parent} / ")))
            })
            .map(|(node_path, evidence)| SubjectAssignment { node_path, evidence_positions: evidence.into_iter().collect() })
            .collect()
    })
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod compact_reference_tests {
    use super::*;

    #[test]
    fn failed_cache_queries_keep_bundled_subject_evidence() {
        let db = Connection::open_in_memory().unwrap();
        let reference = Reference::new(Some(&db));
        // There is no bisac_node table: preparation fails before any result.
        let node = reference.node_by_code(" sci057000 ").unwrap();
        assert_eq!(node.path, "Science / Physics / Quantum Theory");
        assert!(reference.failed.get());
        assert_eq!(reference.node_by_path("science > physics > quantum theory").unwrap().code.as_deref(), Some("SCI057000"));
        let evidence = HashMap::from([("quantum theory".to_owned(), BTreeSet::from([0]))]);
        assert!(reference.candidates(&evidence, 3).iter().any(|candidate| candidate.path == node.path));
        assert!(reference.candidates(&evidence, 4).is_empty());
    }

    #[test]
    fn missing_storage_uses_bundled_reference_without_initialization() {
        let reference = Reference::new(None);
        assert_eq!(reference.node_by_code("SCI057000").unwrap().path, "Science / Physics / Quantum Theory");
        assert!(reference.node_by_code("SCI999999").is_none());
        assert!(reference.node_by_path("not a reference subject").is_none());
    }

    #[test]
    fn unreadable_candidate_rows_retry_the_complete_bundled_lookup() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE bisac_node(id INTEGER PRIMARY KEY,path,code,tail_component,component_count);
                          INSERT INTO bisac_node VALUES(1,NULL,'SCI057000','quantum theory',3);",
        )
        .unwrap();
        let reference = Reference::new(Some(&db));
        let evidence = HashMap::from([("quantum theory".to_owned(), BTreeSet::from([0]))]);
        let candidates = reference.candidates(&evidence, 3);
        assert!(reference.failed.get());
        assert!(candidates.iter().any(|node| node.code.as_deref() == Some("SCI057000") && node.path == "Science / Physics / Quantum Theory"));
    }

    fn database() -> Connection {
        let file = tempfile::NamedTempFile::new().unwrap();
        subject_taxonomy::bisac_reference::compile(file.path(), "code,description\nAAA000000,Topic / GENERAL\nAAA000000,topic / General\nBBB000000,Unicode / Éducation\nCCC000000,Graphic Novels / Horror--Zombies\n", "test").unwrap();
        // Keep the test independent of process-wide reference activation.
        let disk = Connection::open(file.path()).unwrap();
        let mut memory = Connection::open_in_memory().unwrap();
        let backup = rusqlite::backup::Backup::new(&disk, &mut memory).unwrap();
        backup.run_to_completion(64, std::time::Duration::ZERO, None).unwrap();
        drop(backup);
        memory
    }

    #[test]
    fn duplicate_normalized_paths_and_codes_keep_path_order() {
        let db = database();
        let reference = Reference::new(Some(&db));
        assert_eq!(reference.node_by_path("TOPIC > GENERAL").unwrap().path, "topic / General");
        assert_eq!(reference.node_by_code(" aaa000000 ").unwrap().path, "topic / General");
    }

    #[test]
    fn candidate_components_preserve_unicode_and_literal_separators() {
        let db = database();
        let reference = Reference::new(Some(&db));
        let evidence = HashMap::from([("unicode".to_owned(), BTreeSet::from([0])), ("éducation".to_owned(), BTreeSet::from([1])), ("graphic novels".to_owned(), BTreeSet::from([2])), ("horror--zombies".to_owned(), BTreeSet::from([3]))]);
        let candidates = reference.candidates(&evidence, 2);
        let unicode = candidates.iter().find(|node| node.path == "Unicode / Éducation").unwrap();
        assert_eq!(unicode.parent_path(), Some("Unicode"));
        assert_eq!(unicode.name, "Éducation");
        assert_eq!(unicode.normalized_components, ["unicode", "éducation"]);
        let literal = candidates.iter().find(|node| node.path == "Graphic Novels / Horror--Zombies").unwrap();
        assert_eq!(literal.normalized_components, ["graphic novels", "horror--zombies"]);
        assert!(reference.candidates(&evidence, 3).is_empty());
        let root = reference.node_by_path("Unicode").unwrap();
        assert_eq!(root.parent_path(), None);
        assert_eq!(root.name, "Unicode");
    }
}
