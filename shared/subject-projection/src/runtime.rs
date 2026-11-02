use super::lcc::{canonical_lcc_notation, compare_lcc_key, lcc_match_keys, parse_lcc_call};
use super::runtime_index::{build_runtime, LccSelector};
use super::runtime_index::{compare_cutter_endpoint, compare_upper_cutters, cutter_components, UnifiedTaxonomyRuntime};
pub use super::runtime_index::{lcc_subject_sort_orders, UnifiedConceptDefinition, UnifiedConceptRoute};
use super::{compact_whitespace, DDC_SYSTEM_ID, LCC_SYSTEM_ID};
use crate::sqlite_matcher::SqliteTaxonomy;
use crate::LccMatchReport;
use rusqlite::Connection;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

/// Release identifier of the bundled taxonomy image. Keep this cheap constant
/// separate from the runtime so an already-projected library can be opened
/// without materializing the complete taxonomy matcher.
pub const BUNDLED_UNIFIED_TAXONOMY_REVISION_ID: &str = include_str!(concat!(env!("OUT_DIR"), "/unified-taxonomy-revision-id.txt"));

static RUNTIME: OnceLock<Mutex<SqliteTaxonomy>> = OnceLock::new();

#[derive(Deserialize)]
struct ReviewedPathAliases {
    aliases: Vec<ReviewedPathAlias>,
}

#[derive(Deserialize)]
struct ReviewedPathAlias {
    path: String,
    concept_id: i64,
}

pub(super) fn reviewed_path_aliases() -> &'static BTreeMap<String, i64> {
    static ALIASES: OnceLock<BTreeMap<String, i64>> = OnceLock::new();
    ALIASES.get_or_init(|| {
        let reviewed: ReviewedPathAliases = serde_json::from_str(include_str!("../data/reviewed-literature-path-aliases.json")).expect("reviewed path aliases must be valid");
        let mut aliases = BTreeMap::new();
        for alias in reviewed.aliases {
            assert!(aliases.insert(alias.path, alias.concept_id).is_none(), "reviewed path aliases must be unique");
        }
        aliases
    })
}

#[cfg(test)]
fn concept_for_path(runtime: &UnifiedTaxonomyRuntime, path: &str) -> Option<i64> {
    if let Some(route) = (!path.starts_with("concept:")).then(|| runtime.routes.find(path)).flatten() {
        return Some(runtime.routes.concept_id(route));
    }
    reviewed_concept_reference(path, |identity| runtime.routes.paths_for_concept(identity).next().is_some())
}

/// Retired IDs can have the same reviewed redirects as former display paths.
/// An ID still present in an older selected snapshot keeps its original meaning.
pub(super) fn reviewed_concept_reference(path: &str, mut contains: impl FnMut(i64) -> bool) -> Option<i64> {
    if let Some(identity) = path.strip_prefix("concept:").and_then(|value| value.parse::<i64>().ok()) {
        if contains(identity) {
            return Some(identity);
        }
    }
    let identity = *reviewed_path_aliases().get(path)?;
    // A selected/installed taxonomy may omit a subject. Never manufacture
    // assignments to identities absent from the active graph.
    contains(identity).then_some(identity)
}

/// An owned, immutable taxonomy matcher suitable for servers and tools that
/// must not replace the process-wide client taxonomy.
#[derive(Clone)]
pub struct UnifiedTaxonomy {
    runtime: Arc<UnifiedTaxonomyRuntime>,
}

impl UnifiedTaxonomy {
    pub fn from_concepts(concepts: Vec<UnifiedConceptDefinition>) -> Result<Self, String> {
        Ok(Self { runtime: Arc::new(build_runtime(concepts)?) })
    }

    pub fn revision_id(&self) -> &str {
        &self.runtime.revision_id
    }

    pub fn matching_concept_ids(&self, system_id: &str, code: &str) -> Vec<i64> {
        matching_concepts(self.runtime.as_ref(), system_id, code).into_iter().collect()
    }

    /// Returns candidates for one code, or independently resolved subjects for an explicit LCC list.
    pub fn match_candidates(&self, system_id: &str, code: &str) -> Vec<i64> {
        matching_candidates(self.runtime.as_ref(), system_id, code).into_iter().collect()
    }

    /// Returns candidates for one code, or independently resolved subjects for an explicit list.
    /// Resolved subjects, recovery methods and fragments that still need review.
    pub fn lcc_match_report(&self, code: &str) -> LccMatchReport {
        lcc_match_report(self.runtime.as_ref(), code)
    }

