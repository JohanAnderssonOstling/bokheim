//! Controlled subject projection built from source-system parsers and the
//! editable SQLite taxonomy graph.

mod bisac;
mod ddc;
mod lcc_recovery;
mod pruning;
mod runtime;
mod taxonomy_sqlite;
use subject_taxonomy::{compact_whitespace, lcc, range_storage, runtime_index, taxonomy_reader};
pub mod installed;

pub use book_model::BookSubject;
pub use lcc_recovery::{LccMatchAssignment, LccMatchEvidence, LccMatchMethod, LccMatchReport};
pub use pruning::redundant_lcc_exact_selectors;
use std::collections::{BTreeMap, BTreeSet};

pub use bisac::{bisac_code_for_path, bisac_subject_system, match_bisac_subjects, match_bisac_subjects_with_minimum_adjacent, SubjectAssignment, SubjectNode, SubjectSystem};
pub use lcc::{canonical_lcc_notation, lcc_subject_path};
pub use runtime::{
    install_unified_taxonomy_concepts, install_unified_taxonomy_overlay, lcc_subject_sort_orders, loaded_unified_taxonomy_revision_id, unified_concept_definitions, unified_concept_id, unified_has_exact_source_code,
    unified_lcc_has_numeric_coverage, unified_lcc_match_report, unified_subject_paths, unified_subject_similarity_keys, unified_subject_sort_order, unified_taxonomy_revision_id, unified_taxonomy_routes,
    UnifiedConceptDefinition, UnifiedConceptRoute, UnifiedTaxonomy, BUNDLED_UNIFIED_TAXONOMY_REVISION_ID,
};
#[cfg(not(target_arch = "wasm32"))]
pub use runtime::DEFAULT_UNIFIED_TAXONOMY_SQLITE;
pub use taxonomy_sqlite::{
    has_unified_taxonomy_overlay, has_unified_taxonomy_overlay_in, is_unified_taxonomy_client_seed, is_unified_taxonomy_client_seed_in, merge_unified_taxonomy_connection, merge_unified_taxonomy_sqlite,
    merge_unified_taxonomy_sqlite_with_resolutions, published_taxonomy_release_id, read_resolved_unified_taxonomy_codes, read_resolved_unified_taxonomy_codes_in, read_unified_taxonomy_connection, read_unified_taxonomy_sqlite,
    unified_taxonomy_server_release_id, unified_taxonomy_server_release_id_in,
};
#[cfg(not(target_arch = "wasm32"))]
pub use taxonomy_sqlite::reset_unified_taxonomy_connection;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnifiedCodeResolution {
    system_id: String,
    code: String,
    matched_concept_ids: Vec<i64>,
}

impl UnifiedCodeResolution {
    pub fn new(system_id: String, code: String, matched_concept_ids: Vec<i64>) -> Self {
        Self { system_id, code, matched_concept_ids }
    }

