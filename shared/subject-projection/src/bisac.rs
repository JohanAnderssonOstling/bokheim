use super::{canonical_subject_system, compact_whitespace, normalize_path, normalize_term, split_path, BISAC_SYSTEM_ID, BISAC_VERSION};
use book_model::BookSubject;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::OnceLock;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubjectNode {
    path: String,
    parent_path: Option<String>,
    name: String,
    code: Option<String>,
    normalized_components: Vec<String>,
}

impl SubjectNode {
    pub fn path(&self) -> &str {
        &self.path
    }
    pub fn parent_path(&self) -> Option<&str> {
        self.parent_path.as_deref()
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn code(&self) -> Option<&str> {
        self.code.as_deref()
    }
}

#[derive(Clone, Debug)]
pub struct SubjectSystem {
    id: &'static str,
    version: &'static str,
    nodes: Vec<SubjectNode>,
    path_index: HashMap<String, usize>,
    code_index: HashMap<String, usize>,
    tail_component_index: HashMap<String, Vec<usize>>,
}

impl SubjectSystem {
    pub fn id(&self) -> &'static str {
        self.id
    }
    pub fn version(&self) -> &'static str {
        self.version
    }
    pub fn nodes(&self) -> &[SubjectNode] {
        &self.nodes
    }
    pub(super) fn node_by_path(&self, path: &str) -> Option<&SubjectNode> {
        self.path_index.get(&normalize_path(path)).map(|index| &self.nodes[*index])
    }
    pub(super) fn node_by_code(&self, code: &str) -> Option<&SubjectNode> {
        self.code_index.get(&code.trim().to_ascii_uppercase()).map(|index| &self.nodes[*index])
    }
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

pub fn bisac_subject_system() -> &'static SubjectSystem {
    static SYSTEM: OnceLock<SubjectSystem> = OnceLock::new();
    SYSTEM.get_or_init(|| parse_bisac(include_str!("../data/bisac-2021.csv")))
}

pub fn bisac_code_for_path(path: &str) -> Option<&'static str> {
    let system = bisac_subject_system();
    system.node_by_path(path).and_then(SubjectNode::code).or_else(|| system.node_by_path(&format!("{path} / General")).and_then(SubjectNode::code))
}

fn parse_bisac(csv: &str) -> SubjectSystem {
    let mut reader = csv::ReaderBuilder::new().flexible(true).from_reader(csv.as_bytes());
    let mut nodes = BTreeMap::<String, SubjectNode>::new();
    for record in reader.records() {
        let record = record.expect("the embedded BISAC CSV must be valid");
        let code = record.get(0).unwrap_or_default().trim();
        let description = record.get(1).unwrap_or_default().trim();
        if code.is_empty() || description.is_empty() {
            continue;
        }
        let components = description.split('/').map(compact_whitespace).filter(|component| !component.is_empty()).collect::<Vec<_>>();
        let mut path_components = Vec::new();
        for (index, component) in components.iter().enumerate() {
            path_components.push(component.clone());
            let path = path_components.join(" / ");
            let parent_path = (path_components.len() > 1).then(|| path_components[..path_components.len() - 1].join(" / "));
            let final_code = (index + 1 == components.len()).then(|| code.to_owned());
            nodes
                .entry(path.clone())
                .and_modify(|node| {
                    if final_code.is_some() {
                        node.code = final_code.clone();
                    }
                })
                .or_insert_with(|| SubjectNode { path, parent_path, name: component.clone(), code: final_code, normalized_components: path_components.iter().map(|component| normalize_term(component)).collect() });
        }
    }
    let nodes = nodes.into_values().collect::<Vec<_>>();
    let path_index = nodes.iter().enumerate().map(|(index, node)| (normalize_path(&node.path), index)).collect();
    let code_index = nodes.iter().enumerate().filter_map(|(index, node)| node.code.as_ref().map(|code| (code.to_ascii_uppercase(), index))).collect();
    let mut tail_component_index = HashMap::<String, Vec<usize>>::new();
    for (index, node) in nodes.iter().enumerate() {
        if let Some(component) = node.normalized_components.last() {
            tail_component_index.entry(component.clone()).or_default().push(index);
        }
    }
    SubjectSystem { id: BISAC_SYSTEM_ID, version: BISAC_VERSION, nodes, path_index, code_index, tail_component_index }
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
    let system = bisac_subject_system();
    let mut matches = BTreeMap::<String, BTreeSet<usize>>::new();
    let mut component_evidence = HashMap::<String, BTreeSet<usize>>::new();
    for (position, subject) in subjects.iter().enumerate() {
        let authority = subject.authority().map(str::trim).filter(|authority| !authority.is_empty());
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
    let candidate_nodes = component_evidence.keys().filter_map(|component| system.tail_component_index.get(component)).flatten().copied().collect::<BTreeSet<_>>();
    for node_index in candidate_nodes {
        let node = &system.nodes[node_index];
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
}
