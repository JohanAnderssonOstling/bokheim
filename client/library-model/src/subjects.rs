//! Controlled subject taxonomies and deterministic metadata matching.
//!
//! Raw publication subjects remain bibliographic evidence. The values in this
//! module are derived browse assignments and may be rebuilt at any time.

use crate::PublicationSubject;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::OnceLock;

pub const BISAC_SYSTEM_ID: &str = "bisac";
pub const BISAC_VERSION: &str = "2021";
pub const BISAC_MATCHER_VERSION: i64 = 2;

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

    fn node_by_path(&self, path: &str) -> Option<&SubjectNode> {
        self.path_index.get(&normalize_path(path)).map(|index| &self.nodes[*index])
    }

    fn node_by_code(&self, code: &str) -> Option<&SubjectNode> {
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
    SubjectSystem { id: BISAC_SYSTEM_ID, version: BISAC_VERSION, nodes, path_index, code_index }
}

/// Matches exact BISAC labels, explicit paths, and explicit BISAC codes.
///
/// A flattened path matches only when metadata supplies every component. A
/// complete slash-delimited declaration supplies its own components. The
/// result contains only the most-specific matches; `General` remains when its
/// parent branch has no more-specific alternative.
pub fn match_bisac_subjects(subjects: &[PublicationSubject]) -> Vec<SubjectAssignment> {
    let system = bisac_subject_system();
    let mut component_evidence = HashMap::<String, BTreeSet<usize>>::new();
    let mut matches = BTreeMap::<String, BTreeSet<usize>>::new();

    for (position, subject) in subjects.iter().enumerate() {
        let authority = subject.authority().map(str::trim).filter(|authority| !authority.is_empty());
        if authority.is_some_and(|authority| !authority.eq_ignore_ascii_case(BISAC_SYSTEM_ID)) {
            // An explicitly declared vocabulary is authoritative. Preserve the
            // raw metadata, but never reinterpret another system's labels as
            // BISAC merely because some terms happen to be spelled alike.
            continue;
        }

        let components = split_path(subject.name());
        for component in &components {
            component_evidence.entry(normalize_term(component)).or_default().insert(position);
        }

        if components.len() > 1 {
            if let Some(node) = system.node_by_path(subject.name()) {
                matches.entry(node.path.clone()).or_default().insert(position);
            }
        }

        if authority.is_some_and(|authority| authority.eq_ignore_ascii_case(BISAC_SYSTEM_ID)) {
            if let Some(node) = subject.code().and_then(|code| system.node_by_code(code)) {
                matches.entry(node.path.clone()).or_default().insert(position);
            }
        }
    }

    for node in system.nodes() {
        let mut evidence = BTreeSet::<usize>::new();
        let all_components_match = node.normalized_components.iter().all(|component| {
            if let Some(positions) = component_evidence.get(component) {
                evidence.extend(positions.iter().copied());
                true
            } else {
                false
            }
        });
        if all_components_match {
            matches.entry(node.path.clone()).or_default().extend(evidence);
        }
    }

    let candidate_paths = matches.keys().cloned().collect::<Vec<_>>();
    matches
        .into_iter()
        .filter(|(path, _)| !has_matched_descendant(path, &candidate_paths))
        .filter(|(path, _)| !general_has_specific_alternative(path, &candidate_paths, system))
        .map(|(node_path, evidence)| SubjectAssignment { node_path, evidence_positions: evidence.into_iter().collect() })
        .collect()
}

fn has_matched_descendant(path: &str, candidates: &[String]) -> bool {
    let prefix = format!("{path} / ");
    candidates.iter().any(|candidate| candidate.starts_with(&prefix))
}

fn general_has_specific_alternative(path: &str, candidates: &[String], system: &SubjectSystem) -> bool {
    let Some(node) = system.node_by_path(path) else { return false };
    if !node.name.eq_ignore_ascii_case("general") {
        return false;
    }
    let Some(parent) = node.parent_path() else { return false };
    let prefix = format!("{parent} / ");
    candidates.iter().any(|candidate| candidate != path && candidate.starts_with(&prefix))
}

fn split_path(value: &str) -> Vec<String> {
    // EPUB metadata uses several equivalent hierarchy separators in practice.
    // Normalizing only the separator keeps component matching exact.
    let normalized = value.replace("::", "/").replace("--", "/").replace(['>', '|', '›', '→'], "/");
    normalized.split('/').map(compact_whitespace).filter(|component| !component.is_empty()).collect()
}

