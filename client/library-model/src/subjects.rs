//! Controlled subject projection built from source-system parsers and the
//! editable SQLite taxonomy graph.

mod bisac;
mod ddc;
mod fast;
mod lcc;
mod runtime;

use crate::PublicationSubject;
use std::collections::{BTreeMap, BTreeSet};

pub use bisac::{bisac_subject_system, match_bisac_subjects, SubjectAssignment, SubjectNode, SubjectSystem};
pub use ddc::ddc_subject_path;
pub use lcc::{canonical_lcc_notation, lcc_subject_path};
pub use runtime::{
    install_unified_taxonomy_concepts, install_unified_taxonomy_overlay, unified_concept_definitions, unified_concept_id, unified_ddc_subject_paths, unified_has_exact_source_code, unified_lcc_subject_paths, unified_subject_paths,
    unified_subject_similarity_keys, unified_subject_sort_order, unified_taxonomy_release_id, unified_taxonomy_routes, UnifiedConceptDefinition, UnifiedConceptRoute, UnifiedTaxonomy, DEFAULT_UNIFIED_TAXONOMY_SQLITE,
};

pub const BISAC_SYSTEM_ID: &str = "bisac";
pub const BISAC_VERSION: &str = "2021";
pub const BISAC_MATCHER_VERSION: i64 = 2;
pub const DDC_SYSTEM_ID: &str = "ddc";
pub const DDC_VERSION: &str = "23-summary-en-2024-08-01";
pub const DDC_MATCHER_VERSION: i64 = 2;
pub const LCC_SYSTEM_ID: &str = "lcc";
pub const LCC_VERSION: &str = "2024-outline-r2";
pub const LCC_MATCHER_VERSION: i64 = 2;
pub const UNIFIED_SYSTEM_ID: &str = "unified";
pub const UNIFIED_VERSION: &str = "6";
pub const UNIFIED_MATCHER_VERSION: i64 = 12;
const GENERIC_HISTORY_CONCEPT_ID: &str = "bokheim:subject:history";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectedSubjectAssignment {
    system_id: String,
    subject_path: String,
    matcher_version: i64,
    evidence_positions: Vec<usize>,
}

