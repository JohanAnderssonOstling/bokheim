use super::bisac_database::with_reference;
use super::{canonical_subject_system, has_usable_lcc_classification, normalize_path, normalize_term, split_path, BISAC_SYSTEM_ID, UNIFIED_SYSTEM_ID};
use book_model::BookSubject;
use rusqlite::{Connection, OptionalExtension, Row};
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

struct Reference<'a>(&'a Connection);

fn read_node(row: &Row<'_>) -> rusqlite::Result<SubjectNode> {
    let path: String = row.get(0)?;
    // The compiler joins CSV components with this exact separator. Recovering
    // the parent and last label avoids storing both again for every node.
    let (parent_path, name) = match path.rsplit_once(" / ") {
        Some((parent, name)) => (Some(parent.to_owned()), name.to_owned()),
        None => (None, path.clone()),
    };
    Ok(SubjectNode { path, parent_path, name, code: row.get(1)?, normalized_components: Vec::new() })
}

impl Reference<'_> {
    fn node_by_path(&self, path: &str) -> Option<SubjectNode> {
        self.0.prepare_cached("SELECT path,code FROM bisac_node WHERE normalized_path=?1 ORDER BY id DESC LIMIT 1").and_then(|mut query| query.query_row([normalize_path(path)], read_node).optional()).expect("BISAC path query must succeed")
    }

    fn node_by_code(&self, code: &str) -> Option<SubjectNode> {
        self.0.prepare_cached("SELECT path,code FROM bisac_node WHERE code=?1 ORDER BY id DESC LIMIT 1").and_then(|mut query| query.query_row([code.trim().to_ascii_uppercase()], read_node).optional()).expect("BISAC code query must succeed")
    }

    fn candidates(&self, components: &HashMap<String, BTreeSet<usize>>, minimum_components: usize) -> Vec<SubjectNode> {
        if components.is_empty() {
            return Vec::new();
        }
        let keys = serde_json::to_string(&components.keys().collect::<Vec<_>>()).expect("BISAC components must serialize");
        let mut query = self
            .0
            .prepare_cached(
                "SELECT n.path,n.code
            FROM bisac_node n
            WHERE n.id IN (SELECT n.id FROM json_each(?1) selected JOIN bisac_node n ON n.tail_component=selected.value)
              AND n.component_count>=?2 ORDER BY n.id",
            )
            .expect("BISAC candidate query must prepare");
        query
            .query_map(rusqlite::params![keys, i64::try_from(minimum_components).unwrap_or(i64::MAX)], |row| {
                let mut node = read_node(row)?;
                node.normalized_components = node.path.split(" / ").map(normalize_term).collect();
                Ok(node)
            })
            .expect("BISAC candidate query must succeed")
            .collect::<rusqlite::Result<Vec<_>>>()
            .expect("BISAC candidates must be readable")
    }
}

pub fn bisac_code_for_path(path: &str) -> Option<String> {
    with_reference(|db| {
        let reference = Reference(db);
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
        let system = Reference(db);
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
        let reference = Reference(&db);
        assert_eq!(reference.node_by_path("TOPIC > GENERAL").unwrap().path, "topic / General");
        assert_eq!(reference.node_by_code(" aaa000000 ").unwrap().path, "topic / General");
    }

    #[test]
    fn candidate_components_preserve_unicode_and_literal_separators() {
        let db = database();
        let reference = Reference(&db);
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
