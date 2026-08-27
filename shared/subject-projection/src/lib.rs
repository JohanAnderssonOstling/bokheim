//! Controlled subject projection built from source-system parsers and the
//! editable SQLite taxonomy graph.

mod bisac;
mod ddc;
mod fast;
mod lcc;
mod runtime;
mod taxonomy_sqlite;

pub use publication_model::PublicationSubject;
use std::collections::{BTreeMap, BTreeSet};

pub use bisac::{bisac_code_for_path, bisac_subject_system, match_bisac_subjects, match_bisac_subjects_with_minimum_adjacent, SubjectAssignment, SubjectNode, SubjectSystem};
pub use ddc::ddc_subject_path;
pub use lcc::{canonical_lcc_notation, lcc_subject_path};
pub use runtime::{
    install_unified_taxonomy_concepts, install_unified_taxonomy_overlay, loaded_unified_taxonomy_release_id, unified_concept_definitions, unified_concept_id, unified_ddc_subject_paths, unified_has_exact_source_code,
    unified_lcc_subject_paths, unified_subject_paths, unified_subject_similarity_keys, unified_subject_sort_order, unified_taxonomy_release_id, unified_taxonomy_routes, UnifiedConceptDefinition, UnifiedConceptRoute, UnifiedTaxonomy,
    BUNDLED_UNIFIED_TAXONOMY_RELEASE_ID, DEFAULT_UNIFIED_TAXONOMY_SQLITE,
};
pub use taxonomy_sqlite::{
    has_unified_taxonomy_overlay, has_unified_taxonomy_overlay_in, is_unified_taxonomy_client_seed, is_unified_taxonomy_client_seed_in, merge_unified_taxonomy_connection, merge_unified_taxonomy_sqlite,
    merge_unified_taxonomy_sqlite_with_resolutions, read_resolved_unified_taxonomy_codes, read_resolved_unified_taxonomy_codes_in, read_unified_taxonomy_connection, read_unified_taxonomy_sqlite, reset_unified_taxonomy_connection,
    unified_taxonomy_server_release_id, unified_taxonomy_server_release_id_in,
};

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