impl ProjectedSubjectAssignment {
    pub fn system_id(&self) -> &str {
        &self.system_id
    }
    pub fn subject_path(&self) -> &str {
        &self.subject_path
    }
    pub fn matcher_version(&self) -> i64 {
        self.matcher_version
    }
    pub fn evidence_positions(&self) -> &[usize] {
        &self.evidence_positions
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectedSubjectCode {
    system_id: String,
    code: String,
    evidence_positions: Vec<usize>,
}

impl ProjectedSubjectCode {
    pub fn system_id(&self) -> &str {
        &self.system_id
    }
    pub fn code(&self) -> &str {
        &self.code
    }
    pub fn evidence_positions(&self) -> &[usize] {
        &self.evidence_positions
    }
}

pub fn project_subjects(subjects: &[PublicationSubject]) -> Vec<ProjectedSubjectAssignment> {
    let mut projected = BTreeMap::<(String, String, i64), BTreeSet<usize>>::new();
    for assignment in match_bisac_subjects(subjects) {
        projected.entry((BISAC_SYSTEM_ID.to_owned(), assignment.node_path().to_owned(), BISAC_MATCHER_VERSION)).or_default().extend(assignment.evidence_positions());
    }
    for (position, subject) in subjects.iter().enumerate() {
        let Some((system_id, authority_label)) = external_subject_system(subject.authority()) else { continue };
        let notation = subject.code().unwrap_or_else(|| subject.name());
        let (subject_path, matcher_version) = match system_id {
            DDC_SYSTEM_ID => (ddc_subject_path(notation).unwrap_or_else(|| format!("DDC / {}", compact_whitespace(notation))), DDC_MATCHER_VERSION),
            LCC_SYSTEM_ID => (lcc_subject_path(notation).unwrap_or_else(|| format!("LCC / Unclassified / {}", compact_whitespace(notation))), LCC_MATCHER_VERSION),
            _ => {
                let components = subject.name().split('/').map(compact_whitespace).filter(|component| !component.is_empty()).collect::<Vec<_>>();
                if components.is_empty() {
                    continue;
                }
                (["LCC".to_owned(), authority_label.to_owned()].into_iter().chain(components).collect::<Vec<_>>().join(" / "), 1)
            }
        };
        projected.entry((system_id.to_owned(), subject_path, matcher_version)).or_default().insert(position);
    }
    projected.into_iter().map(|((system_id, subject_path, matcher_version), evidence)| ProjectedSubjectAssignment { system_id, subject_path, matcher_version, evidence_positions: evidence.into_iter().collect() }).collect()
}

pub fn project_subject_codes(subjects: &[PublicationSubject]) -> Vec<ProjectedSubjectCode> {
    let mut projected = BTreeMap::<(String, String), BTreeSet<usize>>::new();
    let bisac = bisac_subject_system();
    for assignment in match_bisac_subjects(subjects) {
        let Some(code) = bisac.node_by_path(assignment.node_path()).and_then(SubjectNode::code) else { continue };
        projected.entry((BISAC_SYSTEM_ID.to_owned(), code.to_owned())).or_default().extend(assignment.evidence_positions());
    }
    for (position, subject) in subjects.iter().enumerate() {
        let Some((system_id, _)) = external_subject_system(subject.authority()) else { continue };
        let raw = subject.code().unwrap_or_else(|| subject.name());
        let code = match system_id {
            DDC_SYSTEM_ID => ddc::canonical_ddc_notation(raw),
            LCC_SYSTEM_ID => canonical_lcc_notation(raw),
            _ => None,
        };
        if let Some(code) = code {
            projected.entry((system_id.to_owned(), code)).or_default().insert(position);
        }
    }
    projected.into_iter().map(|((system_id, code), evidence)| ProjectedSubjectCode { system_id, code, evidence_positions: evidence.into_iter().collect() }).collect()
}

pub fn project_unified_subjects(subjects: &[PublicationSubject]) -> Vec<ProjectedSubjectAssignment> {
    let mut projected = BTreeMap::<String, BTreeSet<usize>>::new();
    let runtime = runtime::current();
    for classification in project_subject_codes(subjects) {
        for concept_id in runtime::matching_concepts(&runtime, classification.system_id(), classification.code()) {
            for path in runtime.paths_by_concept.get(&concept_id).cloned().unwrap_or_default() {
                projected.entry(path).or_default().extend(classification.evidence_positions());
            }
        }
    }
    for (position, subject) in subjects.iter().enumerate() {
        let uncontrolled_dc = subject.source().trim().eq_ignore_ascii_case("dc:subject") && subject.authority().is_none() && subject.code().is_none();
        for normalized in normalized_source_labels(subject) {
            // Plain Dublin Core terms are uncontrolled keywords. Common labels
            // such as a country or century occur throughout LCC and must not
            // independently activate every concept carrying that caption.
            if uncontrolled_dc && normalized != "history" {
                continue;
            }
            let Some(concepts) = runtime.source_labels.get(&normalized) else { continue };
            let dc_generic_history = uncontrolled_dc && normalized == "history" && concepts.contains(GENERIC_HISTORY_CONCEPT_ID);
            let concepts = concepts.iter().filter(|concept_id| !dc_generic_history || concept_id.as_str() == GENERIC_HISTORY_CONCEPT_ID);
            for path in concepts.flat_map(|concept_id| runtime.paths_by_concept.get(concept_id).cloned().unwrap_or_default()) {
                projected.entry(path).or_default().insert(position);
            }
        }
        for path in fast::matching_hierarchy_paths(&runtime, subject) {
            projected.entry(path).or_default().insert(position);
        }
    }
    projected
        .into_iter()
        .map(|(subject_path, evidence)| ProjectedSubjectAssignment { system_id: UNIFIED_SYSTEM_ID.to_owned(), subject_path, matcher_version: UNIFIED_MATCHER_VERSION, evidence_positions: evidence.into_iter().collect() })
        .collect()
}

fn normalized_source_labels(subject: &PublicationSubject) -> BTreeSet<String> {
    let mut labels = BTreeSet::from([runtime::normalize_source_label(subject.name())]);
    let source = subject.source().trim().to_ascii_lowercase();
    let authority = subject.authority().map(str::trim).unwrap_or_default().to_ascii_lowercase();
    let delimiter = if source == "dc:subject" && authority.is_empty() && subject.code().is_none() {
        Some(";")
    } else if matches!(authority.as_str(), "lcgft" | "lc genre" | "lc_genre") || source.ends_with("/#lc_genre") {
        Some(">")
    } else {
        None
    };
    if let Some(delimiter) = delimiter {
        labels.extend(subject.name().split(delimiter).map(runtime::normalize_source_label).filter(|label| !label.is_empty()));
    }
    labels.retain(|label| !label.is_empty());
    labels
}

fn external_subject_system(authority: Option<&str>) -> Option<(&'static str, &'static str)> {
    match authority?.trim().to_ascii_lowercase().as_str() {
        "lcsh" => Some(("lcsh", "LCSH")),
        "lcgft" => Some(("lcgft", "LCGFT")),
        "lcc" => Some((LCC_SYSTEM_ID, "LCC")),
        "ddc" => Some((DDC_SYSTEM_ID, "DDC")),
        _ => None,
    }
}

fn split_path(value: &str) -> Vec<String> {
    value.replace("::", "/").replace("--", "/").replace(['>', '|', '›', '→'], "/").split('/').map(compact_whitespace).filter(|component| !component.is_empty()).collect()
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

    #[test]
    fn plain_dc_history_maps_only_to_generic_humanities_history() {
        let subjects = ["History", "United States", "20th Century"].into_iter().map(|name| PublicationSubject::new(None, name, "dc:subject", None, None).unwrap()).collect::<Vec<_>>();
        let paths = project_unified_subjects(&subjects).into_iter().map(|assignment| assignment.subject_path().to_owned()).collect::<BTreeSet<_>>();

        assert!(paths.contains("Humanities / History"));
        assert!(paths.contains("Humanities / History / History by Region / Americas / North America / United States / Historical Eras / 20th Century"));
        assert!(paths.iter().all(|path| path.starts_with("Humanities / History")));
    }

    #[test]
    fn fine_lcc_identifiers_are_recorded_independently_of_local_mapping_detail() {
        let subject = PublicationSubject::new(None, "Regional law detail", "marc", Some("lcc".to_owned()), Some("kjcx 12.4 .a2".to_owned())).unwrap();
        let codes = project_subject_codes(&[subject]);

        assert_eq!(codes.len(), 1);
        assert_eq!(codes[0].system_id(), LCC_SYSTEM_ID);
        assert_eq!(codes[0].code(), "KJCX 12.4 .A2");
    }
}
