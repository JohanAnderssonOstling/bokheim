use super::lcc::{canonical_lcc_notation, lcc_match_keys, compare_lcc_key, lcc_span, lcc_endpoint, parse_lcc_call};
use super::{compact_whitespace, DDC_SYSTEM_ID, LCC_SYSTEM_ID, ROOT_SUBJECT_PATH, UNIFIED_VERSION};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, OnceLock, RwLock};

/// The immutable taxonomy bundled locally in every ordinary client build.
/// Explicit builds without default features can produce a reduced seed.
pub const DEFAULT_UNIFIED_TAXONOMY_SQLITE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/unified-taxonomy-client-v2.sqlite3"));
/// Release identifier of the bundled taxonomy image. Keep this cheap constant
/// separate from the runtime so an already-projected library can be opened
/// without materializing the complete taxonomy matcher.
pub const BUNDLED_UNIFIED_TAXONOMY_REVISION_ID: &str = include_str!(concat!(env!("OUT_DIR"), "/unified-taxonomy-revision-id.txt"));

static RUNTIME: OnceLock<RwLock<Arc<UnifiedTaxonomyRuntime>>> = OnceLock::new();

pub(super) struct UnifiedTaxonomyRuntime {
    /// Persisted overlays retain their definitions so another overlay can be
    /// composed in memory. The immutable bundled taxonomy discards this
    /// redundant source representation after building its lookup indexes.
    concepts: Option<Arc<Vec<UnifiedConceptDefinition>>>,
    pub(super) routes: Arc<BTreeMap<String, UnifiedConceptRoute>>,
    pub(super) paths_by_concept: Arc<BTreeMap<i64, Vec<String>>>,
    selectors: Arc<BTreeMap<String, SelectorIndex>>,
    ancestors: Arc<BTreeMap<i64, BTreeSet<i64>>>,
    pub(super) source_labels: Arc<BTreeMap<String, BTreeSet<i64>>>,
    revision_id: String,
}

/// An owned, immutable taxonomy matcher suitable for servers and tools that
/// must not replace the process-wide client taxonomy.
#[derive(Clone)]
pub struct UnifiedTaxonomy {
    runtime: Arc<UnifiedTaxonomyRuntime>,
}

impl UnifiedTaxonomy {
    pub fn from_concepts(concepts: Vec<UnifiedConceptDefinition>) -> Result<Self, String> {
        Ok(Self { runtime: Arc::new(build_runtime(concepts, false)?) })
    }

    pub fn revision_id(&self) -> &str {
        &self.runtime.revision_id
    }

    pub fn matching_concept_ids(&self, system_id: &str, code: &str) -> Vec<i64> {
        matching_concepts(&self.runtime, system_id, code).into_iter().collect()
    }

    /// Returns candidates for one code, or independently resolved subjects for an explicit LCC list.
    pub fn match_candidates(&self, system_id: &str, code: &str) -> Vec<i64> {
        matching_candidates(&self.runtime, system_id, code).into_iter().collect()
    }

    /// Returns candidates for one code, or independently resolved subjects for an explicit list.
    /// Resolved subjects, recovery methods and fragments that still need review.
    pub fn lcc_match_report(&self, code: &str) -> crate::LccMatchReport {
        lcc_match_report(&self.runtime, code)
    }

    pub fn lcc_match_candidates(&self, code: &str) -> Vec<i64> {
        matching_candidates(&self.runtime, LCC_SYSTEM_ID, code).into_iter().collect()
    }

    /// Unrelated exact-selector conflicts require curation; ancestors are shadowed.
    pub fn lcc_selector_conflicts(&self) -> BTreeMap<String, Vec<i64>> {
        self.runtime.selectors.get(LCC_SYSTEM_ID).map(|index| {
            index.exact.iter().filter_map(|(code, ids)| {
                let shadowed = ids.iter().flat_map(|id| self.runtime.ancestors[id].iter().copied()).collect::<BTreeSet<_>>();
                let specific = ids.difference(&shadowed).copied().collect::<Vec<_>>();
                (specific.len() > 1).then(|| (code.clone(), specific))
            }).collect()
        }).unwrap_or_default()
    }

    pub fn has_exact_source_code(&self, system_id: &str, code: &str) -> bool {
        has_exact_source_code(&self.runtime, system_id, code)
    }
}

#[derive(Default)]
struct SelectorIndex {
    exact: HashMap<String, BTreeSet<i64>>,
    prefixes: Vec<(String, i64)>,
    lcc_ranges: Vec<LccSelector>,
    lcc_ranges_by_subclass: HashMap<String, Vec<usize>>,
    lcc_subclasses: BTreeSet<String>,
}
struct LccSelector {
    start_letters: String,
    start_number: f64,
    end_letters: String,
    end_number: f64,
    start_cutters: Vec<(String, String)>,
    end_cutters: Vec<(String, String)>,
    concept_id: i64,
}

// Preserve the historical interpretation of legacy stored endpoints. New
// endpoints are validated strictly by the SQLite writer and incoming parser.
fn stored_lcc_endpoint(value: &str, upper: bool) -> Option<super::lcc::LccEndpoint> {
    lcc_endpoint(value, upper).or_else(|| parse_lcc_call(value).map(|call| super::lcc::LccEndpoint {
        letters: call.letters, number: call.number, remainder: call.remainder.to_owned(), code: value.to_owned(),
    }))
}

// Cutter digits are decimal fractions. Compare only as many components as
// the schedule endpoint supplies; later author Cutters and dates are filing
// details within that endpoint, not reasons to exclude its books.
pub(super) fn cutter_components(remainder: &str) -> Vec<(String, String)> {
    let mut rest = remainder;
    let mut result = Vec::new();
    loop {
        rest = rest.trim_start_matches(|c: char| c.is_ascii_whitespace() || c == '.');
        let letters = rest.bytes().take_while(u8::is_ascii_alphabetic).count();
        if letters == 0 { break; }
        let label = rest[..letters].to_ascii_uppercase();
        rest = &rest[letters..];
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        let fraction = rest[..digits].trim_end_matches('0');
        result.push((label, if digits > 0 && fraction.is_empty() { "0".to_owned() } else { fraction.to_owned() }));
        rest = &rest[digits..];
    }
    result
}

pub(super) fn compare_cutter_endpoint(call: &[(String, String)], endpoint: &[(String, String)]) -> std::cmp::Ordering {
    for (index, (letters, digits)) in endpoint.iter().enumerate() {
        let Some((call_letters, call_digits)) = call.get(index) else { return std::cmp::Ordering::Less };
        let order = call_letters.cmp(letters).then_with(|| {
            if digits.is_empty() { std::cmp::Ordering::Equal } else { call_digits.cmp(digits) }
        });
        if !order.is_eq() { return order; }
    }
    std::cmp::Ordering::Equal
}

// An upper endpoint covers its whole filing family, including omitted deeper
// Cutters. A shorter endpoint is therefore broader, not earlier.
pub(super) fn compare_upper_cutters(inner: &[(String, String)], outer: &[(String, String)]) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    for ((a, digits_a), (b, digits_b)) in inner.iter().zip(outer) {
        let order = a.cmp(b);
        if !order.is_eq() { return order; }
        let order = match (digits_a.is_empty(), digits_b.is_empty()) {
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            _ => digits_a.cmp(digits_b),
        };
        if !order.is_eq() { return order; }
    }
    outer.len().cmp(&inner.len())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnifiedConceptRoute {
    concept_id: i64,
    path: String,
    parent_path: String,
    label: String,
    sort_order: i64,
}