fn normalize_path(value: &str) -> String {
    split_path(value).iter().map(|component| normalize_term(component)).collect::<Vec<_>>().join("/")
}

fn normalize_term(value: &str) -> String {
    compact_whitespace(value).to_lowercase()
}

fn compact_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn subject(name: &str) -> PublicationSubject {
        PublicationSubject::new(None, name, "dc:subject", None, None).unwrap()
    }

    fn paths(subjects: &[PublicationSubject]) -> Vec<String> {
        match_bisac_subjects(subjects).into_iter().map(|assignment| assignment.node_path).collect()
    }

    #[test]
    fn embedded_bisac_has_codes_and_virtual_parents() {
        let system = bisac_subject_system();
        assert_eq!(system.version(), "2021");
        assert_eq!(system.node_by_code("HIS036060").unwrap().path(), "History / United States / 20th Century");
        assert!(system.node_by_path("History / United States").unwrap().code().is_none());
    }

    #[test]
    fn flattened_white_house_years_metadata_matches_specific_siblings() {
        let subjects = [
            "History",
            "General",
            "International Relations",
            "Political",
            "Personal Memoirs",
            "Political Science",
            "Biography & Autobiography",
            "Biography",
            "Historical",
            "United States",
            "20th Century",
            "Presidents & Heads of State",
            "Statesmen",
        ]
        .map(subject);
        assert_eq!(
            paths(&subjects),
            vec![
                "Biography & Autobiography / Historical",
                "Biography & Autobiography / Personal Memoirs",
                "Biography & Autobiography / Political",
                "Biography & Autobiography / Presidents & Heads of State",
                "History / United States / 20th Century",
                "Political Science / International Relations / General",
            ]
        );
    }

    #[test]
    fn general_is_retained_only_without_a_specific_alternative_in_its_branch() {
        assert_eq!(paths(&[subject("History"), subject("General")]), vec!["History / General"]);
        assert_eq!(paths(&[subject("History"), subject("General"), subject("United States")]), vec!["History / United States / General"]);
        assert_eq!(paths(&[subject("History"), subject("General"), subject("United States"), subject("20th Century")]), vec!["History / United States / 20th Century"]);
    }

    #[test]
    fn explicit_paths_and_codes_supply_the_hierarchy() {
        assert_eq!(paths(&[subject("History / United States / 20th Century")]), vec!["History / United States / 20th Century"]);
        assert_eq!(paths(&[subject("History -- United States -- 20th Century")]), vec!["History / United States / 20th Century"]);
        assert_eq!(paths(&[subject("History > United States > 20th Century")]), vec!["History / United States / 20th Century"]);
        let coded = PublicationSubject::new(None, "outdated display text", "dc:subject", Some("BISAC".to_owned()), Some("HIS036060".to_owned())).unwrap();
        assert_eq!(paths(&[coded]), vec!["History / United States / 20th Century"]);
    }

    #[test]
    fn explicit_authority_is_not_reinterpreted_as_bisac() {
        let lcsh = PublicationSubject::new(None, "History / United States / 20th Century", "dc:subject", Some("LCSH".to_owned()), None).unwrap();
        assert!(paths(&[lcsh]).is_empty());

        let bisac = PublicationSubject::new(None, "History / United States / 20th Century", "dc:subject", Some("BISAC".to_owned()), None).unwrap();
        assert_eq!(paths(&[bisac]), vec!["History / United States / 20th Century"]);
    }

    #[test]
    fn matching_is_exact_after_case_and_whitespace_normalization() {
        assert_eq!(paths(&[subject(" history "), subject(" united   states "), subject("20TH century")]), vec!["History / United States / 20th Century"]);
        assert!(paths(&[subject("Hist")]).is_empty());
    }

    #[test]
    fn parser_source_does_not_change_the_synchronized_subject_shape() {
        let subject = PublicationSubject::new(None, "History", "se:subject", None, None).unwrap();
        let encoded = wire::encode(&subject).unwrap();
        let decoded: PublicationSubject = wire::decode(&encoded, 1024).unwrap();
        assert_eq!(decoded.name(), "History");
        assert_eq!(decoded.source(), "dc:subject", "remote metadata uses the standard fallback source");
    }
}