    pub fn system_id(&self) -> &str {
        &self.system_id
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn matched_concept_ids(&self) -> &[i64] {
        &self.matched_concept_ids
    }
}

pub const BISAC_SYSTEM_ID: &str = "bisac";
pub const BISAC_VERSION: &str = "2021";
pub const DDC_SYSTEM_ID: &str = "ddc";
pub const DDC_MATCHER_VERSION: i64 = 1;
pub use runtime_index::{LCC_SYSTEM_ID, ROOT_SUBJECT_PATH, UNIFIED_VERSION};
pub const UNIFIED_SYSTEM_ID: &str = "unified";
pub const UNIFIED_MATCHER_VERSION: i64 = 136;

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

pub fn project_subject_codes(subjects: &[BookSubject]) -> Vec<ProjectedSubjectCode> {
    let mut projected = BTreeMap::<(String, String), BTreeSet<usize>>::new();
    for assignment in match_bisac_subjects(subjects) {
        let Some(code) = bisac_code_for_path(assignment.node_path()) else { continue };
        projected.entry((BISAC_SYSTEM_ID.to_owned(), code.to_owned())).or_default().extend(assignment.evidence_positions());
    }
    let declared_lcc = subjects.iter().any(|subject| subject.authority().and_then(canonical_subject_system) == Some(LCC_SYSTEM_ID));
    // DDC is a fallback classification. Once the record has an explicit LCC
    // or a resolved BISAC classification, projecting DDC as well creates
    // unrelated subject branches from secondary metadata (for example a
    // mathematics DDC code on a history title). Keep the strongest source
    // only for unified navigation; raw subjects remain unchanged.
    let has_bisac = projected.keys().any(|(system, _)| system == BISAC_SYSTEM_ID);
    let ddc = if !declared_lcc && !has_bisac { {
            subjects
                .iter()
                .enumerate()
                .filter_map(|(position, subject)| {
                    (subject.authority().and_then(canonical_subject_system)? == DDC_SYSTEM_ID).then(|| ddc::canonical_ddc_notation(subject.code().unwrap_or_else(|| subject.name()))).flatten().map(|code| (position, code))
                })
                .collect::<Vec<_>>()
        } } else { Default::default() };
    let maximum_ddc_specificity = ddc.iter().map(|(_, code)| ddc::specificity(code)).max();
    for (position, code) in ddc {
        if Some(ddc::specificity(&code)) == maximum_ddc_specificity {
            projected.entry((DDC_SYSTEM_ID.to_owned(), code)).or_default().insert(position);
        }
    }
    for (position, subject) in subjects.iter().enumerate() {
        if subject.authority() == Some(UNIFIED_SYSTEM_ID) && subject.source() == "manual:subject" {
            if let Some(path) = subject.code().filter(|path| !path.trim().is_empty()) {
                projected.entry((UNIFIED_SYSTEM_ID.to_owned(), path.to_owned())).or_default().insert(position);
            }
            continue;
        }
        if let Some(code) = subject_lcc_code(subject) {
            projected.entry((LCC_SYSTEM_ID.to_owned(), code)).or_default().insert(position);
        }
    }
    projected.into_iter().map(|((system_id, code), evidence)| ProjectedSubjectCode { system_id, code, evidence_positions: evidence.into_iter().collect() }).collect()
}

/// Infer only complete classification values in otherwise uncontrolled embedded
/// subjects. Keep the original declaration and its evidence position unchanged.
fn subject_lcc_code(subject: &BookSubject) -> Option<String> {
    if let Some(authority) = subject.authority() {
        return (canonical_subject_system(authority)? == LCC_SYSTEM_ID).then(|| canonical_lcc_notation(subject.code().unwrap_or_else(|| subject.name()))).flatten();
    }
    if !subject.source().rsplit(':').next()?.eq_ignore_ascii_case("subject") {
        return None;
    }
    let raw = subject.code().unwrap_or_else(|| subject.name()).trim();
    let raw = raw.split_once(':').filter(|(label, _)| label.trim().eq_ignore_ascii_case("lcc")).map_or(raw, |(_, code)| code.trim());
    let canonical = canonical_lcc_notation(raw)?;
    let call = lcc::parse_lcc_call(&canonical)?;
    // ISBNs, BISAC identifiers and plain alphabetic keywords are not LCC.
    if !"ABCDEFGHJKLMNPQRSTUVZ".contains(call.letters.chars().next()?) || (call.letters.len() > 3 && !call.letters.starts_with('K')) {
        return None;
    }
    let numeric = canonical[call.letters.len()..].trim_start_matches([' ', '\'']);
    if numeric.bytes().take_while(u8::is_ascii_digit).count() > 5 {
        return None;
    }
    Some(canonical)
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

#[cfg(test)]
mod tests {


    #[test]
    fn embedded_lcc_detection_rejects_identifiers_prose_and_other_authorities() {
        for value in [
            "9780131103627",
            "ISBN0131103628",
            "ISBN: 9780131103627",
            "9780131103627 The C Programming Language",
            "HIS036060",
            "COM051010",
            "QA",
            "IT",
            "History",
            "Rust programming QA76.73.R87",
            "QA76.73.R87: Rust programming",
            "CPB123",
            "MLCS123",
        ] {
            let subject = BookSubject::new(None, value, "dc:subject", None, None).unwrap();
            assert!(subject_lcc_code(&subject).is_none(), "{value}");
        }
        for authority in ["LCSH", "BISAC", "custom"] {
            let subject = BookSubject::new(None, "QA76.73.R87", "dc:subject", Some(authority.into()), None).unwrap();
            assert!(subject_lcc_code(&subject).is_none(), "{authority}");
        }
    }

    #[test]
    fn apostrophe_calls_keep_cutter_specificity_and_exact_evidence() {
        let definition = |id: i64, code: &str| UnifiedConceptDefinition::new(id, id.to_string(), vec![], BTreeMap::from([("lcc".into(), vec![code.into()])]));
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![definition(1, "HD1..HD999"), definition(2, "HD69.C6"), definition(3, "HD69.C7..HD69.C9")]).unwrap();
        assert_eq!(taxonomy.matching_concept_ids("lcc", "HD'69'C6'1999"), vec![2]);
        assert_eq!(taxonomy.matching_concept_ids("lcc", "HD'69'C8'1999"), vec![3]);
        assert_eq!(taxonomy.matching_concept_ids("lcc", "HD'69'C5'1999"), vec![1]);
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![definition(1, "HD69.C6"), definition(2, "HD'69'C6'1999")]).unwrap();
        assert_eq!(taxonomy.matching_concept_ids("lcc", "HD'69'C6'1999"), vec![2]);
    }


    use super::*;


}