    /// Unrelated exact-selector conflicts require curation; ancestors are shadowed.
    pub fn lcc_selector_conflicts(&self) -> BTreeMap<String, Vec<i64>> {
        self.runtime
            .selectors
            .get(LCC_SYSTEM_ID)
            .map(|index| {
                index
                    .exact
                    .iter()
                    .filter_map(|(code, ids)| {
                        let shadowed = ids.iter().flat_map(|id| self.runtime.ancestors(*id).iter().copied()).collect::<BTreeSet<_>>();
                        let specific = ids.iter().copied().filter(|id| !shadowed.contains(id)).collect::<Vec<_>>();
                        (specific.len() > 1).then(|| (code.clone(), specific))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn has_exact_source_code(&self, system_id: &str, code: &str) -> bool {
        has_exact_source_code(self.runtime.as_ref(), system_id, code)
    }
}

fn current() -> MutexGuard<'static, SqliteTaxonomy> {
    RUNTIME.get_or_init(|| Mutex::new(default_runtime())).lock().unwrap_or_else(|error| error.into_inner())
}

pub fn install_unified_taxonomy_concepts(concepts: Vec<UnifiedConceptDefinition>) -> Result<(), String> {
    install_runtime(SqliteTaxonomy::from_concepts(concepts)?)
}

fn install_runtime(replacement: SqliteTaxonomy) -> Result<(), String> {
    // Initialize with the replacement without opening the default database.
    let mut replacement = Some(replacement);
    let runtime = RUNTIME.get_or_init(|| Mutex::new(replacement.take().unwrap()));
    if let Some(replacement) = replacement {
        *runtime.lock().map_err(|_| "unified taxonomy lock is poisoned".to_owned())? = replacement;
    }
    Ok(())
}

pub(crate) fn install_from_connection(db: Connection) -> Result<(), String> {
    install_runtime(SqliteTaxonomy::from_connection(db)?)
}

pub fn unified_taxonomy_revision_id() -> String {
    current().revision_id.clone()
}

/// Returns the active content revision only when a matcher has already been built.
/// This is intentionally non-initializing for startup-sensitive callers.
pub fn loaded_unified_taxonomy_revision_id() -> Option<String> {
    RUNTIME.get().and_then(|runtime| runtime.lock().ok().map(|runtime| runtime.revision_id.clone()))
}

pub fn unified_subject_paths(system_id: &str, code: &str) -> Vec<String> {
    let runtime = current();
    if system_id == crate::UNIFIED_SYSTEM_ID {
        if runtime.find_path(code).is_some() {
            return vec![code.to_owned()];
        }
        return runtime.concept_for_path(code).map(|id| runtime.paths_for_concept(id)).unwrap_or_default();
    }
    matching_concepts(&*runtime, system_id, code).into_iter().flat_map(|id| runtime.paths_for_concept(id)).collect::<BTreeSet<_>>().into_iter().collect()
}

pub(super) fn resolved_subject_concepts(system_id: &str, code: &str) -> BTreeSet<i64> {
    let runtime = current();
    if system_id == crate::UNIFIED_SYSTEM_ID {
        return runtime.concept_for_path(code).into_iter().collect();
    }
    matching_concepts(&*runtime, system_id, code)
}

/// Remove only assigned ancestors of other assignments. Sibling branches and
/// unrelated roots remain independent, regardless of their depth or source code.
pub fn most_specific_concept_ids(concepts: impl IntoIterator<Item = i64>) -> BTreeSet<i64> {
    most_specific_concept_ids_in(&*current(), concepts)
}

fn most_specific_concept_ids_in(runtime: &impl MatchSource, concepts: impl IntoIterator<Item = i64>) -> BTreeSet<i64> {
    let mut concepts = concepts.into_iter().collect::<BTreeSet<_>>();
    let ancestors = runtime.assigned_ancestors(&concepts);
    concepts.retain(|id| !ancestors.contains(id));
    concepts
}

fn has_exact_source_code(runtime: &impl MatchSource, system_id: &str, code: &str) -> bool {
    let code = compact_whitespace(code).to_ascii_uppercase();
    let code = if system_id == LCC_SYSTEM_ID { lcc_match_keys(&code).and_then(|keys| keys.last().cloned()).unwrap_or(code) } else { code };
    !runtime.exact(system_id, &code).is_empty()
}

/// Returns the nearest stable taxonomy branches used to compare external
/// classifications. The first two levels identify the broad domain; the
/// third distinguishes branches such as Classical Greek & Latin Literatures
/// from Romance-Language Literatures.
pub fn unified_subject_similarity_keys(system_id: &str, code: &str) -> Vec<String> {
    unified_subject_paths(system_id, code)
        .into_iter()
        .filter_map(|path| {
            let components = path.split(" / ").take(3).collect::<Vec<_>>();
            (components.len() == 3).then(|| components.join(" / "))
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Returns the ID assigned in the immutable taxonomy; unknown paths have no ID.
pub fn unified_concept_id(path: &str) -> Option<i64> {
    current().concept_for_path(path)
}

pub fn unified_taxonomy_routes() -> Vec<UnifiedConceptRoute> {
    current().routes()
}
/// Whether the class number has numeric coverage in the local taxonomy.
/// A bare letter prefix (or a range spanning different subclasses) cannot
/// validate an extracted number: E3002 must not pass merely because E is known.
/// This is conservative coverage, not proof of an official item assignment.
pub fn unified_lcc_has_numeric_coverage(code: &str) -> bool {
    let Some(canonical) = canonical_lcc_notation(code) else { return false };
    let Some(call) = parse_lcc_call(&canonical) else { return false };
    let runtime = current();
    if lcc_match_keys(&canonical).is_some_and(|keys| keys.iter().any(|key| !runtime.exact(LCC_SYSTEM_ID, key).is_empty() && parse_lcc_call(key).is_some_and(|known| known.number == call.number))) {
        return true;
    }
    runtime.ranges(&call.letters, call.number).iter().any(|range| range.start_letters == call.letters && range.end_letters == call.letters)
}

pub fn unified_lcc_match_report(code: &str) -> LccMatchReport {
    lcc_match_report(&*current(), code)
}

pub(super) fn lcc_match_report(runtime: &impl MatchSource, code: &str) -> LccMatchReport {
    lcc_match_report_with_candidates(runtime, code, matching_candidates(runtime, LCC_SYSTEM_ID, code))
}

fn lcc_match_report_with_candidates(runtime: &impl MatchSource, code: &str, candidates: BTreeSet<i64>) -> LccMatchReport {
    if candidates.len() > 1 {
        return LccMatchReport { original: code.to_owned(), assignments: Vec::new(), unresolved_fragments: vec![code.to_owned()] };
    }
    // Recovery first asks for the whole field again. Reuse this call's result,
    // including misses, instead of executing the same database queries twice.
    crate::lcc_recovery::report(code, |part| if part == code { candidates.iter().copied().collect() } else { strict_matching_concepts(runtime, LCC_SYSTEM_ID, part).into_iter().collect() }, |letters| runtime.has_subclass(letters))
}

pub(super) fn matching_concepts(runtime: &impl MatchSource, system_id: &str, code: &str) -> BTreeSet<i64> {
    let candidates = matching_candidates(runtime, system_id, code);
    // Conflicting complete selectors require curation; recovery must not
    // silently bypass an existing ambiguity by shortening a valid code.
    match candidates.len() {
        0 if system_id == LCC_SYSTEM_ID => lcc_match_report_with_candidates(runtime, code, candidates).assignments.into_iter().map(|a| a.concept_id).collect(),
        0 | 1 => candidates,
        _ => BTreeSet::new(),
    }
}

fn strict_matching_concepts(runtime: &impl MatchSource, system_id: &str, code: &str) -> BTreeSet<i64> {
    let candidates = matching_candidates(runtime, system_id, code);
    if candidates.len() > 1 {
        BTreeSet::new()
    } else {
        candidates
    }
}

fn matching_candidates(runtime: &impl MatchSource, system_id: &str, code: &str) -> BTreeSet<i64> {
    let canonical = if system_id == LCC_SYSTEM_ID {
        let Some(code) = canonical_lcc_notation(code) else { return BTreeSet::new() };
        code
    } else if system_id == DDC_SYSTEM_ID {
        let Some(code) = super::ddc::canonical_ddc_notation(code) else { return BTreeSet::new() };
        code[..3].to_owned()
    } else {
        code.trim().to_ascii_uppercase()
    };
    let separated = (system_id == LCC_SYSTEM_ID).then(|| super::lcc::apostrophe_call_base(&canonical)).flatten();
    let match_code = if system_id == LCC_SYSTEM_ID {
        // Display separators and item suffixes can occur in the same call.
        // Strip validated item enumeration after separator normalization too,
        // so a volume range cannot be mistaken for a classification range.
        let base = separated.as_deref().unwrap_or(&canonical);
        super::lcc::lcc_item_base(base).unwrap_or(base)
    } else {
        &canonical
    };
    // kind, exact/prefix specificity, single subclass, numeric width
    type Rank<'a> = (u8, usize, bool, f64, Option<&'a LccSelector>);
    let mut matches = Vec::<(i64, Rank<'_>)>::new();
    let keys = if system_id == LCC_SYSTEM_ID { lcc_match_keys(&canonical).unwrap_or_default() } else { vec![canonical.clone()] };
    for (specificity, key) in keys.iter().enumerate() {
        matches.extend(runtime.exact(system_id, key).into_iter().map(|id| (id, (2, specificity, false, 0.0, None))));
    }

    let call = (system_id == LCC_SYSTEM_ID).then(|| parse_lcc_call(match_code)).flatten();
    let allow_range_matching = system_id != LCC_SYSTEM_ID || (!match_code.contains('-') && call.as_ref().is_some_and(|call| runtime.has_subclass(&call.letters)));
    let ranges = call.as_ref().filter(|_| allow_range_matching).map(|call| runtime.ranges(&call.letters, call.number)).unwrap_or_default();
    if allow_range_matching {
        if let Some(call) = &call {
            let cutters = cutter_components(call.remainder);
            for range in &ranges {
                if compare_lcc_key(&range.start_letters, range.start_number, &call.letters, call.number).is_le()
                    && compare_lcc_key(&call.letters, call.number, &range.end_letters, range.end_number).is_le()
                    && (call.letters != range.start_letters || call.number != range.start_number || compare_cutter_endpoint(&cutters, &range.start_cutters).is_ge())
                    && (call.letters != range.end_letters || call.number != range.end_number || compare_cutter_endpoint(&cutters, &range.end_cutters).is_le())
                {
                    let single = range.start_letters == call.letters && range.end_letters == call.letters;
                    matches.push((range.concept_id, (1, range.start_cutters.len().max(range.end_cutters.len()), single, if single { range.end_number - range.start_number } else { f64::INFINITY }, Some(range))));
                }
            }
        }
        runtime.visit_prefixes(system_id, |prefix, id| {
            let accepts = if system_id != LCC_SYSTEM_ID || prefix.bytes().all(|byte| byte.is_ascii_alphabetic()) {
                canonical.starts_with(prefix)
            } else {
                lcc_match_keys(prefix).and_then(|keys| keys.last().cloned()).is_some_and(|key| keys.contains(&key))
            };
            if accepts {
                matches.push((id, (0, prefix.len(), false, 0.0, None)));
            }
        });
    }
    let shadowed = runtime.assigned_ancestors(&matches.iter().map(|(id, _)| *id).collect());
    matches.retain(|(id, _)| !shadowed.contains(id));
    let compare = |left: &Rank<'_>, right: &Rank<'_>| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)).then_with(|| left.2.cmp(&right.2)).then_with(|| right.3.total_cmp(&left.3));
    let Some(best) = matches.iter().map(|(_, rank)| rank).max_by(|a, b| compare(a, b)) else { return BTreeSet::new() };
    let tied = matches.iter().filter(|(_, rank)| compare(rank, best).is_eq()).collect::<Vec<_>>();
    // Numeric width alone cannot distinguish nested Cutter intervals at the
    // same class number. Prefer strict containment, retaining genuine ties
    // between equal or merely overlapping intervals.
    tied.iter()
        .filter(|(id, rank)| !tied.iter().any(|(other_id, other_rank)| id != other_id && rank.4.zip(other_rank.4).is_some_and(|(outer, inner)| lcc_range_contains(outer, inner) && !lcc_range_contains(inner, outer))))
        .map(|(id, _)| *id)
        .collect()
}

fn lcc_range_contains(outer: &LccSelector, inner: &LccSelector) -> bool {
    let start = compare_lcc_key(&outer.start_letters, outer.start_number, &inner.start_letters, inner.start_number);
    let end = compare_lcc_key(&inner.end_letters, inner.end_number, &outer.end_letters, outer.end_number);
    start.is_le() && end.is_le() && (!start.is_eq() || compare_cutter_endpoint(&inner.start_cutters, &outer.start_cutters).is_ge()) && (!end.is_eq() || compare_upper_cutters(&inner.end_cutters, &outer.end_cutters).is_le())
}

pub(super) trait MatchSource {
    fn exact(&self, system: &str, code: &str) -> Vec<i64>;
    fn visit_prefixes(&self, system: &str, visit: impl FnMut(&str, i64));
    fn ranges(&self, letters: &str, number: f64) -> Vec<LccSelector>;
    fn has_subclass(&self, letters: &str) -> bool;
    fn assigned_ancestors(&self, concepts: &BTreeSet<i64>) -> BTreeSet<i64>;
}

impl MatchSource for UnifiedTaxonomyRuntime {
    fn exact(&self, system: &str, code: &str) -> Vec<i64> {
        self.selectors.get(system).and_then(|index| index.exact.get(code)).cloned().unwrap_or_default()
    }
    fn visit_prefixes(&self, system: &str, mut visit: impl FnMut(&str, i64)) {
        if let Some(index) = self.selectors.get(system) {
            for (prefix, id) in &index.prefixes {
                visit(prefix, *id);
            }
        }
    }
    fn ranges(&self, letters: &str, number: f64) -> Vec<LccSelector> {
        let Some(index) = self.selectors.get(LCC_SYSTEM_ID) else { return Vec::new() };
        let mut ranges = Vec::new();
        index.lcc_intervals.containing(&index.lcc_ranges, (letters, number), (letters, number), |range| {
            ranges.push(LccSelector {
                start_letters: range.start_letters.clone(),
                start_number: range.start_number,
                end_letters: range.end_letters.clone(),
                end_number: range.end_number,
                start_cutters: range.start_cutters.clone(),
                end_cutters: range.end_cutters.clone(),
                concept_id: range.concept_id,
            })
        });
        ranges
    }
    fn has_subclass(&self, letters: &str) -> bool {
        self.selectors.get(LCC_SYSTEM_ID).is_some_and(|index| index.lcc_subclasses.contains(letters))
    }
    fn assigned_ancestors(&self, concepts: &BTreeSet<i64>) -> BTreeSet<i64> {
        concepts.iter().filter_map(|id| self.ancestors.binary_search_by_key(id, |(id, _)| *id).ok()).flat_map(|index| self.ancestors[index].1.iter().copied()).collect()
    }
}

fn default_runtime() -> SqliteTaxonomy {
    let path = crate::installed::path().expect("taxonomy must be selected before matching");
    let db = crate::installed::open_read_only(&path).expect("selected taxonomy must be readable");
    SqliteTaxonomy::from_connection(db).expect("selected taxonomy queries must be available")
}

#[cfg(all(test, not(target_arch = "wasm32")))]
fn default_concepts() -> Vec<UnifiedConceptDefinition> {
    crate::taxonomy_sqlite::read_bundled_unified_taxonomy().expect("bundled unified taxonomy SQLite must be readable")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_index::validate;

    #[test]
    fn catalog_concept_identity_survives_renaming_and_reparenting() {
        let before = UnifiedTaxonomy::from_concepts(vec![concept(1, "Original parent", &[], &[]), concept(2, "Original label", &[1], &[])]).unwrap();
        let after = UnifiedTaxonomy::from_concepts(vec![concept(3, "New parent", &[], &[]), concept(2, "New label", &[3], &[])]).unwrap();
        assert_eq!(concept_for_path(&before.runtime, "concept:2"), Some(2));
        assert_eq!(concept_for_path(&after.runtime, "concept:2"), Some(2));
        assert_eq!(concept_for_path(&after.runtime, "Original parent / Original label"), None);
        for unknown in ["concept:999", "concept:0", "concept:-1", "concept:invalid"] {
            assert_eq!(concept_for_path(&after.runtime, unknown), None);
        }
    }

    #[test]
    fn most_specific_assignments_keep_divergent_branches_and_shared_routes() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Literature", &[], &[]),
            concept(2, "English", &[1], &[]),
            concept(3, "English periods", &[2], &[]),
            concept(4, "Mystery", &[1], &[]),
            concept(5, "History", &[], &[]),
            concept(6, "Shared topic", &[3, 5], &[]),
        ])
        .unwrap();
        assert_eq!(most_specific_concept_ids_in(taxonomy.runtime.as_ref(), [1, 2, 3, 4, 5, 3]), BTreeSet::from([3, 4, 5]));
        assert_eq!(most_specific_concept_ids_in(taxonomy.runtime.as_ref(), [1, 2, 3, 4, 5, 6]), BTreeSet::from([4, 6]));
        assert_eq!(most_specific_concept_ids_in(taxonomy.runtime.as_ref(), [2, 4]), BTreeSet::from([2, 4]));
        assert_eq!(most_specific_concept_ids_in(taxonomy.runtime.as_ref(), [2]), BTreeSet::from([2]));
        assert_eq!(most_specific_concept_ids_in(taxonomy.runtime.as_ref(), []), BTreeSet::new());
        // Unknown identities must still reach normal database validation.
        assert_eq!(most_specific_concept_ids_in(taxonomy.runtime.as_ref(), [0, 2]), BTreeSet::from([0, 2]));
    }

    #[test]
    fn compact_indexes_preserve_shared_ancestry_and_exact_selector_conflicts() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Root", &[], &[]),
            concept(2, "Branch", &[1], &["QA10"]),
            concept(3, "Other branch", &[1], &[]),
            concept(4, "Shared leaf", &[2, 3], &["QA10", "QA10.0"]),
            concept(5, "Unrelated", &[], &["QA10"]),
        ])
        .unwrap();
        assert_eq!(taxonomy.runtime.ancestors(4), &[1, 2, 3]);
        assert_eq!(taxonomy.match_candidates(LCC_SYSTEM_ID, "QA10"), vec![4, 5]);
        assert_eq!(taxonomy.lcc_selector_conflicts()["QA10"], vec![4, 5]);