pub const BISAC_SYSTEM_ID: &str = "bisac";
pub const BISAC_VERSION: &str = "2021";
pub const BISAC_MATCHER_VERSION: i64 = 8;
pub const DDC_SYSTEM_ID: &str = "ddc";
pub const DDC_VERSION: &str = "23-summary-en-2024-08-01";
pub const DDC_MATCHER_VERSION: i64 = 2;
pub const LCC_SYSTEM_ID: &str = "lcc";
pub const LCC_VERSION: &str = "2024-outline-r2";
pub const LCC_MATCHER_VERSION: i64 = 2;
pub const ROOT_SUBJECT_PATH: &str = "Subject";
pub const UNIFIED_SYSTEM_ID: &str = "unified";
pub const UNIFIED_VERSION: &str = "6";
pub const UNIFIED_MATCHER_VERSION: i64 = 20;
const GENERIC_HISTORY_CONCEPT_ID: &str = "bokheim:subject:history";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectedSubjectAssignment {
    pub system_id: String,
    pub subject_path: String,
    pub matcher_version: i64,
    pub evidence_positions: Vec<usize>,
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
    pub system_id: String,
    pub code: String,
    pub evidence_positions: Vec<usize>,
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
    for assignment in match_bisac_subjects(subjects) {
        let Some(code) = bisac_code_for_path(assignment.node_path()) else { continue };
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
    match canonical_subject_system(authority?)? {
        "lcsh" => Some(("lcsh", "LCSH")),
        "lcgft" => Some(("lcgft", "LCGFT")),
        "lcc" => Some((LCC_SYSTEM_ID, "LCC")),
        "ddc" => Some((DDC_SYSTEM_ID, "DDC")),
        _ => None,
    }
}

pub(crate) fn canonical_subject_system(authority: &str) -> Option<&'static str> {
    let authority = authority.trim().to_ascii_lowercase();
    if authority.is_empty() {
        return None;
    }
    let compact = authority.chars().filter(|character| character.is_ascii_alphanumeric()).collect::<String>();
    if authority == BISAC_SYSTEM_ID || authority.contains("bisac") {
        Some(BISAC_SYSTEM_ID)
    } else if matches!(authority.as_str(), "ddc" | "dewey" | "dewey decimal classification") || compact.contains("deweydecimal") {
        Some(DDC_SYSTEM_ID)
    } else if matches!(authority.as_str(), "lcc" | "library of congress classification") || compact.contains("libraryofcongressclassification") || authority.contains("authorities/classification") {
        Some(LCC_SYSTEM_ID)
    } else if matches!(authority.as_str(), "lcsh" | "library of congress subject headings") || compact.contains("libraryofcongresssubjectheadings") || authority.contains("authorities/subjects") {
        Some("lcsh")
    } else if matches!(authority.as_str(), "lcgft" | "lc genre" | "lc_genre") || authority.contains("genreform") {
        Some("lcgft")
    } else {
        None
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
        runtime::install_authoritative_taxonomy_for_tests();
        let subjects = ["History", "United States", "20th Century"].into_iter().map(|name| PublicationSubject::new(None, name, "dc:subject", None, None).unwrap()).collect::<Vec<_>>();
        let paths = project_unified_subjects(&subjects).into_iter().map(|assignment| assignment.subject_path().to_owned()).collect::<BTreeSet<_>>();

        assert!(paths.contains("Humanities / History"));
        assert!(paths.contains("Humanities / History / History by Region / Americas / North America / United States / Historical Eras / 20th Century"));
        assert!(paths.iter().all(|path| path.starts_with("Humanities / History")));
    }

    #[test]
    fn plain_dc_bisac_headings_resolve_through_codes_before_unified_projection() {
        runtime::install_authoritative_taxonomy_for_tests();
        for (heading, expected_code) in
            [("Fiction", "FIC000000"), ("History", "HIS000000"), ("Juvenile Fiction", "JUV000000"), ("Drama", "DRA000000"), ("Poetry", "POE000000"), ("Science", "SCI000000"), ("Fiction / Short Stories", "FIC029000")]
        {
            let subject = PublicationSubject::new(None, heading, "dc:subject", None, None).unwrap();
            let codes = project_subject_codes(std::slice::from_ref(&subject));
            assert!(codes.iter().any(|code| code.system_id() == BISAC_SYSTEM_ID && code.code() == expected_code), "{heading} must resolve to {expected_code}");

            let expected_paths = unified_subject_paths(BISAC_SYSTEM_ID, expected_code).into_iter().collect::<BTreeSet<_>>();
            assert!(!expected_paths.is_empty(), "{expected_code} must exist in the unified taxonomy");
            let projected_paths = project_unified_subjects(&[subject]).into_iter().map(|assignment| assignment.subject_path().to_owned()).collect::<BTreeSet<_>>();
            assert!(expected_paths.is_subset(&projected_paths), "{heading} must use its BISAC code's unified routes");
        }
    }

    #[test]
    fn source_system_uris_resolve_codes_and_unknown_systems_fall_back_to_bisac_paths() {
        let ddc = PublicationSubject::new(None, "Mathematics", "opds2:subject", Some("https://example.org/schemes/dewey-decimal-classification".to_owned()), Some("510".to_owned())).unwrap();
        let ddc_codes = project_subject_codes(&[ddc]);
        assert!(ddc_codes.iter().any(|code| code.system_id() == DDC_SYSTEM_ID && code.code() == "510"));

        let bisac = PublicationSubject::new(None, "History / United States / 20th Century", "opds2:subject", Some("https://vendor.example/subjects".to_owned()), None).unwrap();
        let bisac_codes = project_subject_codes(&[bisac]);
        assert!(bisac_codes.iter().any(|code| code.system_id() == BISAC_SYSTEM_ID && code.code() == "HIS036060"));
    }

    #[test]
    fn bisac_components_in_separate_dc_subjects_keep_combined_evidence_in_unified_projection() {
        runtime::install_authoritative_taxonomy_for_tests();
        let subjects = ["Fiction", "Short Stories"].into_iter().map(|heading| PublicationSubject::new(None, heading, "dc:subject", None, None).unwrap()).collect::<Vec<_>>();
        let bisac = match_bisac_subjects(&subjects);
        let short_stories = bisac.iter().find(|assignment| assignment.node_path() == "Fiction / Short Stories").expect("production subject components must infer the BISAC path");
        assert_eq!(short_stories.evidence_positions(), &[0, 1]);

        let expected = unified_subject_paths(BISAC_SYSTEM_ID, "FIC029000").into_iter().collect::<BTreeSet<_>>();
        let unified = project_unified_subjects(&subjects);
        for path in expected {
            let assignment = unified.iter().find(|assignment| assignment.subject_path() == path).expect("the inferred BISAC code must resolve into unified taxonomy");
            assert_eq!(assignment.evidence_positions(), &[0, 1]);
        }
    }

    #[test]
    fn lcsh_subjects_require_a_complete_adjacent_bisac_hierarchy() {
        for heading in ["History -- Great Britain", "Great Britain -- History -- Victoria, 1837-1901 -- Fiction", "Clothing and dress -- United States -- History"] {
            let subject = PublicationSubject::new(None, heading, "dc:subject", None, None).unwrap();
            let codes = project_subject_codes(&[subject]);
            assert!(codes.iter().all(|code| code.code() != "HIS015000"), "incomplete or reordered LCSH heading {heading} produced Great Britain history: {codes:?}");
        }

        let complete = PublicationSubject::new(None, "History -- Europe -- Great Britain", "marc", Some("lcsh".to_owned()), None).unwrap();
        assert!(project_subject_codes(&[complete]).iter().any(|code| code.system_id() == BISAC_SYSTEM_ID && code.code() == "HIS015000"));

        let fiction = PublicationSubject::new(None, "Fiction -- Short Stories", "dc:subject", None, None).unwrap();
        assert!(project_subject_codes(&[fiction]).iter().any(|code| code.system_id() == BISAC_SYSTEM_ID && code.code() == "FIC029000"));
    }

    #[test]
    fn bisac_hierarchy_inference_requires_every_component() {
        let partial = ["History", "Great Britain"].into_iter().map(|heading| PublicationSubject::new(None, heading, "dc:subject", None, None).unwrap()).collect::<Vec<_>>();
        assert!(project_subject_codes(&partial).iter().all(|code| code.code() != "HIS015000"), "a missing Europe component must not infer Great Britain history");

        let complete = ["History", "Europe", "Great Britain"].into_iter().map(|heading| PublicationSubject::new(None, heading, "dc:subject", None, None).unwrap()).collect::<Vec<_>>();
        assert!(project_subject_codes(&complete).iter().any(|code| code.system_id() == BISAC_SYSTEM_ID && code.code() == "HIS015000"), "the complete BISAC hierarchy should infer Great Britain history");
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