impl UnifiedConceptRoute {
    pub fn concept_id(&self) -> i64 {
        self.concept_id
    }
    pub fn path(&self) -> &str {
        &self.path
    }
    pub fn parent_path(&self) -> &str {
        &self.parent_path
    }
    pub fn label(&self) -> &str {
        &self.label
    }
    pub fn sort_order(&self) -> i64 {
        self.sort_order
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnifiedConceptDefinition {
    concept_id: i64,
    preferred_label: String,
    parent_ids: Vec<i64>,
    source_selectors: BTreeMap<String, Vec<String>>,
    source_labels: Vec<String>,
}

impl UnifiedConceptDefinition {
    pub fn new(concept_id: i64, preferred_label: String, parent_ids: Vec<i64>, mut source_selectors: BTreeMap<String, Vec<String>>, source_labels: Vec<String>) -> Self {
        for (system, selectors) in &mut source_selectors {
            if system == LCC_SYSTEM_ID {
                for selector in selectors.iter_mut() {
                    *selector = crate::selector_format::canonical_lcc_selector(selector);
                }
                selectors.sort();
                selectors.dedup();
            } else {
                selectors.sort();
            }
        }
        Self { concept_id, preferred_label, parent_ids, source_selectors, source_labels }
    }
    pub fn concept_id(&self) -> i64 {
        self.concept_id
    }
    pub fn preferred_label(&self) -> &str {
        &self.preferred_label
    }
    pub fn parent_ids(&self) -> &[i64] {
        &self.parent_ids
    }

    pub fn source_selectors(&self, system_id: &str) -> &[String] {
        self.source_selectors.get(system_id).map(Vec::as_slice).unwrap_or_default()
    }
    pub fn source_labels(&self) -> &[String] {
        &self.source_labels
    }
}

pub(super) fn current() -> Arc<UnifiedTaxonomyRuntime> {
    RUNTIME.get_or_init(|| RwLock::new(default_runtime())).read().expect("unified taxonomy runtime lock must not be poisoned").clone()
}

pub fn unified_concept_definitions() -> Vec<UnifiedConceptDefinition> {
    let Some(runtime) = RUNTIME.get() else {
        return default_concepts();
    };
    runtime.read().ok().and_then(|runtime| runtime.concepts.as_deref().cloned()).unwrap_or_else(default_concepts)
}

pub fn install_unified_taxonomy_concepts(concepts: Vec<UnifiedConceptDefinition>) -> Result<(), String> {
    let replacement = Arc::new(build_runtime(concepts, true)?);
    let runtime = RUNTIME.get_or_init(|| RwLock::new(replacement.clone()));
    *runtime.write().map_err(|_| "unified taxonomy runtime lock is poisoned".to_owned())? = replacement;
    Ok(())
}

/// Rebuilds the runtime from the embedded client snapshot plus a persisted
/// on-demand overlay. This also removes concepts from an obsolete overlay.
pub fn install_unified_taxonomy_overlay(concepts: Vec<UnifiedConceptDefinition>) -> Result<(), String> {
    let mut merged = default_concepts().into_iter().map(|concept| (concept.concept_id().to_owned(), concept)).collect::<BTreeMap<_, _>>();
    for concept in concepts {
        merged.insert(concept.concept_id().to_owned(), concept);
    }
    install_unified_taxonomy_concepts(merged.into_values().collect())
}

pub fn unified_taxonomy_revision_id() -> String {
    current().revision_id.clone()
}

/// Returns the active content revision only when a matcher has already been built.
/// This is intentionally non-initializing for startup-sensitive callers.
pub fn loaded_unified_taxonomy_revision_id() -> Option<String> {
    RUNTIME.get().and_then(|runtime| runtime.read().ok().map(|runtime| runtime.revision_id.clone()))
}

pub fn unified_subject_paths(system_id: &str, code: &str) -> Vec<String> {
    let runtime = current();
    if system_id == crate::UNIFIED_SYSTEM_ID {
        return runtime.routes.contains_key(code).then(|| code.to_owned()).into_iter().collect();
    }
    matching_concepts(&runtime, system_id, code).into_iter().flat_map(|id| runtime.paths_by_concept.get(&id).cloned().unwrap_or_default()).collect::<BTreeSet<_>>().into_iter().collect()
}

/// True only when the installed taxonomy names this precise external code.
/// Falling inside a broader range is deliberately not exact coverage.
pub fn unified_has_exact_source_code(system_id: &str, code: &str) -> bool {
    has_exact_source_code(&current(), system_id, code)
}

fn has_exact_source_code(runtime: &UnifiedTaxonomyRuntime, system_id: &str, code: &str) -> bool {
    let Some(index) = runtime.selectors.get(system_id) else { return false };
    let code = compact_whitespace(code).to_ascii_uppercase();
    let code = if system_id == LCC_SYSTEM_ID { lcc_match_keys(&code).and_then(|keys| keys.last().cloned()).unwrap_or(code) } else { code };
    index.exact.contains_key(&code)
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
    current().routes.get(path).map(UnifiedConceptRoute::concept_id)
}

pub fn unified_subject_sort_order(path: &str) -> i64 {
    current().routes.get(path).map_or(i64::MAX, UnifiedConceptRoute::sort_order)
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct LccOrderKey {
    letters: String,
    whole: u64,
    fraction: String,
    cutters: Vec<(String, String)>,
}

fn lcc_order_key(selector: &str) -> Option<LccOrderKey> {
    let selector = selector.trim().to_ascii_uppercase();
    let start = selector.split("..").next()?.split('-').next()?.trim_end_matches('*').trim();
    if !start.is_empty() && start.len() <= 4 && start.bytes().all(|byte| byte.is_ascii_alphabetic()) {
        return Some(LccOrderKey { letters: start.to_owned(), whole: 0, fraction: String::new(), cutters: vec![] });
    }
    let call = parse_lcc_call(start)?;
    let number = start[..start.len() - call.remainder.len()].trim_start_matches(|c: char| c.is_ascii_alphabetic() || c.is_ascii_whitespace() || c == '\'');
    let (whole, fraction) = number.split_once('.').unwrap_or((number, ""));
    Some(LccOrderKey {
        letters: call.letters,
        whole: whole.parse().ok()?,
        fraction: fraction.trim_end_matches('0').to_owned(),
        cutters: cutter_components(call.remainder),
    })
}

/// Derived browse order, never persisted. Equal LCC keys receive the same
/// rank so callers can break ties by label. Subjects without LCC selectors
/// sort alphabetically after classified subjects.
pub fn lcc_subject_sort_orders(concepts: &[UnifiedConceptDefinition]) -> BTreeMap<i64, i64> {
    let keys = concepts.iter().map(|concept| {
        (concept.concept_id, concept.source_selectors(LCC_SYSTEM_ID).iter().filter_map(|selector| lcc_order_key(selector)).min())
    }).collect::<BTreeMap<_, _>>();
    let ranks = keys.values().flatten().cloned().collect::<BTreeSet<_>>().into_iter().enumerate()
        .map(|(rank, key)| (key, rank as i64)).collect::<BTreeMap<_, _>>();
    keys.into_iter().map(|(id, key)| (id, key.map_or(i64::MAX, |key| ranks[&key]))).collect()
}
pub fn unified_taxonomy_routes() -> Vec<UnifiedConceptRoute> {
    current().routes.values().cloned().collect()
}
pub fn unified_lcc_subject_paths(code: &str) -> Vec<String> {
    unified_subject_paths(LCC_SYSTEM_ID, code)
}

/// Whether the class number has numeric coverage in the local taxonomy.
/// A bare letter prefix (or a range spanning different subclasses) cannot
/// validate an extracted number: E3002 must not pass merely because E is known.
/// This is conservative coverage, not proof of an official item assignment.
pub fn unified_lcc_has_numeric_coverage(code: &str) -> bool {
    let Some(canonical) = canonical_lcc_notation(code) else { return false };
    let Some(call) = parse_lcc_call(&canonical) else { return false };
    let runtime = current();
    let Some(index) = runtime.selectors.get(LCC_SYSTEM_ID) else { return false };
    if lcc_match_keys(&canonical).is_some_and(|keys| keys.iter().any(|key| {
        index.exact.contains_key(key) && parse_lcc_call(key).is_some_and(|known| known.number == call.number)
    })) { return true; }
    index.lcc_ranges.iter().any(|range| range.start_letters == call.letters
        && range.end_letters == call.letters && range.start_number <= call.number
        && call.number <= range.end_number)
}

pub fn unified_lcc_match_report(code: &str) -> crate::LccMatchReport {
    lcc_match_report(&current(), code)
}

pub(super) fn lcc_match_report(runtime: &UnifiedTaxonomyRuntime, code: &str) -> crate::LccMatchReport {
    if strict_matching_concepts(runtime, LCC_SYSTEM_ID, code).is_empty()
        && matching_candidates(runtime, LCC_SYSTEM_ID, code).len() > 1 {
        return crate::LccMatchReport { original: code.to_owned(), assignments: Vec::new(), unresolved_fragments: vec![code.to_owned()] };
    }
    // Preserve the full call while interpreting malformed separated Cutters;
    // prefix recovery would otherwise detach the Cutter from its class.
    if strict_matching_concepts(runtime, LCC_SYSTEM_ID, code).is_empty() {
        let interpreted = ambiguous_lcc_matches(runtime, code);
        if !interpreted.is_empty() {
            return crate::LccMatchReport {
                original: code.to_owned(),
                assignments: interpreted.into_iter().map(|(concept_id, evidence)|
                    crate::LccMatchAssignment { concept_id, evidence: vec![evidence] }).collect(),
                unresolved_fragments: vec![code.to_owned()],
            };
        }
    }
    let mut report = crate::lcc_recovery::report(code,
        |part| strict_matching_concepts(runtime, LCC_SYSTEM_ID, part).into_iter().collect(),
        |letters| runtime.selectors.get(LCC_SYSTEM_ID).is_some_and(|index| index.lcc_subclasses.contains(letters)));
    for fragment in &report.unresolved_fragments {
        let ambiguous = ambiguous_lcc_matches(runtime, fragment);
        let matches = if ambiguous.is_empty() {
            broad_lcc_fallback(runtime, fragment).into_iter().map(|(id, supporting_concept_ids)| {
                (id, crate::LccMatchEvidence { method: crate::LccMatchMethod::BroadFallback,
                    code: fragment.clone(), supporting_concept_ids, interpretations: Vec::new() })
            }).collect()
        } else { ambiguous };
        for (id, evidence) in matches {
            if let Some(existing) = report.assignments.iter_mut().find(|a| a.concept_id == id) {
                if !existing.evidence.contains(&evidence) { existing.evidence.push(evidence); }
            } else if !report.assignments.iter().any(|a| runtime.ancestors[&a.concept_id].contains(&id)) {
                report.assignments.push(crate::LccMatchAssignment { concept_id: id, evidence: vec![evidence] });
            }
        }
    }
    report.assignments.sort_by_key(|a| a.concept_id);
    report
}

// A prefix is evidence for the shared parent of its represented completions,
// not permission to select one child or manufacture an endpoint. Include exact
// selectors and range boundaries; ancestors spanning an entire unrelated
// numeric region do not masquerade as specific completions of the prefix.
fn broad_lcc_fallback(runtime: &UnifiedTaxonomyRuntime, fragment: &str) -> Vec<(i64, Vec<i64>)> {
    if fragment.contains('/') {
        let parts = fragment.split('/').map(str::trim).collect::<Vec<_>>();
        let Some(index) = runtime.selectors.get(LCC_SYSTEM_ID) else { return Vec::new(); };
        if !(2..=4).contains(&parts.len()) || parts.iter().any(|part|
            part.is_empty() || !part.bytes().all(|b| b.is_ascii_uppercase())
                || !index.lcc_subclasses.contains(*part)) { return Vec::new(); }
        // Bare subclass alternatives do not establish that both subjects
        // apply. Recover only their common subject, keeping the original
        // expression unresolved as evidence of the remaining uncertainty.
        let readings = parts.iter().map(|part| strict_matching_concepts(runtime, LCC_SYSTEM_ID, part)).collect::<Vec<_>>();
        if readings.iter().any(BTreeSet::is_empty) { return Vec::new(); }
        let candidates = readings.into_iter().flatten().collect::<BTreeSet<_>>();
        let ids = if candidates.len() == 1 { candidates.iter().copied().collect() }
            else { shared_lcc_parents(runtime, &candidates) };
        return ids.into_iter().filter(|id| runtime.paths_by_concept.get(id)
            .is_some_and(|paths| paths.iter().any(|p| p != ROOT_SUBJECT_PATH)))
            .map(|id| (id, candidates.iter().copied().collect())).collect();
    }
    let Some(scope) = crate::lcc_recovery::fallback_scope(fragment) else { return Vec::new(); };
    let Some(index) = runtime.selectors.get(LCC_SYSTEM_ID) else { return Vec::new(); };
    if !index.lcc_subclasses.contains(&scope.letters) { return Vec::new(); }
    let mut candidates = BTreeSet::new();
    for range in &index.lcc_ranges {
        if (range.start_letters == scope.letters && scope.contains(range.start_number))
            || (range.end_letters == scope.letters && scope.contains(range.end_number))
            || (range.start_letters == scope.letters && range.end_letters == scope.letters
                && scope.intersects_numeric_prefix(range.start_number, range.end_number)) {
            candidates.insert(range.concept_id);
        }
    }
    for (code, ids) in &index.exact {
        if parse_lcc_call(code).is_some_and(|call| call.letters == scope.letters && scope.contains(call.number)) {
            candidates.extend(ids);
        }
    }
    for (prefix, id) in &index.prefixes {
        if parse_lcc_call(prefix).is_some_and(|call| call.letters == scope.letters && scope.contains(call.number)) {
            candidates.insert(*id);
        }
    }
    shared_lcc_parents(runtime, &candidates).into_iter()
        .map(|id| (id, candidates.iter().copied().collect())).collect()
}

fn shared_lcc_parents(runtime: &UnifiedTaxonomyRuntime, candidates: &BTreeSet<i64>) -> Vec<i64> {
    let mut shared: Option<BTreeSet<i64>> = None;
    for id in candidates {
        let mut parents = runtime.ancestors[id].clone();
        // With only one represented child, use a proper parent; never assume
        // the child is the only possible completion in the actual schedule.
        if candidates.len() > 1 { parents.insert(*id); }
        shared = Some(shared.map_or(parents.clone(), |prior| prior.intersection(&parents).copied().collect()));
    }
    let shared = shared.unwrap_or_default();
    shared.iter().filter(|id| {
        runtime.paths_by_concept.get(id).is_some_and(|paths| paths.iter().any(|p| p != ROOT_SUBJECT_PATH))
            && !shared.iter().any(|other| other != *id && runtime.ancestors[other].contains(id))
    }).copied().collect()
}

fn ambiguous_lcc_matches(runtime: &UnifiedTaxonomyRuntime, fragment: &str) -> Vec<(i64, crate::LccMatchEvidence)> {
    static KNOWN_SUBCLASSES: OnceLock<BTreeSet<String>> = OnceLock::new();
    let known = KNOWN_SUBCLASSES.get_or_init(super::lcc::outline_subclasses);
    let interpretations = crate::lcc_recovery::ambiguous_interpretations(fragment, |letters| known.contains(letters));
    if interpretations.is_empty() { return Vec::new(); }
    let readings = interpretations.iter().map(|code| matching_candidates(runtime, LCC_SYSTEM_ID, code)).collect::<Vec<_>>();
    // A syntactically valid but uncovered reading is still possible. Do not
    // discard it merely because a different reading happens to match.
    if readings.iter().any(BTreeSet::is_empty) { return Vec::new(); }
    let candidates = readings.iter().flatten().copied().collect::<BTreeSet<_>>();
    let agreement = readings.iter().all(|reading| reading == &readings[0])
        && interpretations.iter().all(|code| !strict_matching_concepts(runtime, LCC_SYSTEM_ID, code).is_empty());
    let ids = if agreement { candidates.iter().copied().collect() } else { shared_lcc_parents(runtime, &candidates) };
    ids.into_iter().map(|id| (id, crate::LccMatchEvidence {
        method: if agreement { crate::LccMatchMethod::Normalized } else { crate::LccMatchMethod::BroadFallback },
        code: fragment.to_owned(), supporting_concept_ids: candidates.iter().copied().collect(), interpretations: interpretations.clone(),
    })).collect()
}

pub(super) fn matching_concepts(runtime: &UnifiedTaxonomyRuntime, system_id: &str, code: &str) -> BTreeSet<i64> {
    let strict = strict_matching_concepts(runtime, system_id, code);
    if !strict.is_empty() || system_id != LCC_SYSTEM_ID { return strict; }
    // Conflicting complete selectors require curation; recovery must not
    // silently bypass an existing ambiguity by shortening a valid code.
    if matching_candidates(runtime, system_id, code).len() > 1 { return strict; }
    lcc_match_report(runtime, code).assignments.into_iter().map(|a| a.concept_id).collect()
}

fn strict_matching_concepts(runtime: &UnifiedTaxonomyRuntime, system_id: &str, code: &str) -> BTreeSet<i64> {
    if system_id == LCC_SYSTEM_ID {
        if let Some(ids) = explicit_lcc_list_matches(runtime, code) { return ids; }
    }
    let candidates = matching_candidates(runtime, system_id, code);
    if candidates.len() > 1 { BTreeSet::new() } else { candidates }
}

// Subject ancestry takes precedence over selector form. Among unrelated
// subjects, prefer exact codes, then narrower ranges, then prefixes.
// Equally specific unrelated subjects remain unresolved and diagnostic.
fn explicit_lcc_list_matches(runtime: &UnifiedTaxonomyRuntime, code: &str) -> Option<BTreeSet<i64>> {
    let parts = super::lcc::lcc_code_list(code)?;
    let mut result = BTreeSet::new();
    for part in parts {
        let ids = matching_candidates(runtime, LCC_SYSTEM_ID, &part);
        if ids.len() != 1 { return Some(BTreeSet::new()); }
        result.extend(ids);
    }
    Some(result)
}

fn matching_candidates(runtime: &UnifiedTaxonomyRuntime, system_id: &str, code: &str) -> BTreeSet<i64> {
    if system_id == LCC_SYSTEM_ID {
        if let Some(ids) = explicit_lcc_list_matches(runtime, code) { return ids; }
    }
    let Some(index) = runtime.selectors.get(system_id) else { return BTreeSet::new() };
    let canonical = if system_id == LCC_SYSTEM_ID {
        let Some(code) = canonical_lcc_notation(code) else { return BTreeSet::new() };
        code
    } else if system_id == DDC_SYSTEM_ID {
        let Some(code) = super::ddc::canonical_ddc_notation(code) else { return BTreeSet::new() };
        code[..3].to_owned()
    } else { code.trim().to_ascii_uppercase() };
    let unwrapped = if system_id == LCC_SYSTEM_ID { super::lcc::display_bracket_base(&canonical).unwrap_or(&canonical) } else { &canonical };
    let unwrapped = if system_id == LCC_SYSTEM_ID { super::lcc::repeated_lcc_base(unwrapped).unwrap_or(unwrapped) } else { unwrapped };
    let unwrapped = if system_id == LCC_SYSTEM_ID { super::lcc::range_item_base(unwrapped).unwrap_or(unwrapped) } else { unwrapped };
    let preliminary_span = (system_id == LCC_SYSTEM_ID).then(|| super::lcc::qualified_table_span(unwrapped).or_else(|| super::lcc::table_cutter_span(unwrapped)).or_else(|| super::lcc::variable_cutter_span(unwrapped))).flatten();
    let unwrapped = if system_id == LCC_SYSTEM_ID { super::lcc::schedule_table_base(unwrapped).unwrap_or(unwrapped) } else { unwrapped };
    let unwrapped = if system_id == LCC_SYSTEM_ID { super::lcc::display_bracket_base(unwrapped).unwrap_or(unwrapped) } else { unwrapped };
    let separated = (system_id == LCC_SYSTEM_ID).then(|| super::lcc::reviewed_call_base(unwrapped).map(str::to_owned).or_else(|| super::lcc::documented_literal_date_base(unwrapped)).or_else(|| super::lcc::documented_date_call(unwrapped).map(|(class, _, _)| class.to_owned())).or_else(|| super::lcc::marc_subfield_call_base(unwrapped)).or(preliminary_span).or_else(|| super::lcc::variable_cutter_span(unwrapped)).or_else(|| super::lcc::bracketed_year_call_base(unwrapped)).or_else(|| super::lcc::doubled_cutter_period_base(unwrapped)).or_else(|| super::lcc::period_year_call_base(unwrapped)).or_else(|| super::lcc::apostrophe_call_base(unwrapped)).or_else(|| super::lcc::local_cutter_x_base(unwrapped)).or_else(|| super::lcc::reference_collection_base(unwrapped).map(str::to_owned))).flatten();
    let plus_span = (system_id == LCC_SYSTEM_ID).then(|| super::lcc::reference_plus_span(unwrapped)).flatten();
    let table_family = (system_id == LCC_SYSTEM_ID).then(|| super::lcc::qualified_table_family(unwrapped)).flatten();
    let slash_family = table_family.as_deref().or_else(|| (system_id == LCC_SYSTEM_ID).then(|| super::lcc::terminal_slash_base(unwrapped)).flatten());
    let match_code = if system_id == LCC_SYSTEM_ID {
        // Display separators and item suffixes can occur in the same call.
        // Strip validated item enumeration after separator normalization too,
        // so a volume range cannot be mistaken for a classification range.
        let base = separated.as_deref().unwrap_or(unwrapped);
        super::lcc::lcc_item_base(base).unwrap_or(base)
    } else { &canonical };
    // kind, exact/prefix specificity, single subclass, numeric width
    type Rank<'a> = (u8, usize, bool, f64, Option<&'a LccSelector>);
    let mut matches = Vec::<(i64, Rank<'_>)>::new();
    let keys = if system_id == LCC_SYSTEM_ID { lcc_match_keys(&canonical).unwrap_or_default() } else { vec![canonical.clone()] };
    for (specificity, key) in keys.iter().enumerate() {
        if let Some(ids) = index.exact.get(key) {
            matches.extend(ids.iter().map(|id| (*id, (2, specificity, false, 0.0, None))));
        }
    }
    // An explicit printed-label assignment remains authoritative. Otherwise
    // match the whole incoming interval, never only its start or an overlap.
    if system_id == LCC_SYSTEM_ID && (matches.is_empty() || slash_family.is_some() || plus_span.is_some()) {
        if let Some((start, end)) = plus_span.clone().or_else(|| lcc_span(match_code)).or_else(|| {
            let base = slash_family?;
            Some((super::lcc::lcc_endpoint(base, false)?, super::lcc::lcc_endpoint(base, true)?))
        }).filter(|(start, end)| matches.is_empty()
            || (index.lcc_subclasses.contains(&start.letters) && index.lcc_subclasses.contains(&end.letters))) {
            // Broad class coverage must not turn invented subclass prefixes
            // into valid matches. Explicit reviewed range labels above still win.
            if !index.lcc_subclasses.contains(&start.letters) || !index.lcc_subclasses.contains(&end.letters) {
                return BTreeSet::new();
            }
            if slash_family.is_none() && plus_span.is_none() && start.code == end.code && start.number == end.number {
                return matching_candidates(runtime, system_id, &start.code);
            }
            let start_cutters = cutter_components(&start.remainder);
            let end_cutters = cutter_components(&end.remainder);
            for range in &index.lcc_ranges {
                if compare_lcc_key(&range.start_letters, range.start_number, &start.letters, start.number).is_le()
                    && compare_lcc_key(&end.letters, end.number, &range.end_letters, range.end_number).is_le()
                    && (range.start_letters != start.letters || range.start_number != start.number
                        || compare_cutter_endpoint(&start_cutters, &range.start_cutters).is_ge())
                    && (range.end_letters != end.letters || range.end_number != end.number || range.end_cutters.is_empty()
                        || compare_upper_cutters(&end_cutters, &range.end_cutters).is_le()) {
                    let single = range.start_letters == range.end_letters;
                    matches.push((range.concept_id, (1, range.start_cutters.len().max(range.end_cutters.len()), single,
                        if single { range.end_number - range.start_number } else { f64::INFINITY }, Some(range))));
                }
            }
            for (prefix, id) in &index.prefixes {
                if prefix.bytes().all(|byte| byte.is_ascii_alphabetic())
                    && start.letters.starts_with(prefix) && end.letters.starts_with(prefix)
                    && index.lcc_subclasses.contains(&start.letters) && index.lcc_subclasses.contains(&end.letters) {
                    matches.push((*id, (0, prefix.len(), false, 0.0, None)));
                }
            }
        }
    }

    let call = (system_id == LCC_SYSTEM_ID).then(|| parse_lcc_call(match_code)).flatten();
    let allow_fallbacks = system_id != LCC_SYSTEM_ID || (slash_family.is_none() && plus_span.is_none() && !match_code.contains('-')
        && call.as_ref().is_some_and(|call| index.lcc_subclasses.contains(&call.letters)));
    if allow_fallbacks {
        if let Some(call) = &call {
            let cutters = cutter_components(call.remainder);
            let ranges: Box<dyn Iterator<Item = &LccSelector> + '_> = if let Some(positions) = index.lcc_ranges_by_subclass.get(&call.letters) {
                Box::new(positions.iter().map(|p| &index.lcc_ranges[*p]))
            } else {
                Box::new(index.lcc_ranges.iter().filter(|r| r.start_letters <= call.letters && call.letters <= r.end_letters))
            };
            for range in ranges {
                if compare_lcc_key(&range.start_letters, range.start_number, &call.letters, call.number).is_le()
                    && compare_lcc_key(&call.letters, call.number, &range.end_letters, range.end_number).is_le()
                    && (call.letters != range.start_letters || call.number != range.start_number
                        || compare_cutter_endpoint(&cutters, &range.start_cutters).is_ge())
                    && (call.letters != range.end_letters || call.number != range.end_number
                        || compare_cutter_endpoint(&cutters, &range.end_cutters).is_le()) {
                    let single = range.start_letters == call.letters && range.end_letters == call.letters;
                    matches.push((range.concept_id, (1, range.start_cutters.len().max(range.end_cutters.len()), single, if single { range.end_number - range.start_number } else { f64::INFINITY }, Some(range))));
                }
            }
        }
        for (prefix, id) in &index.prefixes {
            let accepts = if system_id != LCC_SYSTEM_ID || prefix.bytes().all(|byte| byte.is_ascii_alphabetic()) {
                canonical.starts_with(prefix)
            } else {
                lcc_match_keys(prefix).and_then(|keys| keys.last().cloned()).is_some_and(|key| keys.contains(&key))
            };
            if accepts { matches.push((*id, (0, prefix.len(), false, 0.0, None))); }
        }
    }
    let shadowed = matches.iter().flat_map(|(id, _)| runtime.ancestors[id].iter().copied()).collect::<BTreeSet<_>>();
    matches.retain(|(id, _)| !shadowed.contains(id));
    let compare = |left: &Rank<'_>, right: &Rank<'_>| left.0.cmp(&right.0)
        .then_with(|| left.1.cmp(&right.1))
        .then_with(|| left.2.cmp(&right.2))
        .then_with(|| right.3.total_cmp(&left.3));
    let Some(best) = matches.iter().map(|(_, rank)| rank).max_by(|a, b| compare(a, b)) else { return BTreeSet::new() };
    let tied = matches.iter().filter(|(_, rank)| compare(rank, best).is_eq()).collect::<Vec<_>>();
    // Numeric width alone cannot distinguish nested Cutter intervals at the
    // same class number. Prefer strict containment, retaining genuine ties
    // between equal or merely overlapping intervals.
    tied.iter().filter(|(id, rank)| !tied.iter().any(|(other_id, other_rank)| {
        id != other_id && rank.4.zip(other_rank.4).is_some_and(|(outer, inner)| {
            lcc_range_contains(outer, inner) && !lcc_range_contains(inner, outer)
        })
    })).map(|(id, _)| *id).collect()
}

fn lcc_range_contains(outer: &LccSelector, inner: &LccSelector) -> bool {
    let start = compare_lcc_key(&outer.start_letters, outer.start_number, &inner.start_letters, inner.start_number);
    let end = compare_lcc_key(&inner.end_letters, inner.end_number, &outer.end_letters, outer.end_number);
    start.is_le() && end.is_le()
        && (!start.is_eq() || compare_cutter_endpoint(&inner.start_cutters, &outer.start_cutters).is_ge())
        && (!end.is_eq() || compare_upper_cutters(&inner.end_cutters, &outer.end_cutters).is_le())
}

pub(super) fn normalize_source_label(value: &str) -> String {
    compact_whitespace(value).to_lowercase()
}

fn default_runtime() -> Arc<UnifiedTaxonomyRuntime> {
    let runtime = build_runtime(default_concepts(), false).expect("embedded unified taxonomy must be valid");
    debug_assert_eq!(runtime.revision_id, BUNDLED_UNIFIED_TAXONOMY_REVISION_ID, "bundled taxonomy revision constant must track the embedded database");
    Arc::new(runtime)
}

fn default_concepts() -> Vec<UnifiedConceptDefinition> {
    crate::taxonomy_sqlite::read_embedded_unified_taxonomy().expect("embedded unified taxonomy SQLite must be readable")
}

#[cfg(test)]
pub(super) fn install_authoritative_taxonomy_for_tests() {
    static INSTALLED: OnceLock<()> = OnceLock::new();
    INSTALLED.get_or_init(|| {
        let mut connection = rusqlite::Connection::open_in_memory().expect("authoritative taxonomy fixture connection must open");
        connection.deserialize_bytes("main", include_bytes!("../data/unified-taxonomy-v2.sqlite3")).expect("authoritative taxonomy fixture image must open");
        let concepts = crate::taxonomy_sqlite::read_unified_taxonomy_connection(&connection, "embedded authoritative taxonomy fixture").expect("authoritative taxonomy fixture must be readable");
        install_unified_taxonomy_concepts(concepts).expect("authoritative taxonomy fixture must be valid");
    });
}

fn build_runtime(concepts: Vec<UnifiedConceptDefinition>, retain_concepts: bool) -> Result<UnifiedTaxonomyRuntime, String> {
    validate(&concepts)?;
    let mut fingerprint = String::new();
    for concept in &concepts {
        use std::fmt::Write as _;
        write!(&mut fingerprint, "{}\u{1f}{}\u{1f}{}", concept.concept_id, concept.preferred_label, concept.parent_ids.iter().map(i64::to_string).collect::<Vec<_>>().join("|")).unwrap();
        for (system, selectors) in &concept.source_selectors {
            write!(&mut fingerprint, "\u{1f}{system}={}", selectors.join("|")).unwrap();
        }
        writeln!(&mut fingerprint, "\u{1f}{}", concept.source_labels.join("|")).unwrap();
    }
    let concepts = Arc::new(concepts);
    let routes = Arc::new(build_routes(&concepts)?);
    let mut paths_by_concept = BTreeMap::<i64, Vec<String>>::new();
    for route in routes.values() {
        paths_by_concept.entry(route.concept_id.clone()).or_default().push(route.path.clone());
    }
    let selectors = Arc::new(build_selectors(&concepts)?);
    let parents = concepts.iter().map(|c| (c.concept_id, c.parent_ids.clone())).collect::<BTreeMap<_, _>>();
    fn ancestors_of(id: i64, parents: &BTreeMap<i64, Vec<i64>>, cache: &mut BTreeMap<i64, BTreeSet<i64>>) -> BTreeSet<i64> {
        if let Some(ancestors) = cache.get(&id) { return ancestors.clone(); }
        let mut ancestors = BTreeSet::new();
        for parent in &parents[&id] {
            ancestors.insert(*parent);
            ancestors.extend(ancestors_of(*parent, parents, cache));
        }
        cache.insert(id, ancestors.clone());
        ancestors
    }
    let mut ancestors = BTreeMap::new();
    for id in parents.keys() { ancestors_of(*id, &parents, &mut ancestors); }
    let ancestors = Arc::new(ancestors);
    let mut source_labels = BTreeMap::<String, BTreeSet<i64>>::new();
    for concept in concepts.iter() {
        for label in &concept.source_labels {
            source_labels.entry(normalize_source_label(label)).or_default().insert(concept.concept_id.clone());
        }
    }
    const NAMESPACE: uuid::Uuid = uuid::Uuid::from_bytes([0xa7, 0xed, 0x9f, 0x7d, 0xa1, 0x2a, 0x59, 0xf5, 0xb5, 0x3d, 0x67, 0x4d, 0xe0, 0x80, 0x7c, 0x66]);
    let revision_id = format!("{UNIFIED_VERSION}:{}", uuid::Uuid::new_v5(&NAMESPACE, fingerprint.as_bytes()));
    let concepts = retain_concepts.then_some(concepts);
    Ok(UnifiedTaxonomyRuntime { concepts, routes, paths_by_concept: Arc::new(paths_by_concept), selectors, ancestors, source_labels: Arc::new(source_labels), revision_id })
}

fn validate(concepts: &[UnifiedConceptDefinition]) -> Result<(), String> {
    if concepts.iter().any(|concept| concept.concept_id <= 0 || concept.preferred_label.trim().is_empty()) {
        return Err("every unified concept requires an ID and preferred label".to_owned());
    }
    let ids = concepts.iter().map(|concept| concept.concept_id).collect::<BTreeSet<_>>();
    if ids.len() != concepts.len() {
        return Err("unified taxonomy concept IDs must be unique".to_owned());
    }
    let labels_by_id = concepts.iter().map(|concept| (concept.concept_id, normalized_concept_label(&concept.preferred_label))).collect::<BTreeMap<_, _>>();
    let mut sibling_labels = BTreeMap::<(i64, String), i64>::new();
    let mut selector_labels = BTreeMap::<(&str, &str, String), i64>::new();
    for concept in concepts {
        for parent in &concept.parent_ids {
            if !ids.contains(parent) {
                return Err(format!("unified concept {} references missing parent {parent}", concept.concept_id));
            }
            let label = labels_by_id.get(&concept.concept_id).expect("concept label exists");
            let parent_label = labels_by_id.get(parent).expect("parent label exists");
            if label == parent_label {
                return Err(format!("unified concept {} duplicates the preferred label of direct parent {parent}", concept.concept_id));
            }
            if let Some(existing) = sibling_labels.insert((*parent, label.clone()), concept.concept_id) {
                if existing != concept.concept_id {
                    return Err(format!("unified concepts {existing} and {} duplicate a preferred label beneath parent {parent}", concept.concept_id));
                }
            }
        }
        let label = labels_by_id.get(&concept.concept_id).expect("concept label exists");
        for (system, selectors) in &concept.source_selectors {
            for selector in selectors {
                // Range overlaps, including equal bounds, are resolved by
                // specificity or exposed as ambiguous by match_candidates.
                if system == LCC_SYSTEM_ID && selector.contains("..") { continue; }
                if let Some(existing) = selector_labels.insert((system.as_str(), selector.as_str(), label.clone()), concept.concept_id) {
                    if existing != concept.concept_id {
                        return Err(format!("unified concepts {existing} and {} duplicate source selector {system}:{selector} with the same preferred label", concept.concept_id));
                    }
                }
            }
        }
    }
    let mut reachable = concepts.iter().filter(|concept| concept.parent_ids.is_empty()).map(|concept| concept.concept_id).collect::<BTreeSet<_>>();
    loop {
        let before = reachable.len();
        for concept in concepts {
            if concept.parent_ids.iter().all(|parent| reachable.contains(parent)) {
                reachable.insert(concept.concept_id);
            }
        }
        if reachable.len() == before {
            break;
        }
    }
    if reachable.len() != concepts.len() {
        return Err("unified taxonomy parent graph contains a cycle".to_owned());
    }
    Ok(())
}

fn normalized_concept_label(value: &str) -> String {
    let characters = value.to_lowercase().chars().collect::<Vec<_>>();
    characters
        .iter()
        .enumerate()
        .map(|(index, character)| {
            if character.is_alphanumeric() || matches!(character, '#' | '+') {
                *character
            } else if *character == '.' && characters[..index].iter().all(|previous| previous.is_whitespace()) && characters.get(index + 1).is_some_and(|next| next.is_alphanumeric()) {
                // A leading dot is part of names such as `.NET`; a trailing
                // dot remains decorative punctuation for duplicate checks.
                *character
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn build_routes(concepts: &[UnifiedConceptDefinition]) -> Result<BTreeMap<String, UnifiedConceptRoute>, String> {
    let sort_orders = lcc_subject_sort_orders(concepts);
    let mut child_counts = HashMap::<i64, usize>::new();
    for concept in concepts {
        for parent_id in &concept.parent_ids {
            *child_counts.entry(parent_id.clone()).or_insert(0) += 1;
        }
    }

    let mut routes = BTreeMap::new();
    let mut known = BTreeMap::<i64, Vec<String>>::new();
    let mut routed_concepts = BTreeSet::new();

    let is_root_with_no_children = |concept: &UnifiedConceptDefinition| concept.parent_ids.is_empty()
        && child_counts.get(&concept.concept_id()).copied().unwrap_or_default() == 0
        && concept.source_selectors.values().all(Vec::is_empty);

    for concept in concepts.iter().filter(|concept| concept.parent_ids.is_empty()) {
        if is_root_with_no_children(concept) {
            continue;
        }
        let path = concept.preferred_label.clone();
        routes.insert(path.clone(), UnifiedConceptRoute { concept_id: concept.concept_id.clone(), path: path.clone(), parent_path: ROOT_SUBJECT_PATH.to_owned(), label: concept.preferred_label.clone(), sort_order: sort_orders[&concept.concept_id] });
        known.entry(concept.concept_id.clone()).or_default().push(path);
        routed_concepts.insert(concept.concept_id.clone());
    }

    loop {
        let mut added = false;
        for concept in concepts.iter().filter(|concept| !concept.parent_ids.is_empty()) {
            if is_root_with_no_children(concept) {
                continue;
            }
            for parent_id in &concept.parent_ids {
                let Some(parent_paths) = known.get(parent_id).cloned() else { continue };
                for parent_path in parent_paths {
                    let path = format!("{parent_path} / {}", concept.preferred_label);
                    if routes.contains_key(&path) {
                        // A multi-parent diamond can discover the same route
                        // more than once. Keep one navigable route while still
                        // recording the concept's projection onto that route.
                        let concept_paths = known.entry(concept.concept_id.clone()).or_default();
                        if !concept_paths.contains(&path) {
                            concept_paths.push(path);
                        }
                        routed_concepts.insert(concept.concept_id.clone());
                        continue;
                    }
                    routes.insert(path.clone(), UnifiedConceptRoute { concept_id: concept.concept_id.clone(), path: path.clone(), parent_path, label: concept.preferred_label.clone(), sort_order: sort_orders[&concept.concept_id] });
                    known.entry(concept.concept_id.clone()).or_default().push(path);
                    routed_concepts.insert(concept.concept_id.clone());
                    added = true;
                }
            }
        }
        if !added {
            break;
        }
    }
    let routable_concepts = concepts.iter().filter(|concept| !is_root_with_no_children(concept)).count();
    if routed_concepts.len() != routable_concepts {
        return Err("every unified concept parent must resolve and the graph must be acyclic".to_owned());
    }
    Ok(routes)
}

fn build_selectors(concepts: &[UnifiedConceptDefinition]) -> Result<BTreeMap<String, SelectorIndex>, String> {
    let mut indexes = BTreeMap::<String, SelectorIndex>::new();
    for concept in concepts {
        for (system_id, selectors) in &concept.source_selectors {
            let index = indexes.entry(system_id.clone()).or_default();
            for selector in selectors {
                let selector = selector.trim().to_ascii_uppercase();
                if let Some(prefix) = selector.strip_suffix('*') {
                    if prefix.is_empty() {
                        return Err(format!("unified concept {} has an empty prefix selector", concept.concept_id));
                    }
                    index.prefixes.push((prefix.to_owned(), concept.concept_id.clone()));
                } else if system_id == LCC_SYSTEM_ID && selector.contains("..") {
                    if selector.contains('@') { return Err(format!("invalid legacy LCC range suffix: {selector}")); }
                    let (start, end) = selector.split_once("..").ok_or_else(|| format!("invalid LCC selector {selector}"))?;
                    let start = stored_lcc_endpoint(start, false).ok_or_else(|| format!("invalid LCC range start {start}"))?;
                    let end = stored_lcc_endpoint(end, true).ok_or_else(|| format!("invalid LCC range end {end}"))?;
                    index.lcc_subclasses.insert(start.letters.clone());
                    index.lcc_subclasses.insert(end.letters.clone());
                    index.lcc_ranges.push(LccSelector {
                        start_cutters: cutter_components(&start.remainder),
                        end_cutters: cutter_components(&end.remainder),
                        start_letters: start.letters,
                        start_number: start.number,
                        end_letters: end.letters,
                        end_number: end.number,
                        concept_id: concept.concept_id.clone(),
                    });
                } else {
                    if system_id == LCC_SYSTEM_ID && selector.bytes().all(|byte| byte.is_ascii_alphabetic()) {
                        index.lcc_subclasses.insert(selector.clone());
                    }
                    let selector = if system_id == LCC_SYSTEM_ID {
                        lcc_match_keys(&selector).and_then(|keys| keys.last().cloned()).unwrap_or(selector)
                    } else { selector };
                    index.exact.entry(selector).or_default().insert(concept.concept_id.clone());
                }
            }
        }
    }
    for index in indexes.values_mut() {
        for subclass in super::lcc::outline_subclasses() {
            if index.lcc_ranges.iter().any(|r| r.start_letters <= subclass && subclass <= r.end_letters) {
                index.lcc_subclasses.insert(subclass);
            }
        }
        for subclass in &index.lcc_subclasses {
            let positions = index.lcc_ranges.iter().enumerate()
                .filter(|(_, range)| range.start_letters <= *subclass && *subclass <= range.end_letters)
                .map(|(position, _)| position).collect();
            index.lcc_ranges_by_subclass.insert(subclass.clone(), positions);
        }
    }
    Ok(indexes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_cutter_intervals_choose_the_more_specific_subject() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "County", &[], &["DA670.C8..DA670.C89"]),
            concept(2, "Hills", &[], &["DA670.C83..DA670.C839"]),
            concept(3, "Local history", &[], &["DS54.95.A..DS54.95.Z"]),
            concept(4, "City", &[], &["DS54.95.L37..DS54.95.L379"]),
        ]).unwrap();
        assert_eq!(taxonomy.lcc_match_candidates("DA670.C83 S35x"), vec![2]);
        assert_eq!(taxonomy.lcc_match_candidates("DA670.C85 .A1"), vec![1]);
        assert_eq!(taxonomy.lcc_match_candidates("DS54.95.L373 A58 2011"), vec![4]);
        assert_eq!(taxonomy.lcc_match_candidates("DA670.C831-DA670.C838"), vec![2]);
    }

    #[test]
    fn cutter_interval_ties_and_partial_overlaps_remain_ambiguous() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "First", &[], &["QA1.C2..QA1.C6"]),
            concept(2, "Second", &[], &["QA1.C4..QA1.C8"]),
            concept(3, "Equal first", &[], &["QA2.C8..QA2.C89"]),
            concept(4, "Equal second", &[], &["QA2.C80..QA2.C890"]),
        ]).unwrap();
        assert_eq!(taxonomy.lcc_match_candidates("QA1.C5"), vec![1, 2]);
        assert_eq!(taxonomy.lcc_match_candidates("QA2.C83"), vec![3, 4]);
    }

    #[test]
    fn subject_ids_are_positive_and_unknown_paths_have_no_identity() {
        for id in [0, -1] {
            assert!(validate(&[concept(id, "Invalid", &[], &[])]).is_err());
        }
        assert!(validate(&[concept(i64::MAX, "Valid", &[], &[])]).is_ok());
        assert_eq!(unified_concept_id("This path does not exist in the taxonomy"), None);
    }

    #[test]
    fn topic_cutters_choose_one_most_specific_subject() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(3, "Management", &[], &["HD69"]),
            concept(24, "Consultants", &[3], &["HD 69 .C6"]),
            concept(19, "Other topic", &[3], &["HD69.C65"]),
        ]).unwrap();
        for code in ["HD69.C6 R38 1999", "HD 69 .C6R38 1999", "HD69.C6.R38 1999", "HD 69 .C6 R38 1999a"] {
            assert_eq!(taxonomy.matching_concept_ids("lcc", code), vec![24], "{code}");
        }
        assert_eq!(taxonomy.matching_concept_ids("lcc", "HD69.C65 R38"), vec![19]);
        assert_eq!(taxonomy.matching_concept_ids("lcc", "HD69.C654"), vec![3]);
        assert!(taxonomy.matching_concept_ids("lcc", "HD69.1.C6").is_empty());
        // Documented Cutter-final x retains the complete topic Cutter;
        // arbitrary suffix letters remain unsupported.
        assert_eq!(taxonomy.matching_concept_ids("lcc", "HD69.C6X"), vec![24]);
        assert!(taxonomy.matching_concept_ids("lcc", "HD69.C6Z").is_empty());
        assert!(taxonomy.matching_concept_ids("lcc", "HD69-70").is_empty());
    }

    #[test]
    fn fully_written_printed_ranges_match_only_their_exact_label() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(16, "Literature", &[], &["PN1-PN6790"]),
            concept(11, "Film", &[16], &["PN1997"]),
        ]).unwrap();
        assert_eq!(taxonomy.matching_concept_ids("lcc", "PN1-PN6790"), vec![16]);
        assert_eq!(taxonomy.matching_concept_ids("lcc", "PN1-6790"), vec![16]);
        assert_eq!(taxonomy.matching_concept_ids("lcc", "pn 1 - pn 6790"), vec![16]);
        assert_eq!(taxonomy.matching_concept_ids("lcc", "PN1997"), vec![11]);
        assert!(taxonomy.matching_concept_ids("lcc", "PN100").is_empty());
        assert!(taxonomy.matching_concept_ids("lcc", "PN1-PN6790.A3").is_empty());
    }

    #[test]
    fn reviewed_bases_keep_both_person_and_work_cutters() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Classicists", &[], &["PA85"]),
            concept(2, "David Grene", &[1], &["PA85.G646"]),
            concept(3, "Memoir", &[2], &["PA85.G646.A3"]),
        ]).unwrap();
        assert_eq!(taxonomy.matching_concept_ids("lcc", "PA85.G 646A3 date"), vec![3]);
    }

    #[test]
    fn literal_date_texts_do_not_match_neighboring_criticism() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Version", &[], &["BS195.E64..BS195.E642"]),
            concept(2, "Texts", &[1], &["BS195.E64"]),
            concept(3, "Criticism", &[1], &["BS195.E642"]),
        ]).unwrap();
        assert_eq!(taxonomy.matching_concept_ids("lcc", "BS195 .E64 date"), vec![2]);
        assert!(taxonomy.matching_concept_ids("lcc", "BS195 .E642 date").is_empty());
    }

    #[test]
    fn dated_calls_preserve_date_and_item_specificity() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Papal documents", &[], &["BX870"]),
            concept(6, "Accession year", &[1], &["BX870 1978"]),
            concept(2, "John Paul II", &[6], &["BX870 1978 Oct."]),
            concept(3, "Specific item", &[2], &["BX870 1978 Oct. .S65"]),
            concept(4, "Events of 1815", &[], &["DC240"]),
            concept(5, "Abdication", &[4], &["DC240 June 22"]),
        ]).unwrap();
        for code in ["BX870 1978 Oct.", "BX870 1978 OCT", "BX870 1978 October 2005b"] {
            assert_eq!(taxonomy.matching_concept_ids("lcc", code), vec![2], "{code}");
        }
        assert_eq!(taxonomy.matching_concept_ids("lcc", "BX870 1978 Oct .S65 2012"), vec![3]);
        assert_eq!(taxonomy.matching_concept_ids("lcc", "BX870 1978 Aug."), vec![6]);
        assert_eq!(taxonomy.matching_concept_ids("lcc", "DC240 June 22 B47 2011"), vec![5]);
        assert_eq!(taxonomy.matching_concept_ids("lcc", "DC240 June 16 .B37 2006"), vec![4]);
        for code in ["DC240 June 32", "DC240 Feb. 29", "BX870 1978 OCT. EXTRA", "BX870 1978 OCT. 2005 NOV."] {
            assert!(taxonomy.matching_concept_ids("lcc", code).is_empty(), "{code}");
        }
    }

    #[test]
    fn numbered_table_plus_scopes_require_the_whole_derived_interval() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Complete table scope", &[], &["PK6729..PK6732.9"]),
            concept(2, "Partial scope", &[1], &["PK6729..PK6730"]),
            concept(3, "First number", &[2], &["PK6729"]),
        ]).unwrap();
        assert_eq!(taxonomy.matching_concept_ids("lcc", "PK6729+"), vec![1]);
        assert_eq!(taxonomy.matching_concept_ids("lcc", "PK6729"), vec![3]);
    }

    #[test]
    fn explicit_closed_plus_range_does_not_expand_into_a_broader_heading() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Tibet", &[], &["DS785..DS786"]),
            concept(2, "Through 1950", &[1], &["DS785.A5..DS785.Z"]),
            concept(3, "Partial interval", &[2], &["DS785.A5..DS785.M"]),
        ]).unwrap();
        assert_eq!(taxonomy.matching_concept_ids("lcc", "DS785.A5-Z+"), vec![2]);
        assert_eq!(taxonomy.matching_concept_ids("lcc", "DS785.A5+"), vec![1]);
        assert!(taxonomy.matching_concept_ids("lcc", "DS785.A5-ZI+").is_empty());
    }

    #[test]
    fn unfinished_item_captions_do_not_change_cutter_specificity() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "History", &[], &["DS400..DS500"]),
            concept(2, "Specific work", &[1], &["DS436.N47"]),
            concept(3, "Different Cutter", &[1], &["DS436.N48"]),
        ]).unwrap();
        for code in ["DS436 .N47 1987 PT.", "DS436 .N47 1987 VOL", "DS436 .N47 1987 V.1:NO."] {
            assert_eq!(taxonomy.matching_concept_ids("lcc", code), vec![2], "{code}");
        }
        assert_eq!(taxonomy.matching_concept_ids("lcc", "DS436 BAD TEXT PT."), vec![1]);
    }

    #[test]
    fn encoded_hyphens_preserve_full_range_containment() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Atlases", &[], &["G1000..G3200"]),
            concept(2, "Partial range", &[1], &["G1100..G1200"]),
            concept(3, "First endpoint", &[2], &["G1100"]),
        ]).unwrap();
        for code in ["G1100#8209;3102", "G1100&#8209;3102", "G1100‑3102"] {
            assert_eq!(taxonomy.matching_concept_ids("lcc", code), vec![1], "{code}");
        }
        assert!(taxonomy.matching_concept_ids("lcc", "G3102#8209;1100").is_empty());
    }

    #[test]
    fn equivalent_range_labels_do_not_choose_between_conflicting_subjects() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(17, "Mathematics", &[], &["QA1-939"]),
            concept(19, "Other", &[], &["QA1-QA939"]),
        ]).unwrap();
        assert!(taxonomy.matching_concept_ids("lcc", "QA1-939").is_empty());
        assert!(taxonomy.matching_concept_ids("lcc", "QA1-QA939").is_empty());
        assert!(taxonomy.matching_concept_ids("lcc", "QA500").is_empty());
    }

    #[test]
    fn incoming_ranges_require_full_containment_and_keep_individual_specificity() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Mathematics", &[], &["QA1..QA939"]),
            concept(2, "Computing", &[1], &["QA75.5..QA76.95"]),
            concept(3, "Programming", &[2], &["QA76.6..QA76.66"]),
            concept(4, "Exact topic", &[3], &["QA76.65"]),
        ]).unwrap();
        for code in ["QA75.5-76.95", "qa 75.5 - qa 76.95"] {
            assert_eq!(taxonomy.lcc_match_candidates(code), vec![2], "{code}");
        }
        assert_eq!(taxonomy.lcc_match_candidates("QA75-77"), vec![1]);
        assert_eq!(taxonomy.lcc_match_candidates("QA76.6-76.66"), vec![3]);
        assert_eq!(taxonomy.lcc_match_candidates("QA76.65 .R3 2024"), vec![4]);
        assert_eq!(taxonomy.lcc_match_candidates("QA76.65-76.65"), vec![4]);
        assert!(taxonomy.lcc_match_candidates("QA900-950").is_empty());
        assert!(taxonomy.lcc_match_candidates("QA950-900").is_empty());
    }

    #[test]
    fn incoming_ranges_preserve_explicit_labels_and_report_ties() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "First", &[], &["QA1..QA10"]),
            concept(2, "Second", &[], &["QA1..QA10"]),
            concept(3, "Reviewed span", &[], &["QA2-QA9"]),
        ]).unwrap();
        assert_eq!(taxonomy.lcc_match_candidates("QA2-9"), vec![3]);
        assert_eq!(taxonomy.lcc_match_candidates("QA3-8"), vec![1, 2]);
        let explicit_conflict = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "First label", &[], &["QA2-9"]),
            concept(2, "Second label", &[], &["QA2-QA9"]),
            concept(3, "Containing range", &[], &["QA1..QA10"]),
        ]).unwrap();
        assert_eq!(explicit_conflict.lcc_match_candidates("QA2-9"), vec![1, 2]);
    }

    #[test]
    fn incoming_ranges_do_not_bridge_gaps_or_cut_through_cutter_boundaries() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Separated coverage", &[], &["QA1..QA4", "QA8..QA10"]),
            concept(2, "Cutter boundary", &[], &["QB1.A1..QB10.C5"]),
            concept(3, "Structural subclass", &[], &["QC", "QC*"]),
        ]).unwrap();
        assert!(taxonomy.lcc_match_candidates("QA3-9").is_empty());
        assert!(taxonomy.lcc_match_candidates("QB1-9").is_empty());
        assert!(taxonomy.lcc_match_candidates("QB2-10").is_empty());
        assert_eq!(taxonomy.lcc_match_candidates("QB2-9"), vec![2]);
        assert_eq!(taxonomy.lcc_match_candidates("QC2-9"), vec![3]);
        assert!(taxonomy.lcc_match_candidates("QX2-9").is_empty());
    }

    #[test]
    fn legacy_priorities_do_not_resolve_equal_ranges_or_preserve_duplicates() {
        let left = concept(1, "Shared label", &[], &["QA1..QA10@1", "QA1..QA10@9"]);
        assert_eq!(left.source_selectors("lcc"), &["QA1..QA10"]);
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            left, concept(2, "Shared label", &[], &["QA1..QA10"]),
        ]).unwrap();
        assert_eq!(taxonomy.lcc_match_candidates("QA5"), vec![1, 2]);
        assert!(taxonomy.matching_concept_ids("lcc", "QA5").is_empty());
    }

    #[test]
    fn equally_specific_lcc_matches_are_unresolved() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(3, "Management", &[], &["HD69"]),
            concept(12, "Consultants", &[3], &["HD69.C6"]),
            concept(23, "Consulting", &[3], &["HD 69 .C6"]),
        ]).unwrap();
        assert!(taxonomy.matching_concept_ids("lcc", "HD69.C6 R38").is_empty());
        assert_eq!(taxonomy.lcc_match_candidates("HD69.C6 R38"), vec![12, 23]);
        assert_eq!(taxonomy.lcc_selector_conflicts()["HD69.C6"], vec![12, 23]);
    }

    #[test]
    fn fallback_ranges_preserve_exact_and_existing_range_assignments() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(17, "Mathematics", &[], &["QA1..QA939@0"]),
            concept(9, "Computing", &[17], &["QA75.5..QA76.95@1"]),
            concept(24, "Specific topic", &[9], &["QA76.73.R87"]),
            concept(2, "Analysis", &[17], &["QA299.6..QA433@0"]),
        ]).unwrap();
        assert_eq!(taxonomy.matching_concept_ids("lcc", "QA76.73.R87 .A1 2025"), vec![24]);
        assert_eq!(taxonomy.matching_concept_ids("lcc", "QA76.9.X99"), vec![9]);
        assert_eq!(taxonomy.matching_concept_ids("lcc", "QA350 .A1"), vec![2]);
        assert_eq!(taxonomy.matching_concept_ids("lcc", "QA500 .A1"), vec![17]);
        assert_eq!(taxonomy.matching_concept_ids("lcc", "QA1-939"), vec![17]);
        assert!(taxonomy.matching_concept_ids("lcc", "QA940").is_empty());
    }

    #[test]
    fn specificity_unrelated_ties_are_unresolved_for_every_code_system() {
        let mut definitions = vec![
            concept(15, "Left", &[], &["QA1..QA10@1"]),
            concept(21, "Right", &[], &["QA1..QA10@9"]),
        ];
        for definition in &mut definitions {
            definition.source_selectors.insert("bisac".into(), vec!["COM000000".into()]);
        }
        let taxonomy = UnifiedTaxonomy::from_concepts(definitions).unwrap();
        assert!(taxonomy.matching_concept_ids("lcc", "QA5.A1").is_empty());
        assert_eq!(taxonomy.lcc_match_candidates("QA5.A1"), vec![15, 21]);
        assert!(taxonomy.matching_concept_ids("bisac", "COM000000").is_empty());
    }

    #[test]
    fn subclass_specificity_beats_cross_class_priority_and_keeps_unknown_subclasses() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Broad", &[], &["A1..AZ100@2"]),
            concept(2, "Known subclass", &[], &["AC"]),
            concept(3, "Lower priority", &[], &["AC1..AC10@1"]),
        ]).unwrap();
        assert_eq!(taxonomy.lcc_match_candidates("AC5"), vec![3]);
        assert_eq!(taxonomy.lcc_match_candidates("A1"), vec![1]);
        assert_eq!(taxonomy.lcc_match_candidates("AZ100"), vec![1]);
        assert!(taxonomy.lcc_match_candidates("AZ101").is_empty());
        assert!(taxonomy.lcc_match_candidates("AO5").is_empty());
        assert!(taxonomy.lcc_match_candidates("AO1-AO2").is_empty());
        assert!(taxonomy.lcc_match_candidates("AC1-AO2").is_empty());
    }

    #[test]
    fn specificity_descendants_win_even_against_exact_ancestors_and_higher_priorities() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Parent", &[], &["QA76", "QA1..QA100@9"]),
            concept(2, "Child", &[1], &["QA75..QA80@0"]),
            concept(3, "Grandchild", &[2], &["QA76..QA77@0"]),
        ]).unwrap();
        assert_eq!(taxonomy.matching_concept_ids("lcc", "QA76.A1"), vec![3]);
        assert_eq!(taxonomy.matching_concept_ids("lcc", "QA78"), vec![2]);
        assert_eq!(taxonomy.matching_concept_ids("lcc", "QA90"), vec![1]);
    }

    #[test]
    fn specificity_narrower_unrelated_ranges_win_before_priority() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Broad", &[], &["QA1..QA100@9"]),
            concept(2, "Specific", &[], &["QA10..QA20@0"]),
        ]).unwrap();
        assert_eq!(taxonomy.matching_concept_ids("lcc", "QA15"), vec![2]);
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
        ]).unwrap();
        assert_eq!(taxonomy.lcc_match_candidates("QA10"), vec![3, 6]);
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
    fn bundled_revision_constant_tracks_embedded_taxonomy() {
        assert_eq!(default_runtime().revision_id, BUNDLED_UNIFIED_TAXONOMY_REVISION_ID);
    }

    #[test]
    fn ranges_with_item_metadata_require_full_containment() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Language", &[], &["P100..P200"]),
            concept(2, "Partial range", &[1], &["P121..P130"]),
            concept(3, "Cross subclass", &[], &["QA75..QB76"]),
            concept(4, "Start only", &[3], &["QA75..QA76"]),
        ]).unwrap();
        assert_eq!(taxonomy.matching_concept_ids("lcc", "P121-143.3 .S66 2021"), vec![1]);
        assert_eq!(taxonomy.matching_concept_ids("lcc", "QA75-QB76 .S66 2021"), vec![3]);
        assert!(taxonomy.matching_concept_ids("lcc", "P121-201 .S66 2021").is_empty());
        assert!(taxonomy.matching_concept_ids("lcc", "P143.3-121 .S66 2021").is_empty());
    }

    #[test]
    fn printed_comparative_law_heading_preserves_its_complete_range() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Comparative law", &[], &["K520..K5582"]),
            concept(2, "Treaties", &[1], &["K524..K525"]),
        ]).unwrap();
        for raw in ["K(520) 5582", "K(520)-5582", "K(520) – 5582"] {
            let report = taxonomy.lcc_match_report(raw);
            assert_eq!(report.assignments.iter().map(|a| a.concept_id).collect::<Vec<_>>(), [1]);
            assert_eq!(report.assignments[0].evidence[0].method, crate::LccMatchMethod::Normalized);
            assert_eq!(report.assignments[0].evidence[0].code, raw);
        }
        assert!(taxonomy.matching_concept_ids("lcc", "K(521) 5582").is_empty());
    }

    #[test]
    fn clipped_tail_recovery_does_not_merge_away_complete_subjects() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Epistemology", &[], &["BD143..BD237"]),
            concept(2, "Psychology", &[], &["BF1..BF990"]),
            concept(3, "Logic", &[], &["BC1..BC999"]),
        ]).unwrap();
        let report = taxonomy.lcc_match_report("BD143-237BF201Q342BC");
        assert_eq!(report.assignments.iter().map(|a| a.concept_id).collect::<Vec<_>>(), [1,2], "{report:?}");
        assert_eq!(report.unresolved_fragments, ["BC"]);
    }

    #[test]
    fn joined_terminal_subclasses_keep_complete_preceding_codes() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Computing", &[], &["QA75..QA76.99"]),
            concept(2, "Artificial intelligence", &[], &["Q334..Q342"]),
        ]).unwrap();
        let raw = "QA76.9.D35Q334-342QA";
        let report = taxonomy.lcc_match_report(raw);
        assert_eq!(report.assignments.iter().map(|a| a.concept_id).collect::<Vec<_>>(), [1,2], "{report:?}");
        assert_eq!(report.unresolved_fragments, ["QA"]);
        assert!(report.assignments.iter().flat_map(|a| &a.evidence).all(|e| e.code != "QA"));
    }

    #[test]
    fn partial_lists_recover_complete_unicode_and_marc_members() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Computing", &[], &["QA75..QA76.99"]),
        ]).unwrap();
        for raw in ["$aQA76; UNKNOWN", "QA75–QA76; UNKNOWN", "QA76; 未分類", "UNKNOWN; $aQA76"] {
            let report = taxonomy.lcc_match_report(raw);
            assert_eq!(report.assignments.iter().map(|a| a.concept_id).collect::<Vec<_>>(), [1], "{report:?}");
            assert!(!report.unresolved_fragments.is_empty());
            assert!(report.assignments[0].evidence.iter().all(|e| raw.contains(&e.code)));
        }
        assert!(taxonomy.lcc_match_report("未分類 QA76").assignments.is_empty());
    }

    #[test]
    fn ambiguous_characters_require_agreement_or_a_shared_parent() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Software", &[], &["QA76.76..QA76.76"]),
            concept(2, "Operating systems", &[1], &["QA76.76.O63"]),
        ]).unwrap();
        let report = taxonomy.lcc_match_report("QA76.76.063");
        assert_eq!(report.assignments.iter().map(|a| a.concept_id).collect::<Vec<_>>(), [2]);
        let evidence = &report.assignments[0].evidence[0];
        assert_eq!(evidence.method, crate::LccMatchMethod::Normalized);
        assert_eq!(evidence.interpretations, ["QA76.76.O63"]);
        assert_eq!(evidence.code, "QA76.76.063");
        for code in ["QA76.76 .063", "QA76.76 .063 2000"] {
            let report = taxonomy.lcc_match_report(code);
            assert_eq!(report.assignments.iter().map(|a| a.concept_id).collect::<Vec<_>>(), [2], "{report:?}");
            assert_eq!(report.unresolved_fragments, [code]);
            assert!(report.assignments[0].evidence[0].interpretations.iter().all(|v| !code.ends_with("2000") || v.ends_with("2000")));
        }
        assert!(taxonomy.lcc_match_report("WB18.02").assignments.is_empty());
        assert!(taxonomy.lcc_match_report("QA76.76.O63").assignments[0].evidence[0].interpretations.is_empty());
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Mathematics", &[], &[]),
            concept(2, "First numeric family", &[1], &["QA7..QA7.99"]),
            concept(3, "Second numeric family", &[1], &["QA705..QA705.99"]),
        ]).unwrap();
        let report = taxonomy.lcc_match_report("QA7O5.063");
        assert_eq!(report.assignments.iter().map(|a| a.concept_id).collect::<Vec<_>>(), [1], "{report:?}");
        assert_eq!(report.assignments[0].evidence[0].method, crate::LccMatchMethod::BroadFallback);
        assert!(report.assignments[0].evidence[0].interpretations.len() > 1);
        let unrelated = UnifiedTaxonomy::from_concepts(vec![
            concept(2, "First family", &[], &["QA7..QA7.99"]),
            concept(3, "Second family", &[], &["QA705..QA705.99"]),
        ]).unwrap();
        assert!(unrelated.lcc_match_report("QA7O5.063").assignments.is_empty());
        let incomplete = UnifiedTaxonomy::from_concepts(vec![concept(2, "First family", &[], &["QA7..QA7.99"])]).unwrap();
        assert!(incomplete.lcc_match_report("QA7O5.063").assignments.is_empty());
    }

    #[test]
    fn unfinished_codes_use_shared_parents_and_keep_original_fragments() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Computing", &[], &["QA75..QA76.99"]),
            concept(2, "First topic", &[1], &["QA76.1..QA76.2"]),
            concept(3, "Second topic", &[1], &["QA76.3..QA76.4"]),
        ]).unwrap();
        let report = taxonomy.lcc_match_report("QA76.");
        assert_eq!(report.assignments.iter().map(|a| a.concept_id).collect::<Vec<_>>(), [1]);
        assert_eq!(report.unresolved_fragments, ["QA76."]);
        let evidence = &report.assignments[0].evidence[0];
        assert_eq!(evidence.method, crate::LccMatchMethod::BroadFallback);
        assert_eq!(evidence.supporting_concept_ids, [1, 2, 3]);
        assert!(taxonomy.lcc_match_report("WB18.").assignments.is_empty());
        assert!(taxonomy.lcc_match_report("QA76.UNKNOWN").assignments.iter()
            .flat_map(|a| &a.evidence).all(|e| e.method != crate::LccMatchMethod::BroadFallback));
        assert_eq!(taxonomy.matching_concept_ids("lcc", "QA76.1QA76."), vec![2]);
        assert_eq!(taxonomy.lcc_match_report("QA76.1").assignments[0].evidence[0].method, crate::LccMatchMethod::Exact);
    }

    #[test]
    fn numeric_suffix_markers_recover_broad_subjects_without_stripping_scope() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Psychology", &[], &["BF1..BF990"]),
            concept(2, "Applied psychology", &[1], &["BF636..BF637.2"]),
            concept(3, "One application", &[2], &["BF637.C5"]),
            concept(4, "Later psychology", &[1], &["BF700..BF800"]),
        ]).unwrap();
        for (code, id) in [("BF637*", 1), ("BF637+", 1)] {
            let report = taxonomy.lcc_match_report(code);
            assert_eq!(report.assignments.iter().map(|a| a.concept_id).collect::<Vec<_>>(), [id]);
            assert_eq!(report.assignments[0].evidence[0].method, crate::LccMatchMethod::BroadFallback);
            assert_eq!(report.unresolved_fragments, [code]);
        }
        for code in ["LAW+", "CPB*", "WB18*", "BF637**", "BF637+UNKNOWN"] {
            assert!(taxonomy.lcc_match_report(code).assignments.is_empty(), "{code}");
        }
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Mathematics", &[], &[]),
            concept(2, "Early numbers", &[1], &["QA7..QA7.9"]),
            concept(3, "Later numbers", &[1], &["QA700..QA799"]),
        ]).unwrap();
        assert_eq!(taxonomy.matching_concept_ids("lcc", "QA7*"), vec![1]);
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Canadian literature", &[], &[]),
            concept(2, "French Canadian literature", &[1], &["PS9001..PS9649"]),
            concept(3, "General language classification", &[], &["P..PZ"]),
        ]).unwrap();
        assert_eq!(taxonomy.matching_concept_ids("lcc", "PS9573*"), vec![1]);
    }

    #[test]
    fn broad_fallback_includes_numeric_prefix_selectors() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Technology", &[], &["QA75..QA76.99"]),
            concept(2, "Computer science", &[1], &[]),
            concept(3, "Security", &[2], &["QA76.9.A1"]),
            concept(4, "Other applications", &[1], &["QA76.9.N*"]),
        ]).unwrap();
        let report = taxonomy.lcc_match_report("QA76.9.");
        assert_eq!(report.assignments[0].concept_id, 1);
        assert_eq!(report.assignments[0].evidence[0].supporting_concept_ids, [3, 4]);
    }

    #[test]
    fn open_range_and_single_child_do_not_choose_specific_topics() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Mathematics", &[], &["QA100..QA999"]),
            concept(2, "Probability", &[1], &["QA273..QA274.9"]),
            concept(3, "Algebra", &[1], &["QA300..QA399"]),
        ]).unwrap();
        assert_eq!(taxonomy.matching_concept_ids("lcc", "QA274-"), vec![1]);
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Computing", &[], &["QA75..QA76.99"]),
            concept(2, "One recorded topic", &[1], &["QA76.9.A1"]),
        ]).unwrap();
        assert_eq!(taxonomy.matching_concept_ids("lcc", "QA76.9."), vec![1]);
    }

    #[test]
    fn complete_joined_decimal_calls_keep_both_subjects() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Computing", &[], &["QA75..QA76.99"]),
            concept(2, "Microprogramming", &[1], &["QA76.76.M52"]),
            concept(3, "Software engineering", &[1], &["QA76.758"]),
        ]).unwrap();
        assert_eq!(taxonomy.matching_concept_ids("lcc", "QA76.76.M52QA76.758"), vec![2,3]);
        assert_eq!(taxonomy.matching_concept_ids("lcc", "QA76.76.M52QA76."), vec![2]);
        let report = taxonomy.lcc_match_report("QA76.76.M52QA76.");
        assert_eq!(report.unresolved_fragments, ["QA76."]);
        assert_eq!(report.assignments[0].evidence[0].method, crate::LccMatchMethod::RecoveredComponent);
        assert_eq!(taxonomy.matching_concept_ids("lcc", "QA76.76.M52QA76.758UNKNOWN"), vec![2]);
    }

    fn concept(id: i64, label: &str, parents: &[i64], lcc: &[&str]) -> UnifiedConceptDefinition {
        let source_selectors = if lcc.is_empty() { BTreeMap::new() } else { BTreeMap::from([(LCC_SYSTEM_ID.to_owned(), lcc.iter().map(|value| (*value).to_owned()).collect())]) };
        UnifiedConceptDefinition::new(id.to_owned(), label.to_owned(), parents.iter().map(|value| (*value).to_owned()).collect(), source_selectors, Vec::new())
    }

    #[test]
    fn topic_cutter_ranges_respect_decimal_bounds_and_filing_suffixes() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Germany", &[], &["DD801..DD801"]),
            concept(2, "Saxony", &[1], &["DD801.S31..DD801.S46"]),
            concept(3, "Württemberg", &[1], &["DD801.W6..DD801.W82"]),
        ]).unwrap();
        for code in ["DD801.S31", "DD801.S4", "DD801.S46 .B33 2007", "DD801.S460 B33 2007"] {
            assert_eq!(taxonomy.lcc_match_candidates(code), vec![2], "{code}");
        }
        for code in ["DD801", "DD801.A28", "DD801.S3", "DD801.S469", "DD801.W9"] {
            assert_eq!(taxonomy.lcc_match_candidates(code), vec![1], "{code}");
        }
        assert_eq!(taxonomy.lcc_match_candidates("DD801.W7 .B2 2005"), vec![3]);
        assert_eq!(taxonomy.lcc_match_candidates("DD801.S4 VOL. III"), vec![2]);
        assert_eq!(taxonomy.lcc_match_candidates("DD801.S469 VOL. III"), vec![1]);
    }

    #[test]
    fn variable_cutter_matching_does_not_pick_a_country_or_first_endpoint() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Small business by country", &[], &["HD2346.A..HD2346.Z"]),
            concept(2, "A country", &[1], &["HD2346.A..HD2346.B"]),
            concept(3, "I country", &[1], &["HD2346.I4..HD2346.I5"]),
        ]).unwrap();
        assert_eq!(taxonomy.lcc_match_candidates("HD2346.A-ZI.x (H73)+"), vec![1]);
        assert_eq!(taxonomy.lcc_match_candidates("HD2346.I4"), vec![3]);
    }

    #[test]
    fn explicit_mlc_subclasses_use_existing_heading_without_inventing_numbers() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Literature", &[], &["PQ", "(P)"]),
            concept(2, "Author", &[1], &["PQ8550.426.A74"]),
        ]).unwrap();
        assert_eq!(taxonomy.lcc_match_candidates("MLCF 2013/40001 (PQ)"), vec![1]);
        assert_eq!(taxonomy.lcc_match_candidates("PQ8550.426.A74"), vec![2]);
        assert!(taxonomy.lcc_match_candidates("MLCF 2013/40001 (ZZ)").is_empty());
    }

    #[test]
    fn placeholder_main_class_does_not_supply_a_subclass() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Language and literature", &[], &["(P)"]),
            concept(2, "Kannada author", &[1], &["PL4659.L33"]),
        ]).unwrap();
        assert_eq!(taxonomy.lcc_match_candidates("MLCxxx (P)"), vec![1]);
        assert_eq!(taxonomy.lcc_match_candidates("PL4659.L33"), vec![2]);
        assert!(taxonomy.lcc_match_candidates("MLCxxx").is_empty());
    }

    #[test]
    fn bracketed_years_keep_exact_edition_and_cutter_matches() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Topic", &[], &["QA76..QA76"]),
            concept(2, "Cutter topic", &[1], &["QA76.A3..QA76.A6"]),
            concept(3, "Edition", &[2], &["QA76.A4 1999"]),
        ]).unwrap();
        assert_eq!(taxonomy.lcc_match_candidates("QA76.A4 [1999]"), vec![3]);
        assert_eq!(taxonomy.lcc_match_candidates("QA76.A4 [2000]"), vec![2]);
        assert_eq!(taxonomy.lcc_match_candidates("QA76.A69 [1999]"), vec![1]);
    }

    #[test]
    fn table_notes_preserve_full_numeric_and_cutter_span_containment() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Economics", &[], &["HC1..HC999"]),
            concept(2, "Topic", &[1], &["HC40..HC60"]),
            concept(3, "Narrow topic", &[2], &["HC50.A3..HC50.A6"]),
        ]).unwrap();
        assert_eq!(taxonomy.lcc_match_candidates("HC40-HC80 (H15)+"), vec![1]);
        assert_eq!(taxonomy.lcc_match_candidates("HC40-HC60 (H15)+"), vec![2]);
        assert_eq!(taxonomy.lcc_match_candidates("HC50.A4 (H15)+"), vec![3]);
        assert_eq!(taxonomy.lcc_match_candidates("[HC50.A4] (H15)+"), vec![3]);
        assert_eq!(taxonomy.lcc_match_candidates("HC50.A4-HC50.A8 (H15)+"), vec![2]);
    }

    #[test]
    fn plus_scopes_cannot_select_just_the_first_number_or_partial_cutter_family() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Forms", &[], &["PK2916-2947"]),
            concept(2, "Poetry only", &[1], &["PK2916"]),
            concept(3, "Period", &[], &["F123", "F123..F123"]),
            concept(4, "Partial Cutter family", &[3], &["F123..F123.M"]),
        ]).unwrap();
        assert_eq!(taxonomy.lcc_match_candidates("PK2916+"), vec![1]);
        assert_eq!(taxonomy.lcc_match_candidates("F123+"), vec![3]);
        assert_eq!(taxonomy.lcc_match_candidates("IN PROCESS [F123+]"), vec![3]);
        assert_eq!(taxonomy.lcc_match_candidates("PK2916"), vec![2]);
    }

    #[test]
    fn single_pz_title_letters_preserve_cutter_bounds_and_exact_evidence() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Children's literature", &[], &["PZ7..PZ7"]),
            concept(2, "Author group", &[1], &["PZ7.S26..PZ7.S27"]),
            concept(3, "Exact original", &[2], &["PZ7.S268 E"]),
        ]).unwrap();
        assert_eq!(taxonomy.lcc_match_candidates("PZ7.S268 E"), vec![3]);
        assert_eq!(taxonomy.lcc_match_candidates("PZ7.S268 F"), vec![2]);
        assert_eq!(taxonomy.lcc_match_candidates("PZ7.S28 E"), vec![1]);
    }

    #[test]
    fn series_numbering_does_not_change_cutter_subjects() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Mathematics", &[], &["QA3..QA3"]),
            concept(2, "Selected series", &[1], &["QA3.A5..QA3.A6"]),
        ]).unwrap();
        assert_eq!(taxonomy.lcc_match_candidates("QA3 .A572 SER. 2, VOL. 130"), vec![2]);
        assert_eq!(taxonomy.lcc_match_candidates("QA3 .A7 NEW SER.:V.1"), vec![1]);
        assert!(taxonomy.lcc_match_candidates("QA3 .A572 SER. 2 UNKNOWN").is_empty());
    }

    #[test]
    fn item_shaped_list_tails_do_not_create_spurious_subjects() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Psychoanalysis", &[], &["BF175..BF175"]),
            concept(2, "Philosophy", &[], &["B1..B100"]),
        ]).unwrap();
        assert_eq!(taxonomy.matching_concept_ids(LCC_SYSTEM_ID, "BF175; B43"), vec![1]);
        assert_eq!(taxonomy.matching_concept_ids(LCC_SYSTEM_ID, "BF175; $aB43"), vec![1,2]);
        assert_eq!(taxonomy.matching_concept_ids(LCC_SYSTEM_ID, "BF175; [B43]"), vec![1,2]);
        assert_eq!(taxonomy.matching_concept_ids(LCC_SYSTEM_ID, "BF175 B43"), vec![1]);
    }

    #[test]
    fn explicit_lists_assign_all_subjects_without_accepting_ambiguous_components() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Computing", &[], &["QA76..QA77"]),
            concept(2, "Physics", &[], &["QC1..QC10"]),
            concept(3, "Conflicting physics", &[], &["QC5..QC5"]),
            concept(4, "Other conflicting physics", &[], &["QC5..QC5"]),
        ]).unwrap();
        assert_eq!(taxonomy.matching_concept_ids(LCC_SYSTEM_ID, "QA76; QC1"), vec![1,2]);
        assert_eq!(taxonomy.lcc_match_candidates("QA76; QC1"), vec![1,2]);
        assert_eq!(taxonomy.matching_concept_ids(LCC_SYSTEM_ID, "QA76; QA77"), vec![1]);
        assert_eq!(taxonomy.matching_concept_ids(LCC_SYSTEM_ID, "[QA76; QC1]"), vec![1,2]);
        assert_eq!(taxonomy.matching_concept_ids(LCC_SYSTEM_ID, "QA76; QC5"), vec![1]);
        assert_eq!(taxonomy.lcc_match_report("QA76; QC5").unresolved_fragments, ["QC5"]);
        assert_eq!(taxonomy.lcc_match_report("QA76; QC1").assignments.len(), 2);
        assert_eq!(taxonomy.matching_concept_ids(LCC_SYSTEM_ID, "QA76; QB1"), vec![1]);
        assert_eq!(taxonomy.matching_concept_ids(LCC_SYSTEM_ID, "QA76; QC1 truncated"), vec![1,2]);
    }

    #[test]
    fn marc_item_cutters_retain_specific_subject_evidence() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Computing", &[], &["QA76..QA77"]),
            concept(2, "Security", &[1], &["QA76.9.A25..QA76.9.A25"]),
        ]).unwrap();
        assert_eq!(taxonomy.lcc_match_candidates("QA76.9$b.A25 2008"), vec![2]);
        assert_eq!(taxonomy.lcc_match_candidates("$aQA76.9$b.A25 2008"), vec![2]);
        assert!(taxonomy.lcc_match_candidates("QA76.9$b.A25$aQC1").is_empty());
    }

    #[test]
    fn bare_subclass_alternatives_recover_only_their_common_subject() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Literature", &[], &["PS"]),
            concept(2, "Fiction", &[1], &["PZ"]),
            concept(3, "Law", &[], &["KJ", "KK"]),
            concept(4, "Medicine", &[], &["RC"]),
            concept(5, "Psychology", &[], &["BF"]),
        ]).unwrap();
        for (code, expected) in [("PS/PZ", 1), ("KJ/KK", 3)] {
            let report = taxonomy.lcc_match_report(code);
            assert_eq!(report.assignments.iter().map(|a| a.concept_id).collect::<Vec<_>>(), vec![expected]);
            assert_eq!(report.assignments[0].evidence[0].method, crate::LccMatchMethod::BroadFallback);
            assert_eq!(report.unresolved_fragments, [code]);
        }
        for code in ["RC/BF", "PS/UNKNOWN", "PS/PZ/", "PS/PZ prose"] {
            assert!(taxonomy.lcc_match_report(code).assignments.is_empty(), "{code}");
        }
    }

    #[test]
    fn unmarked_preliminary_range_requires_the_published_complete_scope() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Terrorism", &[], &["HV6433.A..HV6433.Z"]),
            concept(2, "Partial geography", &[1], &["HV6433.A..HV6433.M"]),
        ]).unwrap();
        assert_eq!(taxonomy.lcc_match_candidates("HV6433.A-ZI"), vec![1]);
        assert!(taxonomy.lcc_match_candidates("HV6433.A2-ZI").is_empty());
        assert!(taxonomy.lcc_match_candidates("HV6433.A-ZI1A").is_empty());
    }

    #[test]
    fn abbreviated_author_criticism_range_keeps_author_and_both_bounds() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Literature", &[], &["PK2631..PK2659"]),
            concept(2, "Author", &[1], &["PK2659.P348..PK2659.P348"]),
            concept(3, "Criticism", &[2], &["PK2659.P348.Z5..PK2659.P348.Z999"]),
            concept(4, "Partial criticism", &[3], &["PK2659.P348.Z5..PK2659.P348.Z8"]),
        ]).unwrap();
        for code in ["PK2659.P348Z5-Z999", "PK2659.P348.Z5-.Z999"] {
            assert_eq!(taxonomy.lcc_match_candidates(code), vec![3], "{code}");
            let report = taxonomy.lcc_match_report(code);
            assert_eq!(report.assignments[0].evidence[0].code, code);
        }
        assert_eq!(taxonomy.lcc_match_candidates("PK2659.U32Z5-Z999"), vec![1]);
        assert!(taxonomy.lcc_match_candidates("PK2659.P348Z5-Z9999").is_empty());
    }

    #[test]
    fn incomplete_table_author_requires_whole_initial_family() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Literature", &[], &["PR9499..PR9500"]),
            concept(2, "S authors", &[1], &["PR9499.3.S..PR9499.3.S"]),
            concept(3, "Some S authors", &[2], &["PR9499.3.S..PR9499.3.S5"]),
            concept(4, "One author criticism", &[2], &["PR9499.3.S53.Z5..PR9499.3.S53.Z999"]),
        ]).unwrap();
        for code in ["PR9499.3.S (P-PZ40:.xZ5-.xZ999)+", "PR9499.3.S.xZ5-Z999 (P-PZ40)+"] {
            assert_eq!(taxonomy.lcc_match_candidates(code), vec![2]);
        }
        assert_eq!(taxonomy.lcc_match_candidates("PR9499.3.S53 (P-PZ40:.xZ5-.xZ999)+"), vec![4]);
    }

    #[test]
    fn nested_preliminary_span_retains_prefix_and_entire_language_range() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Text", &[], &["BL1140.4.B432..BL1140.4.B432"]),
            concept(2, "Language range", &[1], &["BL1140.4.B432.A..BL1140.4.B432.Z"]),
            concept(3, "Partial languages", &[2], &["BL1140.4.B432.A..BL1140.4.B432.M"]),
        ]).unwrap();
        assert_eq!(taxonomy.lcc_match_candidates("BL1140.4.B432A-ZE 2009 (BL3)+"), vec![2]);
        assert_eq!(taxonomy.lcc_match_candidates("BL1140.4.B432A-.B432ZE 2009 (BL3)+"), vec![2]);
        assert!(taxonomy.lcc_match_candidates("BL1140.4.B432A-.B433ZE 2009 (BL3)+").is_empty());
        assert!(taxonomy.lcc_match_candidates("BL1140.4.C432A-ZE 2009 (BL3)+").is_empty());
    }

    #[test]
    fn qualified_table_span_retains_author_and_all_criticism_endpoints() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Author", &[], &["PR9499.3.S53..PR9499.3.S53"]),
            concept(2, "Criticism", &[1], &["PR9499.3.S53.Z5..PR9499.3.S53.Z999"]),
            concept(3, "Partial criticism", &[2], &["PR9499.3.S53.Z5..PR9499.3.S53.Z7"]),
        ]).unwrap();
        assert_eq!(taxonomy.lcc_match_candidates("PR9499.3.S53 (P-PZ40:.xZ5-.xZ999)+"), vec![2]);
        assert!(taxonomy.lcc_match_candidates("PR9499.3.S54 (P-PZ40:.xZ5-.xZ999)+").is_empty());
        assert_eq!(taxonomy.lcc_match_candidates("PR9499.3.S53.xZ5-Z999 (P-PZ40)+"), vec![2]);
        assert!(taxonomy.lcc_match_candidates("PR9499.3.S54.xZ5-Z999 (P-PZ40)+").is_empty());
    }

    #[test]
    fn repeated_ranges_keep_full_containment_and_original_exact_evidence() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Management", &[], &["HD28..HD70"]),
            concept(2, "Partial range", &[1], &["HD28..HD50"]),
            concept(3, "Exact original", &[1], &["HD28-70HD28-70HD28-70"]),
        ]).unwrap();
        assert_eq!(taxonomy.lcc_match_candidates("HD28-70HD28-70"), vec![1]);
        assert_eq!(taxonomy.lcc_match_candidates("HD28-70HD28-HD70"), vec![1]);
        assert_eq!(taxonomy.lcc_match_candidates("HD28-HD70HD28-70"), vec![1]);
        assert!(taxonomy.lcc_match_candidates("HD28-70HD28-HD70HD28").is_empty());
        assert_eq!(taxonomy.lcc_match_candidates("HD28-70HD28-70HD28-70"), vec![3]);
        assert_eq!(taxonomy.lcc_match_candidates("[HD28-70HD28-70]"), vec![1]);
        assert!(taxonomy.lcc_match_candidates("HD28-70HD28-7").is_empty());
    }

    #[test]
    fn table_range_upper_endpoint_keeps_author_and_full_span() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Literature", &[], &["PL4718..PL4719"]),
            concept(2, "Author criticism", &[1], &["PL4718.9.B33.Z5..PL4718.9.B33.Z999"]),
            concept(3, "Partial criticism range", &[2], &["PL4718.9.B33.Z5..PL4718.9.B33.Z7"]),
        ]).unwrap();
        assert_eq!(taxonomy.lcc_match_candidates("PL4718.9.B33Z5-Z999 (P-PZ40)+"), vec![2]);
        assert_eq!(taxonomy.lcc_match_candidates("PL4718.9.B33Z5-.Z999 (P-PZ40)+"), vec![2]);
        assert_eq!(taxonomy.lcc_match_candidates("PL4718.9.B34Z5-Z999 (P-PZ40)+"), vec![1]);
    }

    #[test]
    fn preliminary_plus_calls_do_not_infer_country_cutters() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Terrorism by country", &[], &["HV6433.A..HV6433.Z"]),
            concept(2, "India", &[1], &["HV6433.I4..HV6433.I4"]),
            concept(3, "Partial countries", &[1], &["HV6433.A..HV6433.M"]),
        ]).unwrap();
        assert_eq!(taxonomy.lcc_match_candidates("HV6433.A-ZI+"), vec![1]);
        assert_eq!(taxonomy.lcc_match_candidates("HV6433.A-ZI4+"), vec![1]);
        assert_eq!(taxonomy.lcc_match_candidates("HV6433.A-ZI"), vec![1]);
    }

    #[test]
    fn documented_placeholders_require_the_whole_country_interval() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Terrorism by country", &[], &["HV6433.A..HV6433.Z"]),
            concept(2, "I countries", &[1], &["HV6433.I..HV6433.I"]),
            concept(3, "Partial countries", &[1], &["HV6433.A..HV6433.M"]),
        ]).unwrap();
        assert_eq!(taxonomy.lcc_match_candidates("HV6433.A-ZI.x+"), vec![1]);
        assert!(taxonomy.lcc_match_candidates("HV6433.A-ZI.x99+").is_empty());
    }

    #[test]
    fn numeric_qualified_ranges_keep_full_scope_and_significant_zeroes() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Newspapers", &[], &["PN5499..PN5499"]),
            concept(2, "A42 scope", &[1], &["PN5499.A42.A6..PN5499.A42.Z"]),
            concept(3, "A402 scope", &[1], &["PN5499.A402.A6..PN5499.A402.Z"]),
            concept(4, "Partial scope", &[2], &["PN5499.A42.A6..PN5499.A42.M"]),
        ]).unwrap();
        assert_eq!(taxonomy.lcc_match_candidates("PN5499.A4 (PN4:.x2A6-.x2Z)+"), vec![2]);
        assert_eq!(taxonomy.lcc_match_candidates("PN5499.A40 (PN4:.x2A6-.x2Z)+"), vec![3]);
    }

    #[test]
    fn numeric_nested_tables_require_whole_outer_range() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Planning by country", &[], &["HT169.A..HT169.Z"]),
            concept(2, "One country family", &[1], &["HT169.I..HT169.I"]),
            concept(3, "Partial range", &[1], &["HT169.A..HT169.M"]),
        ]).unwrap();
        assert_eq!(taxonomy.lcc_match_candidates("HT169.A-ZI.x2A-.x2ZD (H73)+"), vec![1]);
        assert!(taxonomy.lcc_match_candidates("HT169.A-ZI.x2A-.x2ZD (H15)+").is_empty());
    }

    #[test]
    fn nested_variable_tables_do_not_invent_a_person_cutter() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Biography scope", &[], &["BL1175.A..BL1175.Z"]),
            concept(2, "One initial", &[1], &["BL1175.G..BL1175.G"]),
            concept(3, "Partial scope", &[1], &["BL1175.A..BL1175.M"]),
        ]).unwrap();
        assert_eq!(taxonomy.lcc_match_candidates("BL1175.A-ZG.xA6-.xZ (BL4)+"), vec![1]);
        assert!(taxonomy.lcc_match_candidates("BL1175.A-ZG.xA6-.x7 (BL4)+").is_empty());
    }

    #[test]
    fn historical_biography_ranges_preserve_person_scope() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Biography", &[], &["DS481..DS481"]),
            concept(2, "Gandhi biography", &[1], &["DS481.G3.A6..DS481.G3.Z"]),
            concept(3, "Partial biography", &[2], &["DS481.G3.A6..DS481.G3.M"]),
        ]).unwrap();
        assert_eq!(taxonomy.lcc_match_candidates("DS481.G3 (DS7:.xA6-.xZ)+"), vec![2]);
        assert_eq!(taxonomy.lcc_match_candidates("DS481.G (DS7:.xA6-.xZ)+"), vec![1]);
        assert!(taxonomy.lcc_match_candidates("DS481.G3 (DS-DX7:.xA6-.xZ)+").is_empty());
    }

    #[test]
    fn nested_table_ranges_require_complete_upper_scope() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Philosophy", &[], &["B1..B200"]),
            concept(2, "General works", &[1], &["B133.S5.A6..B133.S5.Z"]),
            concept(3, "Partial works", &[2], &["B133.S5.A6..B133.S5.M"]),
        ]).unwrap();
        for code in ["B133.S5A6-S5Z (B-BJ5)+", "B133.S5A6-Z (B-BJ5)+", "B133.S5A6-S5Z 2004 (B-BJ5)+"] {
            assert_eq!(taxonomy.lcc_match_candidates(code), vec![2], "{code}");
        }
    }

    #[test]
    fn terminal_slashes_require_whole_filing_family_containment() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Class", &[], &["DA690", "DA690..DA690"]),
            concept(2, "S family", &[1], &["DA690.S..DA690.S"]),
            concept(3, "Partial family", &[2], &["DA690.S..DA690.S5"]),
            concept(4, "S4 family", &[2], &["DA690.S4..DA690.S4"]),
            concept(5, "Partial second Cutter", &[4], &["DA690.S4.A..DA690.S4.M"]),
        ]).unwrap();
        assert_eq!(taxonomy.lcc_match_candidates("DA690.S/"), vec![2]);
        assert_eq!(taxonomy.lcc_match_candidates("DA690.S4/"), vec![4]);
        assert_eq!(taxonomy.lcc_match_candidates("[DA690.S/]"), vec![2]);
        assert_eq!(taxonomy.lcc_match_candidates("DA690/"), vec![1]);
        assert!(taxonomy.lcc_match_candidates("ZZ690.S/").is_empty());
    }

    #[test]
    fn source_reviewed_unmarked_ranges_do_not_guess_cutters_or_match_partial_coverage() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Translations", &[], &["PK1716.5..PK1716.5"]),
            concept(2, "Partial", &[1], &["PK1716.5.A..PK1716.5.R"]),
            concept(3, "English", &[2], &["PK1716.5.E5"]),
            concept(4, "Authors", &[], &["PK2598.A3..PK2598.Z5"]),
            concept(5, "Partial authors", &[4], &["PK2598.A3..PK2598.Z4"]),
        ]).unwrap();
        assert_eq!(taxonomy.lcc_match_candidates("PK1716.5.A-ZE"), vec![1]);
        assert_eq!(taxonomy.lcc_match_candidates("PK1716.5.E5"), vec![3]);
        assert_eq!(taxonomy.lcc_match_candidates("PK2598.A3-Z5.P"), vec![4]);
        let exact = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Translations", &[], &["PK1716.5..PK1716.5"]),
            concept(2, "Explicit evidence", &[1], &["PK1716.5.A-ZE"]),
        ]).unwrap();
        assert_eq!(exact.lcc_match_candidates("PK1716.5.A-ZE"), vec![2]);
    }

    #[test]
    fn preliminary_initials_do_not_select_a_partial_range_or_guessed_cutter() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Local economics", &[], &["HC437..HC437"]),
            concept(2, "Partial range", &[1], &["HC437.A..HC437.R"]),
            concept(3, "Manipur", &[2], &["HC437.M33"]),
        ]).unwrap();
        assert_eq!(taxonomy.lcc_match_candidates("HC437.A-ZM (H15)+"), vec![1]);
        assert_eq!(taxonomy.lcc_match_candidates("HC437.A-ZM33 (H15)+"), vec![1]);
        assert_eq!(taxonomy.lcc_match_candidates("HC437.M33"), vec![3]);
    }

    #[test]
    fn multiple_table_notes_preserve_full_span_and_original_exact_selector() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Literature", &[], &["PL4758..PL4759"]),
            concept(2, "Partial Cutter span", &[1], &["PL4758.85.A..PL4758.85.R"]),
            concept(3, "Exact evidence", &[1], &["PL4758.85.A-ZS.X (P-PZ25; P-PZ40)+"]),
        ]).unwrap();
        assert_eq!(taxonomy.lcc_match_candidates("PL4758.85.A-ZS.x (P-PZ25, 26)+"), vec![1]);
        assert_eq!(taxonomy.lcc_match_candidates("PL4758.85.A-ZS.x (P-PZ25; P-PZ40)+"), vec![3]);
        assert!(taxonomy.lcc_match_candidates("PL4758.85.A-ZS.x (P-PZ25, UNKNOWN)+").is_empty());
    }

    #[test]
    fn no_cutter_work_marks_do_not_invent_author_cutters() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Modern authors", &[], &["PS3500..PS3599"]),
            concept(2, "Specific author", &[1], &["PS3566.I372"]),
        ]).unwrap();
        assert_eq!(taxonomy.lcc_match_candidates("PS3566 PIC"), vec![1]);
        assert_eq!(taxonomy.lcc_match_candidates("PS3566.I372"), vec![2]);
        assert_eq!(taxonomy.lcc_match_candidates("[PS3566 PIC]"), vec![1]);
    }

    #[test]
    fn doubled_period_keeps_cutter_range_boundaries() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Germany", &[], &["DD801..DD801"]),
            concept(2, "Saxony", &[1], &["DD801.S31..DD801.S46"]),
        ]).unwrap();
        assert_eq!(taxonomy.lcc_match_candidates("DD801..S4 2003"), vec![2]);
        assert_eq!(taxonomy.lcc_match_candidates("DD801. .S469 2003"), vec![1]);
        assert_eq!(taxonomy.lcc_match_candidates("[DD801..S4 2003]"), vec![2]);
        assert!(taxonomy.lcc_match_candidates("DD801...S4 2003").is_empty());
    }

    #[test]
    fn documented_sociology_heading_uses_existing_subclass_assignment() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Social sciences", &[], &["H..HX"]),
            concept(2, "Sociology", &[1], &["HM", "HM1-HM299", "HM401-HM1281"]),
        ]).unwrap();
        assert_eq!(taxonomy.lcc_match_candidates("HM(1)-1281"), vec![2]);
        assert_eq!(taxonomy.lcc_match_candidates("[HM(1)-1281]"), vec![2]);
        assert!(taxonomy.lcc_match_candidates("HM(1)-1281QA76").is_empty());
    }

    #[test]
    fn period_year_retains_topic_bounds_and_exact_year_selectors() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Germany", &[], &["DD801..DD801"]),
            concept(2, "Saxony", &[1], &["DD801.S31..DD801.S46"]),
            concept(3, "Selected edition", &[2], &["DD801.S4 2003"]),
        ]).unwrap();
        assert_eq!(taxonomy.lcc_match_candidates("DD801.S4.2003"), vec![3]);
        assert_eq!(taxonomy.lcc_match_candidates("DD801.S4.2004"), vec![2]);
        assert_eq!(taxonomy.lcc_match_candidates("DD801.S469.2003"), vec![1]);
        assert_eq!(taxonomy.lcc_match_candidates("[DD801.S4.2003]"), vec![3]);
    }

    #[test]
    fn display_brackets_preserve_cutter_and_span_boundaries() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Germany", &[], &["DD801..DD801"]),
            concept(2, "Saxony", &[1], &["DD801.S31..DD801.S46"]),
        ]).unwrap();
        for code in ["DD801.S31", "DD801.S46 .B33 2007", "DD801.S469", "DD801.S31-DD801.S46", "DD801.S31-DD801.S469"] {
            assert_eq!(taxonomy.lcc_match_candidates(&format!("[{code}]")), taxonomy.lcc_match_candidates(code), "{code}");
        }
        assert_eq!(taxonomy.lcc_match_candidates("[DD801.S4]"), vec![2]);
        assert_eq!(taxonomy.lcc_match_candidates("[DD801.S469]"), vec![1]);
        assert!(taxonomy.lcc_match_candidates("[FIC]").is_empty());
    }

    #[test]
    fn browse_order_comes_from_numeric_classes_and_decimal_cutters() {
        let concepts = vec![
            concept(1, "Without a code", &[], &[]),
            concept(2, "Later Saxony", &[], &["DD801.S46..DD801.S49", "DD900"]),
            concept(3, "Weimar Saxony", &[], &["DD801.S453"]),
            concept(4, "Early Saxony", &[], &["DD801.S4"]),
            concept(5, "Ten", &[], &["QA10"]),
            concept(6, "Two", &[], &["QA2"]),
            concept(7, "One tenth", &[], &["QA10.1"]),
            concept(8, "Eleven hundredths", &[], &["QA10.11"]),
            concept(9, "Same later Saxony", &[], &["DD801.S460"]),
        ];
        let order = lcc_subject_sort_orders(&concepts);
        for (left, right) in [(4, 3), (3, 2), (2, 6), (6, 5), (5, 7), (7, 8), (8, 1)] {
            assert!(order[&left] < order[&right], "{left} before {right}");
        }
        assert_eq!(order[&2], order[&9]);
        assert_eq!(order[&1], i64::MAX);
        let reversed = concepts.into_iter().rev().collect::<Vec<_>>();
        assert_eq!(order, lcc_subject_sort_orders(&reversed));
    }

    #[test]
    fn validation_rejects_parallel_taxonomy_structures() {
        let redundant_wrapper = vec![concept(22, "History", &[], &[]), concept(8, "History.", &[22], &[])];
        assert!(validate(&redundant_wrapper).unwrap_err().contains("direct parent"));

        let duplicate_siblings = vec![concept(22, "Humanities", &[], &[]), concept(15, "By period", &[22], &[]), concept(21, "By Period.", &[22], &[])];
        assert!(validate(&duplicate_siblings).unwrap_err().contains("beneath parent"));

        let duplicate_source_record = vec![
            concept(22, "Humanities", &[], &[]),
            concept(15, "Ethnography", &[22], &["GN301"]),
            concept(20, "Social Sciences", &[], &[]),
            concept(21, "Ethnography.", &[20], &["GN301"]),
        ];
        assert!(validate(&duplicate_source_record).unwrap_err().contains("duplicate source selector"));
    }

    #[test]
    fn validation_preserves_meaningful_programming_language_punctuation() {
        let programming_languages =
            vec![concept(22, "Programming Languages", &[], &[]), concept(5, "C", &[22], &[]), concept(7, "C#", &[22], &[]), concept(6, "C++", &[22], &[]), concept(10, ".NET", &[22], &[])];

        assert!(validate(&programming_languages).is_ok());
    }

    #[test]
    fn broad_ranges_do_not_count_as_exact_local_code_coverage() {
        let taxonomy =
            UnifiedTaxonomy::from_concepts(vec![concept(22, "Social Sciences", &[], &[]), concept(4, "Business", &[22], &["HF1..HF6182@1"]), concept(1, "Accounting", &[4], &["HF5601"])]).unwrap();

        let broad_only = UnifiedTaxonomy::from_concepts(vec![concept(22, "Social Sciences", &[], &[]), concept(4, "Business", &[22], &["HF1..HF6182@1"])]).unwrap();
        assert!(!broad_only.matching_concept_ids(LCC_SYSTEM_ID, "HF5601").is_empty());
        assert!(!broad_only.has_exact_source_code(LCC_SYSTEM_ID, "HF5601"));
        assert!(taxonomy.has_exact_source_code(LCC_SYSTEM_ID, "hf5601"));
    }

    #[test]
    fn lcc_wildcards_require_a_curated_structural_subclass() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(22, "Subjects", &[], &[]),
            concept(13, "History", &[22], &["C*"]),
            concept(18, "Music", &[22], &["M*", "ML", "ML1..ML3930@1"]),
            concept(14, "Law", &[22], &["K*", "KKT1..KKT9999@1"]),
        ])
        .unwrap();

        for shelf_mark in ["CPB", "CPB Box no. 1574 vol. 4", "MLCS", "MLCS 2021/45060 (P)"] {
            assert!(taxonomy.matching_concept_ids(LCC_SYSTEM_ID, shelf_mark).is_empty(), "invalid shelf mark matched: {shelf_mark}");
        }
        assert_eq!(taxonomy.matching_concept_ids(LCC_SYSTEM_ID, "ML410"), vec![18]);
        assert_eq!(taxonomy.matching_concept_ids(LCC_SYSTEM_ID, "KKT2070"), vec![14]);
    }

    #[test]
    fn congressional_committee_hearings_do_not_match_public_administration() {
        install_authoritative_taxonomy_for_tests();
        for call_number in ["KF26", "KF27"] {
            let paths = unified_lcc_subject_paths(call_number);
            assert!(paths.iter().all(|path| !path.ends_with("Public Administration & Local Government")), "{call_number} incorrectly matched public administration: {paths:?}");
        }
    }



    #[test]
    fn subject_bibliography_topics_are_contextual_and_grouped() {
        install_authoritative_taxonomy_for_tests();
        let expected = [
            ("Z6658", "Subject bibliography / Medicine, Health & Disability / Medicine"),
            ("Z6201", "Subject bibliography / Humanities & Culture / History Bibliographies"),
            ("Z5521", "Subject bibliography / Natural Sciences & Mathematics / Chemistry"),
            ("Z7911", "Subject bibliography / Technology & Engineering / Technology & Applied Science"),
        ];

        for (call_number, suffix) in expected {
            let paths = unified_lcc_subject_paths(call_number);
            assert!(paths.iter().any(|path| path.ends_with(suffix)), "{call_number} did not resolve beneath {suffix}: {paths:?}");
        }

        let medicine_paths = unified_lcc_subject_paths("Z6658");
        assert!(medicine_paths.iter().all(|path| !path.ends_with(" / Medicine") || path.contains("Subject bibliography")));
    }


    #[test]
    fn decorative_arts_lcc_detail_routes_to_existing_concepts() {
        install_authoritative_taxonomy_for_tests();
        let expected = [
            ("NK3700", "Other arts & art industries / Ceramics"),
            ("NK5100", "Other arts & art industries / Glass"),
            ("NK6400", "Other arts & art industries / Art metalwork"),
            ("NK8555", "Other arts & art industries / Papercrafts"),
            ("NK8800", "Other arts & art industries / Textile arts"),
            ("NK9600", "Other arts & art industries / Woodwork"),
        ];

        for (call_number, suffix) in expected {
            let paths = unified_lcc_subject_paths(call_number);
            assert!(paths.iter().any(|path| path.ends_with(suffix)), "{call_number} did not resolve beneath {suffix}: {paths:?}");
        }
    }



    #[test]
    fn library_science_has_distinct_discipline_and_collection_routes() {
        install_authoritative_taxonomy_for_tests();
        let expected = [
            ("Z666.5", "Library & Information Science / Library Systems & Services / Information Organization"),
            ("Z674.2", "Library & Information Science / Library Systems & Services / Information Services"),
            ("Z678", "Library & Information Science / Library Administration / Library & Archive Operations"),
            ("Z679", "Library & Information Science / Library Administration / Library Buildings"),
            ("Z711", "Library & Information Science / Library Collections & Services / Public & Reference Services"),
            ("Z718.1", "Library & Information Science / Education & User Groups / Children’s Libraries"),
        ];

        for (call_number, suffix) in expected {
            let paths = unified_lcc_subject_paths(call_number);
            assert!(paths.iter().any(|path| path.ends_with(suffix)), "{call_number} did not resolve beneath {suffix}: {paths:?}");
            assert!(paths.iter().all(|path| !path.contains("Libraries & Collections / Library & Information Science")), "redundant library route remains for {call_number}: {paths:?}");
        }
    }
    #[test]
    fn cross_subclass_and_cutter_spans_require_complete_coverage() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "History", &[], &["D1..DX301"]),
            concept(2, "European history", &[1], &["DA1..DR999"]),
            concept(3, "Medicine", &[], &["R..RZ"]),
            concept(4, "Probability", &[], &["QA273.A1..QA274.9"]),
            concept(5, "Cutter interval", &[], &["QB1.A1..QB1.C5"]),
            concept(6, "Deep Cutter interval", &[], &["QC1.A1..QC1.C5.B9"]),
        ]).unwrap();
        for (code, expected) in [("D1-DX301",1), ("DA10-DR20",2), ("DB10",2),
            ("R-RZ",3), ("R-R",3), ("RA10-RC20",3), ("RC20",3), ("QA273.A1-274.9",4),
            ("QB1.A2-.C4",5)] {
            assert_eq!(taxonomy.matching_concept_ids("lcc",code),vec![expected],"{code}");
        }
        for code in ["D1-DX302", "D-DX", "QA273-274.9", "QB1.A2-1", "QB1.A2-.D1", "QC1.A1-1.C5", "QB1.A2-1.C", "R-Q"] {
            assert!(taxonomy.matching_concept_ids("lcc",code).is_empty(),"{code}");
        }
    }

    #[test]
    fn abbreviated_decimal_ranges_match_like_full_endpoints() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Operations", &[], &["T57.6..T57.97"]),
            concept(2, "Physical properties", &[], &["TA418.5..TA418.84"]),
        ]).unwrap();
        for (short, full, id) in [("T57.6-.97", "T57.6-T57.97", 1),
            ("TA418.5–.84", "TA418.5-TA418.84", 2)] {
            assert_eq!(taxonomy.matching_concept_ids("lcc", short), vec![id]);
            assert_eq!(taxonomy.matching_concept_ids("lcc", short), taxonomy.matching_concept_ids("lcc", full));
        }
        for code in ["T57.6-.98", "T57.6-.5", "TA418.5-.85"] {
            assert!(taxonomy.matching_concept_ids("lcc",code).is_empty(),"{code}");
        }
    }

    #[test]
    fn alphabetic_cutter_spans_require_full_interval_coverage() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Phenomenology", &[], &["B829.5.A..B829.5.Z"]),
            concept(2, "Partial interval", &[], &["QA76.B..QA76.D"]),
        ]).unwrap();
        for (code, id) in [("B829.5.A-Z", 1), ("B829.5.A3-Z", 1), ("B829.5.A-B829.5.Z", 1), ("QA76.B-D", 2)] {
            assert_eq!(taxonomy.matching_concept_ids("lcc", code), vec![id], "{code}");
        }
        for code in ["QA76.A-Z", "QA76.B-E", "B829.5.A-Z1"] {
            assert!(taxonomy.matching_concept_ids("lcc", code).is_empty(), "{code}");
        }
    }

    #[test]
    fn pz_work_marks_match_author_exceptions_and_numeric_ranges() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Juvenile fiction", &[], &["PZ5..PZ10.5"]),
            concept(2, "Author collection", &[1], &["PZ7.R79835"]),
        ]).unwrap();
        assert_eq!(taxonomy.matching_concept_ids("lcc", "PZ7.R79835 Ham 1999"), vec![2]);
        assert_eq!(taxonomy.matching_concept_ids("lcc", "PZ7.T47 Wh"), vec![1]);
        assert_eq!(taxonomy.matching_concept_ids("lcc", "PZ7.R79835 BOX NO. 4"), vec![2]);
    }

    #[test]
    fn separated_calls_with_volume_ranges_reach_numeric_and_cutter_selectors() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Philosophy", &[], &["B3000..B3999"]),
            concept(2, "Bounded author", &[1], &["B3279.H1..B3279.H9"]),
        ]).unwrap();
        for (code, id) in [("B'3279'H9'BD. 18-19", 2),
            ("B'3300'A1'VOL. 1-3", 1), ("B3279.H9 BD. 18-19", 2)] {
            assert_eq!(taxonomy.matching_concept_ids("lcc", code), vec![id], "{code}");
        }
        for code in ["B'3279'H9'BD. 18-UNKNOWN", "B'3279'H9'BD. 18-",
            "B'3279'H9'BD. 18-19 UNKNOWN"] {
            assert!(taxonomy.matching_concept_ids("lcc", code).is_empty(), "{code}");
        }
    }

    #[test]
    fn volume_ranges_do_not_become_classification_ranges_or_cutters() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Mathematics", &[], &["QA1..QA999"]),
            concept(2, "Series", &[1], &["QA3.L28"]),
            concept(3, "Bounded author", &[], &["QB1.A1..QB1.C5"]),
            concept(4, "Complete item", &[2], &["QA3.L28 VOL. 1"]),
        ]).unwrap();
        for (code, id) in [("QA3 .L28 vol. 1-3", 2), ("QA3.L28 vol. 1", 4),
            ("QA4.A1 no. 2", 1), ("QB1.C5 vol. 1-3", 3), ("QA3.L28 Suppl. 2", 2)] {
            assert_eq!(taxonomy.matching_concept_ids("lcc", code), vec![id], "{code}");
        }
        assert!(taxonomy.matching_concept_ids("lcc", "QB1.D1 vol. 1").is_empty());
    }

    #[test]
    fn custodial_assignments_do_not_replace_subjects_or_author_matches() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Literature", &[], &["PL2000..PL3000"]),
            concept(2, "Author", &[1], &["PL2780.F4"]),
        ]).unwrap();
        for code in ["PL2780.F4 1981b <Orien China>", "PL2780.F4 vol. 2 <Asian Japan>"] {
            assert_eq!(taxonomy.matching_concept_ids("lcc", code), vec![2]);
        }
        assert_eq!(taxonomy.matching_concept_ids("lcc", "PL2800.A1 <Hebr>"), vec![1]);
        assert_eq!(taxonomy.matching_concept_ids("lcc", "PL2780.F4 <UNKNOWN>"), vec![2]);
    }

    #[test]
    fn mlc_assignments_use_broad_curated_keys_and_routable_root_subjects() {
        let taxonomy = UnifiedTaxonomy::from_concepts(vec![
            concept(1, "Language and literature", &[], &["(P)"]),
            concept(2, "Agriculture", &[], &["(S)"]),
            concept(3, "Music", &[], &["M1..M9999"]),
        ]).unwrap();
        assert_eq!(taxonomy.matching_concept_ids("lcc", "MLCM 98/02114 (P)"), vec![1]);
        assert_eq!(taxonomy.matching_concept_ids("lcc", "MICROFILM 85/4003 (P)"), vec![1]);
        assert_eq!(taxonomy.matching_concept_ids("lcc", "MLCMJ 2003/00135 (S)"), vec![2]);
        assert!(taxonomy.matching_concept_ids("lcc", "MLCMJ 2003/00135").is_empty());
        assert!(taxonomy.matching_concept_ids("lcc", "MLCS 2000/12345 (B)").is_empty());
        assert!(taxonomy.runtime.routes.contains_key("Language and literature"));
    }

}