        let bytes = bincode::serde::encode_to_vec(taxonomy.runtime.as_ref(), bincode::config::standard()).unwrap();
        let (decoded, consumed): (UnifiedTaxonomyRuntime, usize) = bincode::serde::decode_from_slice(&bytes, bincode::config::standard()).unwrap();
        assert_eq!(consumed, bytes.len());
        assert_eq!(&decoded, taxonomy.runtime.as_ref());
        let restored = UnifiedTaxonomy { runtime: Arc::new(decoded) };
        assert_eq!(restored.match_candidates(LCC_SYSTEM_ID, "QA10"), vec![4, 5]);
        assert_eq!(restored.lcc_selector_conflicts(), taxonomy.lcc_selector_conflicts());
    }

    #[test]
    fn reviewed_history_ranges_cover_their_complete_boundaries() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![concept(10305, "Eastern campaigns", &[], &["D551..D552.Z"]), concept(54459, "Kazakh political history", &[], &["DK908.67..DK908.68", "DK908.682"])]).unwrap();
        for code in ["D551", "D552.A1", "D552.L5", "D552.T3", "D552.Z9"] {
            assert_eq!(taxonomy.match_candidates(LCC_SYSTEM_ID, code), vec![10305], "{code}");
        }
        for code in ["D550", "D553", "DK908.681", "DK908.69"] {
            assert!(taxonomy.match_candidates(LCC_SYSTEM_ID, code).is_empty(), "{code}");
        }
        for code in ["DK908.67", "DK908.68", "DK908.682"] {
            assert_eq!(taxonomy.match_candidates(LCC_SYSTEM_ID, code), vec![54459], "{code}");
        }
    }

    #[test]
    fn malformed_stored_boundaries_are_rejected_without_truncation() {
        for selector in ["D551..D552.A-Z", "DK908.67..DK908.68.2"] {
            let result = UnifiedTaxonomy::from_concepts(vec![concept(1, "Malformed", &[], &[selector])]);
            assert!(result.err().unwrap().contains("invalid LCC range end"), "{selector}");
        }
    }

    #[test]
    fn every_bundled_range_preserves_structured_boundary_semantics() {
        for concept in default_concepts() {
            for selector in concept.source_selectors(LCC_SYSTEM_ID) {
                let Some((start, end)) = selector.split_once("..") else { continue };
                for (text, upper) in [(start, false), (end, true)] {
                    let parsed = super::super::lcc::lcc_endpoint(text, upper).expect("stored boundary must parse completely");
                    let cutters = cutter_components(&parsed.remainder);
                    let mut formatted = parsed.letters.clone();
                    if parsed.code != parsed.letters {
                        formatted.push_str(&parsed.number.to_string());
                    }
                    for (letters, fraction) in &cutters {
                        formatted.push('.');
                        formatted.push_str(letters);
                        formatted.push_str(fraction);
                    }
                    let restored = super::super::lcc::lcc_endpoint(&formatted, upper).unwrap();
                    assert_eq!(parsed.letters, restored.letters, "{text}");
                    assert_eq!(parsed.number, restored.number, "{text}");
                    assert_eq!(cutters, cutter_components(&restored.remainder), "{text}");
                }
            }
        }
    }

    #[test]
    fn sqlite_matches_compiled_taxonomy_selectors_ranges_and_navigation() {
        let rebuilt = build_runtime(default_concepts()).expect("compile bundled definitions");
        let sqlite = default_runtime();
        assert_eq!(sqlite.revision_id, rebuilt.revision_id);
        assert_eq!(sqlite.routes(), rebuilt.routes.values().collect::<Vec<_>>());
        let mut checked = 0;
        for (system, index) in rebuilt.selectors.iter() {
            let mut codes = index.exact.keys().cloned().collect::<BTreeSet<_>>();
            for (prefix, _) in &index.prefixes {
                codes.insert(format!("{prefix}10"));
            }
            for range in &index.lcc_ranges {
                for (letters, number, cutters) in [(&range.start_letters, range.start_number, &range.start_cutters), (&range.end_letters, range.end_number, &range.end_cutters)] {
                    if number.is_finite() {
                        let tail = cutters.iter().map(|(letters, digits)| format!(".{letters}{digits}")).collect::<String>();
                        codes.insert(format!("{letters}{number}{tail}"));
                    }
                }
            }
            for code in codes {
                assert_eq!(matching_candidates(&sqlite, system, &code), matching_candidates(&rebuilt, system, &code), "{system}:{code}");
                checked += 1;
            }
        }
        for route in rebuilt.routes.values() {
            assert_eq!(sqlite.concept_for_path(&route.path), Some(route.concept_id), "{}", route.path);
        }
        eprintln!("Compared {checked} selector/range lookups and all navigation routes");
    }

    #[test]
    fn sqlite_preserves_cutters_ambiguity_recovery_and_multiple_parents() {
        let definitions = vec![
            concept(1, "Root", &[], &["QA1..QB100"]),
            concept(2, "Branch", &[1], &["QA10..QA99", "QA20.A1", "QA30"]),
            concept(3, "Other root", &[], &["QA30"]),
            concept(4, "Leaf / literal separator", &[2, 3], &["QA40.A..QA40.Z", "QA50*"]),
            concept(5, "Nested", &[4], &["QA40.L37..QA40.L379"]),
            concept(7, "Taxonomy-only subclass", &[], &["FC1..FC200"]),
            UnifiedConceptDefinition::new(6, "Private system".into(), vec![], BTreeMap::from([("private".into(), vec!["A*".into(), "ABC".into()])])),
        ];
        let runtime = build_runtime(definitions.clone()).unwrap();
        let sqlite = SqliteTaxonomy::from_concepts(definitions).unwrap();
        for code in ["QA", "QB20", "QA9", "QA10", "QA99", "QA100", "QA20.A1 2025", "QA20.A2", "QA30", "QA40.L37 A9", "QA40.Z", "QA50.9", "QA20; QB20", "QA20QB20", "QA20 (v. 2)", "QA20 c.1", "未分類 QA20", "QA20 YEAR 2020"] {
            assert_eq!(matching_candidates(&sqlite, LCC_SYSTEM_ID, code), matching_candidates(&runtime, LCC_SYSTEM_ID, code), "{code}");
            assert_eq!(lcc_match_report(&sqlite, code), lcc_match_report(&runtime, code), "{code}");
        }
        assert_eq!(matching_candidates(&sqlite, LCC_SYSTEM_ID, "FC123 .A1 2001 F"), BTreeSet::from([7]));
        // QAA is syntactically valid and lies within QA..QB, but the selected
        // taxonomy does not declare it as a class. Item suffixes cannot bypass
        // that boundary now that parsing is independent of reference data.
        for code in ["QAA20 .A1 2001 F", "QAA20 .A1 1999-2000", "QAA20 .A1 (SET)", "QAA20 .A1 2001EB"] {
            assert!(matching_candidates(&sqlite, LCC_SYSTEM_ID, code).is_empty(), "{code}");
            assert!(matching_candidates(&runtime, LCC_SYSTEM_ID, code).is_empty(), "{code}");
        }
        for route in runtime.routes.values() {
            assert_eq!(sqlite.concept_for_path(&route.path), Some(route.concept_id));
            assert_eq!(sqlite.paths_for_concept(route.concept_id), runtime.routes.paths_for_concept(route.concept_id).collect::<Vec<_>>());
        }
        for ids in [vec![1, 2, 3, 4, 5], vec![0, 2, 3], vec![]] {
            assert_eq!(most_specific_concept_ids_in(&sqlite, ids.clone()), most_specific_concept_ids_in(&runtime, ids));
        }
        assert_eq!(sqlite.concept_for_path("concept:4"), Some(4));
        assert_eq!(sqlite.concept_for_path("concept:999"), None);
        for code in ["A", "ABC", "AXYZ", "other"] {
            assert_eq!(matching_candidates(&sqlite, "private", code), matching_candidates(&runtime, "private", code));
        }
    }

    #[test]
    fn sqlite_preserves_canonical_ownership_when_display_paths_collide() {
        let definitions = vec![concept(1, "A", &[], &[]), concept(2, "A / B", &[], &["QA20"]), concept(3, "B", &[1], &["QA10"]), concept(4, "Child", &[3], &["QA30"])];
        let runtime = build_runtime(definitions.clone()).unwrap();
        let sqlite = SqliteTaxonomy::from_concepts(definitions).unwrap();
        assert_eq!(sqlite.routes(), runtime.routes.values().collect::<Vec<_>>());
        assert_eq!(sqlite.find_path("A / B"), Some(2));
        assert_eq!(sqlite.concept_for_path("concept:3"), None);
        assert!(sqlite.paths_for_concept(3).is_empty());
        for path in ["A", "A / B", "A / B / Child", "concept:1", "concept:2", "concept:3", "concept:4"] {
            assert_eq!(sqlite.concept_for_path(path), concept_for_path(&runtime, path), "{path}");
        }
        for id in 1..=4 {
            assert_eq!(sqlite.paths_for_concept(id), runtime.routes.paths_for_concept(id).collect::<Vec<_>>());
        }
    }

    #[test]
    fn sqlite_preserves_ancestry_without_a_canonical_navigation_route() {
        let definitions = vec![concept(1, "A", &[], &[]), concept(2, "A / B", &[], &["QA20"]), concept(3, "B", &[1], &["QA10"])];
        let runtime = build_runtime(definitions.clone()).unwrap();
        let sqlite = SqliteTaxonomy::from_concepts(definitions).unwrap();
        assert!(runtime.routes.paths_for_concept(3).next().is_none());
        assert_eq!(most_specific_concept_ids_in(&sqlite, [1, 3]), BTreeSet::from([3]));
        for ids in [vec![1, 3], vec![1, 2, 3], vec![3], vec![1, 999], vec![]] {
            assert_eq!(most_specific_concept_ids_in(&sqlite, ids.clone()), most_specific_concept_ids_in(&runtime, ids));
        }
    }

    #[test]
    fn sqlite_path_lookup_requires_the_complete_literal_path() {
        let definitions = vec![concept(1, "Root", &[], &[]), concept(2, "Leaf", &[1], &["QA10"]), concept(3, "Label / separator", &[1], &["QA20"]), concept(4, "Subject", &[], &["QA30"])];
        let runtime = build_runtime(definitions.clone()).unwrap();
        let sqlite = SqliteTaxonomy::from_concepts(definitions).unwrap();
        for path in ["Root", "Root / Leaf", "Root / Label / separator", "Subject"] {
            assert_eq!(sqlite.concept_for_path(path), concept_for_path(&runtime, path));
            for invalid in [format!("{path} / "), format!("{path} /"), format!("{path} "), format!(" {path}")] {
                assert_eq!(sqlite.find_path(&invalid), None, "{invalid:?}");
                assert_eq!(sqlite.concept_for_path(&invalid), None, "{invalid:?}");
            }
        }
        assert_eq!(sqlite.routes(), runtime.routes.values().collect::<Vec<_>>());
    }

    #[test]
    fn sqlite_numeric_intervals_preserve_closed_endpoints_gaps_and_cross_subclasses() {
        let definitions = vec![
            concept(1, "Root", &[], &[]),
            concept(2, "First", &[1], &["QA1..QA2", "QA8..QA8.5"]),
            concept(3, "Adjacent", &[1], &["QA2..QA3"]),
            concept(4, "Point", &[1], &["QA5..QA5"]),
            concept(5, "Cutters", &[1], &["QA7.A..QA7.Z"]),
            concept(6, "Cross subclass", &[1], &["QA9..QB2"]),
            concept(7, "Whole subclasses", &[1], &["QC..QD"]),
            concept(8, "Empty reversed range", &[1], &["QA10..QA9", "QF..QE"]),
        ];
        let runtime = build_runtime(definitions.clone()).unwrap();
        let sqlite = SqliteTaxonomy::from_concepts(definitions).unwrap();
        let keys = |ranges: Vec<LccSelector>| ranges.into_iter().map(|range| format!("{range:?}")).collect::<BTreeSet<_>>();
        for letters in ["QA", "QB", "QC", "QD"] {
            for boundary in [0.0_f64, 1.0, 2.0, 3.0, 5.0, 7.0, 8.0, 8.5, 9.0, 100.0, f64::MAX] {
                for number in [boundary.next_down().max(0.0), boundary, boundary.next_up()] {
                    if number.is_finite() {
                        assert_eq!(keys(sqlite.ranges(letters, number)), keys(runtime.ranges(letters, number)), "{letters}{number}");
                    }
                }
            }
        }
    }

    #[test]
    fn sqlite_numeric_candidates_match_every_bundled_boundary_and_gap() {
        let runtime = build_runtime(default_concepts()).unwrap();
        let sqlite = default_runtime();
        let index = &runtime.selectors[LCC_SYSTEM_ID];
        let keys = |ranges: Vec<LccSelector>| ranges.into_iter().map(|range| format!("{range:?}")).collect::<BTreeSet<_>>();
        let mut checked = 0;
        for letters in &index.lcc_subclasses {
            let mut boundaries = vec![0.0_f64];
            for range in &index.lcc_ranges {
                if &range.start_letters == letters && range.start_number.is_finite() {
                    boundaries.push(range.start_number);
                }
                if &range.end_letters == letters && range.end_number.is_finite() {
                    boundaries.push(range.end_number);
                }
            }
            boundaries.sort_unstable_by(f64::total_cmp);
            boundaries.dedup();
            let mut points = boundaries.iter().flat_map(|&number| [number.next_down().max(0.0), number, number.next_up()]).filter(|number| number.is_finite()).collect::<Vec<_>>();
            points.extend(boundaries.windows(2).map(|pair| pair[0] + (pair[1] - pair[0]) / 2.0));
            points.sort_unstable_by(f64::total_cmp);
            points.dedup();
            for number in points {
                assert_eq!(keys(sqlite.ranges(letters, number)), keys(runtime.ranges(letters, number)), "{letters}{number}");
                checked += 1;
            }
        }
        eprintln!("Compared {checked} numeric coverage probes across {} subclasses", index.lcc_subclasses.len());
    }

    #[cfg(feature = "authoritative-taxonomy")]
    #[test]
    fn bundled_computer_science_calls_do_not_fall_through_to_mathematics() {
        let taxonomy = UnifiedTaxonomy::from_concepts(default_concepts()).unwrap();
        for code in ["QA75.5", "QA75.51", "QA76.76", "QA76.76 .F634 2005", "QA76.94", "QA76.95"] {
            let ids = taxonomy.matching_concept_ids(LCC_SYSTEM_ID, code);
            assert!(!ids.is_empty(), "{code}");
            for id in ids {
                assert!(id == 36 || taxonomy.runtime.ancestors(id).contains(&36), "{code} must resolve under Computing");
                assert!(id != 4916 && !taxonomy.runtime.ancestors(id).contains(&4916), "{code} must not resolve under Mathematics");
            }
        }
        assert_eq!(taxonomy.matching_concept_ids(LCC_SYSTEM_ID, "QA76.76.D47 F634 2005"), vec![168]);
        // The new interval must not take adjacent mathematics classifications.
        for code in ["QA75.49", "QA77", "QA99", "QA699"] {
            let ids = taxonomy.matching_concept_ids(LCC_SYSTEM_ID, code);
            assert!(!ids.is_empty(), "{code}");
            assert!(ids.iter().all(|id| *id == 4916 || taxonomy.runtime.ancestors(*id).contains(&4916)), "{code} must remain under Mathematics");
        }
    }

    #[test]
    fn nested_cutter_intervals_choose_the_more_specific_subject() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "County", &[], &["DA670.C8..DA670.C89"]),
            concept(2, "Hills", &[], &["DA670.C83..DA670.C839"]),
            concept(3, "Local history", &[], &["DS54.95.A..DS54.95.Z"]),
            concept(4, "City", &[], &["DS54.95.L37..DS54.95.L379"]),
        ])
        .unwrap();
        assert_eq!(taxonomy.match_candidates(LCC_SYSTEM_ID, "DA670.C83 S35x"), vec![2]);
        assert_eq!(taxonomy.match_candidates(LCC_SYSTEM_ID, "DA670.C85 .A1"), vec![1]);
        assert_eq!(taxonomy.match_candidates(LCC_SYSTEM_ID, "DS54.95.L373 A58 2011"), vec![4]);
    }

    #[test]
    fn cutter_interval_ties_and_partial_overlaps_remain_ambiguous() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "First", &[], &["QA1.C2..QA1.C6"]),
            concept(2, "Second", &[], &["QA1.C4..QA1.C8"]),
            concept(3, "Equal first", &[], &["QA2.C8..QA2.C89"]),
            concept(4, "Equal second", &[], &["QA2.C80..QA2.C890"]),
        ])
        .unwrap();
        assert_eq!(taxonomy.match_candidates(LCC_SYSTEM_ID, "QA1.C5"), vec![1, 2]);
        assert_eq!(taxonomy.match_candidates(LCC_SYSTEM_ID, "QA2.C83"), vec![3, 4]);
    }

    #[test]
    fn equivalent_range_labels_do_not_choose_between_conflicting_subjects() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![concept(17, "Mathematics", &[], &["QA1-939"]), concept(19, "Other", &[], &["QA1-QA939"])]).unwrap();
        assert!(taxonomy.matching_concept_ids("lcc", "QA500").is_empty());
    }

    #[test]
    fn incoming_ranges_require_full_containment_and_keep_individual_specificity() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Mathematics", &[], &["QA1..QA939"]),
            concept(2, "Computing", &[1], &["QA75.5..QA76.95"]),
            concept(3, "Programming", &[2], &["QA76.6..QA76.66"]),
            concept(4, "Exact topic", &[3], &["QA76.65"]),
        ])
        .unwrap();
        assert_eq!(taxonomy.match_candidates(LCC_SYSTEM_ID, "QA76.65 .R3 2024"), vec![4]);
        assert!(taxonomy.match_candidates(LCC_SYSTEM_ID, "QA75.5-76.95").is_empty());
        assert!(taxonomy.match_candidates(LCC_SYSTEM_ID, "QA76.6-76.66").is_empty());
        assert!(taxonomy.match_candidates(LCC_SYSTEM_ID, "QA76.65-76.65").is_empty());
        assert!(taxonomy.match_candidates(LCC_SYSTEM_ID, "QA900-950").is_empty());
        assert!(taxonomy.match_candidates(LCC_SYSTEM_ID, "QA950-900").is_empty());
    }

    #[test]
    fn incoming_ranges_do_not_bridge_gaps_or_cut_through_cutter_boundaries() {
        let taxonomy =
            UnifiedTaxonomy::from_concepts(vec![concept(1, "Separated coverage", &[], &["QA1..QA4", "QA8..QA10"]), concept(2, "Cutter boundary", &[], &["QB1.A1..QB10.C5"]), concept(3, "Structural subclass", &[], &["QC", "QC*"])]).unwrap();
        assert!(taxonomy.match_candidates(LCC_SYSTEM_ID, "QA3-9").is_empty());
        assert!(taxonomy.match_candidates(LCC_SYSTEM_ID, "QB1-9").is_empty());
        assert!(taxonomy.match_candidates(LCC_SYSTEM_ID, "QB2-10").is_empty());
        assert!(taxonomy.match_candidates(LCC_SYSTEM_ID, "QB2-9").is_empty());
        assert!(taxonomy.match_candidates(LCC_SYSTEM_ID, "QC2-9").is_empty());
        assert!(taxonomy.match_candidates(LCC_SYSTEM_ID, "QX2-9").is_empty());
    }

    #[test]
    fn equally_specific_lcc_matches_are_unresolved() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![concept(3, "Management", &[], &["HD69"]), concept(12, "Consultants", &[3], &["HD69.C6"]), concept(23, "Consulting", &[3], &["HD 69 .C6"])]).unwrap();
        assert!(taxonomy.matching_concept_ids("lcc", "HD69.C6 R38").is_empty());
        assert_eq!(taxonomy.match_candidates(LCC_SYSTEM_ID, "HD69.C6 R38"), vec![12, 23]);
        assert_eq!(taxonomy.lcc_selector_conflicts()["HD69.C6"], vec![12, 23]);
    }

    #[test]
    fn specificity_unrelated_ties_are_unresolved_for_every_code_system() {
        let mut definitions = vec![concept(15, "Left", &[], &["QA1..QA10"]), concept(21, "Right", &[], &["QA1..QA10"])];
        for definition in &mut definitions {
            definition.source_selectors.insert("bisac".into(), vec!["COM000000".into()]);
        }
        let taxonomy = UnifiedTaxonomy::from_concepts(definitions).unwrap();
        assert!(taxonomy.matching_concept_ids("lcc", "QA5.A1").is_empty());
        assert_eq!(taxonomy.match_candidates(LCC_SYSTEM_ID, "QA5.A1"), vec![15, 21]);
        assert!(taxonomy.matching_concept_ids("bisac", "COM000000").is_empty());
    }

    #[test]
    fn specificity_uses_all_parent_paths_and_not_absolute_depth() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "First root", &[], &["QA10"]),
            concept(2, "Second root", &[], &["QA10"]),
            concept(3, "Shared child", &[1, 2], &["QA10"]),
            concept(4, "Other root", &[], &[]),
            concept(5, "Other child", &[4], &[]),
            concept(6, "Unrelated deeper topic", &[5], &["QA10"]),
        ])
        .unwrap();
        assert_eq!(taxonomy.match_candidates(LCC_SYSTEM_ID, "QA10"), vec![3, 6]);
        assert!(taxonomy.matching_concept_ids("lcc", "QA10").is_empty());
    }

    #[test]
    fn specificity_bisac_descendants_win_and_unrelated_ties_remain_diagnostic() {
        let mut definitions = vec![concept(1, "Parent", &[], &[]), concept(2, "Child", &[1], &[])];
        for definition in &mut definitions {
            definition.source_selectors.insert("bisac".into(), vec!["COM000000".into()]);
        }
        let taxonomy = UnifiedTaxonomy::from_concepts(definitions.clone()).unwrap();
        assert_eq!(taxonomy.matching_concept_ids("bisac", "COM000000"), vec![2]);
        let mut unrelated = concept(3, "Unrelated", &[], &[]);
        unrelated.source_selectors.insert("bisac".into(), vec!["COM000000".into()]);
        definitions.push(unrelated);
        let taxonomy = UnifiedTaxonomy::from_concepts(definitions).unwrap();
        assert!(taxonomy.matching_concept_ids("bisac", "COM000000").is_empty());
        assert_eq!(taxonomy.match_candidates("bisac", "COM000000"), vec![2, 3]);
    }

    #[test]
    fn printed_paren_range_spellings_are_rejected() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![concept(1, "Comparative law", &[], &["K520..K5582"]), concept(2, "Treaties", &[1], &["K524..K525"])]).unwrap();
        // Parenthesized range spellings are below the coverage threshold: rejected, not expanded.
        assert!(taxonomy.matching_concept_ids("lcc", "K(520) 5582").is_empty());
        assert!(taxonomy.matching_concept_ids("lcc", "K(520)-5582").is_empty());
        assert!(taxonomy.matching_concept_ids("lcc", "K(521) 5582").is_empty());
    }

    #[test]
    fn clipped_tail_recovery_does_not_merge_away_complete_subjects() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![concept(1, "Epistemology", &[], &["BD143..BD237"]), concept(2, "Psychology", &[], &["BF1..BF990"]), concept(3, "Logic", &[], &["BC1..BC999"])]).unwrap();
        let report = taxonomy.lcc_match_report("BD143-237BF201Q342BC");
        assert_eq!(report.assignments.iter().map(|a| a.concept_id).collect::<Vec<_>>(), [2], "{report:?}");
        assert_eq!(report.unresolved_fragments, ["BD143-237", "BC"]);
    }

    #[test]
    fn partial_lists_recover_complete_unicode_and_marc_members() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![concept(1, "Computing", &[], &["QA75..QA76.99"])]).unwrap();
        for raw in ["QA76; 未分類"] {
            let report = taxonomy.lcc_match_report(raw);
            assert_eq!(report.assignments.iter().map(|a| a.concept_id).collect::<Vec<_>>(), [1], "{report:?}");
            assert!(!report.unresolved_fragments.is_empty());
            assert!(report.assignments[0].evidence.iter().all(|e| raw.contains(&e.code)));
        }
        assert!(taxonomy.lcc_match_report("未分類 QA76").assignments.is_empty());
    }

    fn concept(id: i64, label: &str, parents: &[i64], lcc: &[&str]) -> UnifiedConceptDefinition {
        let source_selectors = if lcc.is_empty() { BTreeMap::new() } else { BTreeMap::from([(LCC_SYSTEM_ID.to_owned(), lcc.iter().map(|value| (*value).to_owned()).collect())]) };
        UnifiedConceptDefinition::new(id.to_owned(), label.to_owned(), parents.iter().map(|value| (*value).to_owned()).collect(), source_selectors)
    }

    #[test]
    fn topic_cutter_ranges_respect_decimal_bounds_and_filing_suffixes() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![concept(1, "Germany", &[], &["DD801..DD801"]), concept(2, "Saxony", &[1], &["DD801.S31..DD801.S46"]), concept(3, "Württemberg", &[1], &["DD801.W6..DD801.W82"])]).unwrap();
        for code in ["DD801.S31", "DD801.S4", "DD801.S46 .B33 2007", "DD801.S460 B33 2007"] {
            assert_eq!(taxonomy.match_candidates(LCC_SYSTEM_ID, code), vec![2], "{code}");
        }
        for code in ["DD801", "DD801.A28", "DD801.S3", "DD801.S469", "DD801.W9"] {
            assert_eq!(taxonomy.match_candidates(LCC_SYSTEM_ID, code), vec![1], "{code}");
        }
        assert_eq!(taxonomy.match_candidates(LCC_SYSTEM_ID, "DD801.W7 .B2 2005"), vec![3]);
    }

    #[test]
    fn placeholder_main_class_does_not_supply_a_subclass() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![concept(1, "Language and literature", &[], &["(P)"]), concept(2, "Kannada author", &[1], &["PL4659.L33"])]).unwrap();
        assert_eq!(taxonomy.match_candidates(LCC_SYSTEM_ID, "MLCxxx (P)"), vec![1]);
        assert_eq!(taxonomy.match_candidates(LCC_SYSTEM_ID, "PL4659.L33"), vec![2]);
        assert!(taxonomy.match_candidates(LCC_SYSTEM_ID, "MLCxxx").is_empty());
    }

    #[test]
    fn single_pz_title_letters_preserve_cutter_bounds_and_exact_evidence() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![concept(1, "Children's literature", &[], &["PZ7..PZ7"]), concept(2, "Author group", &[1], &["PZ7.S26..PZ7.S27"]), concept(3, "Exact original", &[2], &["PZ7.S268 E"])]).unwrap();
        assert_eq!(taxonomy.match_candidates(LCC_SYSTEM_ID, "PZ7.S268 E"), vec![3]);
        assert_eq!(taxonomy.match_candidates(LCC_SYSTEM_ID, "PZ7.S268 F"), vec![2]);
        assert_eq!(taxonomy.match_candidates(LCC_SYSTEM_ID, "PZ7.S28 E"), vec![1]);
    }

    #[test]
    fn item_shaped_list_tails_do_not_create_spurious_subjects() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![concept(1, "Psychoanalysis", &[], &["BF175..BF175"]), concept(2, "Philosophy", &[], &["B1..B100"])]).unwrap();
        assert_eq!(taxonomy.matching_concept_ids(LCC_SYSTEM_ID, "BF175; B43"), vec![1]);
        assert_eq!(taxonomy.matching_concept_ids(LCC_SYSTEM_ID, "BF175 B43"), vec![1]);
    }

    #[test]
    fn validation_rejects_parallel_taxonomy_structures() {
        let redundant_wrapper = vec![concept(22, "History", &[], &[]), concept(8, "History.", &[22], &[])];
        assert!(validate(&redundant_wrapper).unwrap_err().contains("direct parent"));

        let duplicate_siblings = vec![concept(22, "Humanities", &[], &[]), concept(15, "By period", &[22], &[]), concept(21, "By Period.", &[22], &[])];
        assert!(validate(&duplicate_siblings).unwrap_err().contains("beneath parent"));

        let duplicate_source_record = vec![concept(22, "Humanities", &[], &[]), concept(15, "Ethnography", &[22], &["GN301"]), concept(20, "Social Sciences", &[], &[]), concept(21, "Ethnography.", &[20], &["GN301"])];
        assert!(validate(&duplicate_source_record).unwrap_err().contains("duplicate source selector"));
    }

    #[test]
    fn pz_work_marks_match_author_exceptions_and_numeric_ranges() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![concept(1, "Juvenile fiction", &[], &["PZ5..PZ10.5"]), concept(2, "Author collection", &[1], &["PZ7.R79835"])]).unwrap();
        assert_eq!(taxonomy.matching_concept_ids("lcc", "PZ7.R79835 Ham 1999"), vec![2]);
        assert_eq!(taxonomy.matching_concept_ids("lcc", "PZ7.T47 Wh"), vec![1]);
        assert_eq!(taxonomy.matching_concept_ids("lcc", "PZ7.R79835 BOX NO. 4"), vec![2]);
    }

    #[test]
    fn volume_ranges_do_not_become_classification_ranges_or_cutters() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Mathematics", &[], &["QA1..QA999"]),
            concept(2, "Series", &[1], &["QA3.L28"]),
            concept(3, "Bounded author", &[], &["QB1.A1..QB1.C5"]),
            concept(4, "Complete item", &[2], &["QA3.L28 VOL. 1"]),
        ])
        .unwrap();
        for (code, id) in [("QA3 .L28 vol. 1-3", 2), ("QA3.L28 vol. 1", 4), ("QA4.A1 no. 2", 1), ("QB1.C5 vol. 1-3", 3), ("QA3.L28 Suppl. 2", 2)] {
            assert_eq!(taxonomy.matching_concept_ids("lcc", code), vec![id], "{code}");
        }
        assert!(taxonomy.matching_concept_ids("lcc", "QB1.D1 vol. 1").is_empty());
    }

    #[test]
    fn custodial_assignments_do_not_replace_subjects_or_author_matches() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![concept(1, "Literature", &[], &["PL2000..PL3000"]), concept(2, "Author", &[1], &["PL2780.F4"])]).unwrap();
        for code in ["PL2780.F4 1981b <Orien China>", "PL2780.F4 vol. 2 <Asian Japan>"] {
            assert_eq!(taxonomy.matching_concept_ids("lcc", code), vec![2]);
        }
        assert_eq!(taxonomy.matching_concept_ids("lcc", "PL2800.A1 <Hebr>"), vec![1]);
        assert_eq!(taxonomy.matching_concept_ids("lcc", "PL2780.F4 <UNKNOWN>"), vec![2]);
    }
}
