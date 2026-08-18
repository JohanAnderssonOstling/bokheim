use super::ddc::canonical_ddc_notation;
use super::lcc::{compare_lcc_key, parse_lcc_call};
use super::{compact_whitespace, DDC_SYSTEM_ID, LCC_SYSTEM_ID, UNIFIED_VERSION};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, OnceLock, RwLock};

pub const DEFAULT_UNIFIED_TAXONOMY_SQLITE: &[u8] = include_bytes!("../../data/unified-taxonomy-v2.sqlite3");

static RUNTIME: OnceLock<RwLock<Arc<UnifiedTaxonomyRuntime>>> = OnceLock::new();

pub(super) struct UnifiedTaxonomyRuntime {
    pub(super) concepts: Arc<Vec<UnifiedConceptDefinition>>,
    pub(super) routes: Arc<BTreeMap<String, UnifiedConceptRoute>>,
    pub(super) paths_by_concept: Arc<BTreeMap<String, Vec<String>>>,
    selectors: Arc<BTreeMap<String, SelectorIndex>>,
    pub(super) source_labels: Arc<BTreeMap<String, BTreeSet<String>>>,
    release_id: String,
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

    pub fn release_id(&self) -> &str {
        &self.runtime.release_id
    }

    pub fn matching_concept_ids(&self, system_id: &str, code: &str) -> Vec<String> {
        matching_concepts(&self.runtime, system_id, code).into_iter().collect()
    }

    pub fn has_exact_source_code(&self, system_id: &str, code: &str) -> bool {
        has_exact_source_code(&self.runtime, system_id, code)
    }
}

#[derive(Default)]
struct SelectorIndex {
    exact: HashMap<String, BTreeSet<String>>,
    prefixes: Vec<(String, String)>,
    numeric_ranges: Vec<NumericSelector>,
    lcc_ranges: Vec<LccSelector>,
}

struct NumericSelector {
    start: u16,
    end: u16,
    concept_id: String,
}
struct LccSelector {
    start_letters: String,
    start_number: f64,
    end_letters: String,
    end_number: f64,
    priority: usize,
    concept_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnifiedConceptRoute {
    concept_id: String,
    path: String,
    parent_path: String,
    label: String,
    sort_order: i64,
}

impl UnifiedConceptRoute {
    pub fn concept_id(&self) -> &str {
        &self.concept_id
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
    concept_id: String,
    preferred_label: String,
    parent_ids: Vec<String>,
    parent_sort_orders: Vec<i64>,
    source_selectors: BTreeMap<String, Vec<String>>,
    source_labels: Vec<String>,
}

impl UnifiedConceptDefinition {
    pub fn new(concept_id: String, preferred_label: String, parent_ids: Vec<String>, source_selectors: BTreeMap<String, Vec<String>>, source_labels: Vec<String>) -> Self {
        let parent_sort_orders = vec![0; parent_ids.len()];
        Self { concept_id, preferred_label, parent_ids, parent_sort_orders, source_selectors, source_labels }
    }
    pub fn new_with_parent_sort_orders(concept_id: String, preferred_label: String, parent_ids: Vec<String>, parent_sort_orders: Vec<i64>, source_selectors: BTreeMap<String, Vec<String>>, source_labels: Vec<String>) -> Self {
        Self { concept_id, preferred_label, parent_ids, parent_sort_orders, source_selectors, source_labels }
    }
    pub fn concept_id(&self) -> &str {
        &self.concept_id
    }
    pub fn preferred_label(&self) -> &str {
        &self.preferred_label
    }
    pub fn parent_ids(&self) -> &[String] {
        &self.parent_ids
    }
    pub fn parent_sort_orders(&self) -> &[i64] {
        &self.parent_sort_orders
    }
    pub fn source_selectors(&self, system_id: &str) -> &[String] {
        self.source_selectors.get(system_id).map(Vec::as_slice).unwrap_or_default()
    }
    pub fn source_labels(&self) -> &[String] {
        &self.source_labels
    }
}

#[cfg(target_arch = "wasm32")]
#[derive(bincode::Decode)]
struct SnapshotRow {
    concept_id: String,
    preferred_label: String,
    parent_ids: String,
    parent_sort_orders: String,
    bisac_codes: String,
    ddc_codes: String,
    lcc_codes: String,
    source_labels: String,
}

pub(super) fn current() -> Arc<UnifiedTaxonomyRuntime> {
    RUNTIME.get_or_init(|| RwLock::new(default_runtime())).read().expect("unified taxonomy runtime lock must not be poisoned").clone()
}

pub fn unified_concept_definitions() -> Vec<UnifiedConceptDefinition> {
    current().concepts.as_ref().clone()
}

pub fn install_unified_taxonomy_concepts(concepts: Vec<UnifiedConceptDefinition>) -> Result<(), String> {
    let replacement = Arc::new(build_runtime(concepts)?);
    let runtime = RUNTIME.get_or_init(|| RwLock::new(replacement.clone()));
    *runtime.write().map_err(|_| "unified taxonomy runtime lock is poisoned".to_owned())? = replacement;
    Ok(())
}

/// Rebuilds the runtime from the embedded client snapshot plus a persisted
/// on-demand overlay. This also removes concepts from an obsolete overlay.
pub fn install_unified_taxonomy_overlay(concepts: Vec<UnifiedConceptDefinition>) -> Result<(), String> {
    let mut merged = default_runtime().concepts.iter().cloned().map(|concept| (concept.concept_id().to_owned(), concept)).collect::<BTreeMap<_, _>>();
    for concept in concepts {
        merged.insert(concept.concept_id().to_owned(), concept);
    }
    install_unified_taxonomy_concepts(merged.into_values().collect())
}

pub fn unified_taxonomy_release_id() -> String {
    current().release_id.clone()
}

pub fn unified_subject_paths(system_id: &str, code: &str) -> Vec<String> {
    let runtime = current();
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

pub fn unified_concept_id(path: &str) -> String {
    if let Some(route) = current().routes.get(path) {
        return route.concept_id.clone();
    }
    const NAMESPACE: uuid::Uuid = uuid::Uuid::from_bytes([0x17, 0x0d, 0x51, 0x62, 0x90, 0x9a, 0x54, 0x33, 0x89, 0x87, 0x61, 0x6d, 0xb1, 0x19, 0x63, 0x6d]);
    format!("unified:{}", uuid::Uuid::new_v5(&NAMESPACE, path.as_bytes()))
}

pub fn unified_subject_sort_order(path: &str) -> i64 {
    current().routes.get(path).map_or(0, UnifiedConceptRoute::sort_order)
}
pub fn unified_taxonomy_routes() -> Vec<UnifiedConceptRoute> {
    current().routes.values().cloned().collect()
}
pub fn unified_ddc_subject_paths(code: &str) -> Vec<String> {
    unified_subject_paths(DDC_SYSTEM_ID, code)
}
pub fn unified_lcc_subject_paths(code: &str) -> Vec<String> {
    unified_subject_paths(LCC_SYSTEM_ID, code)
}

pub(super) fn matching_concepts(runtime: &UnifiedTaxonomyRuntime, system_id: &str, code: &str) -> BTreeSet<String> {
    let Some(index) = runtime.selectors.get(system_id) else { return BTreeSet::new() };
    let canonical = match system_id {
        DDC_SYSTEM_ID => canonical_ddc_notation(code).map(|code| code[..3].to_owned()),
        _ => Some(code.trim().to_ascii_uppercase()),
    };
    let Some(canonical) = canonical else { return BTreeSet::new() };
    if let Some(concepts) = index.exact.get(&canonical) {
        return concepts.clone();
    }
    if system_id == DDC_SYSTEM_ID {
        let Some(number) = canonical.parse::<u16>().ok() else { return BTreeSet::new() };
        let width = index.numeric_ranges.iter().filter(|range| range.start <= number && number <= range.end).map(|range| range.end - range.start).min();
        if let Some(width) = width {
            return index.numeric_ranges.iter().filter(|range| range.start <= number && number <= range.end && range.end - range.start == width).map(|range| range.concept_id.clone()).collect();
        }
    }
    if system_id == LCC_SYSTEM_ID {
        if let Some(call) = parse_lcc_call(code) {
            let matching = index
                .lcc_ranges
                .iter()
                .filter(|range| compare_lcc_key(&range.start_letters, range.start_number, &call.letters, call.number).is_le() && compare_lcc_key(&call.letters, call.number, &range.end_letters, range.end_number).is_le())
                .collect::<Vec<_>>();
            let best = matching
                .iter()
                .map(|range| {
                    let exact = range.start_letters == call.letters && range.end_letters == call.letters;
                    (range.priority, exact, if exact { range.end_number - range.start_number } else { f64::INFINITY })
                })
                .max_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)).then_with(|| right.2.total_cmp(&left.2)));
            if let Some((priority, exact, span)) = best {
                return matching
                    .into_iter()
                    .filter(|range| {
                        let range_exact = range.start_letters == call.letters && range.end_letters == call.letters;
                        range.priority == priority && range_exact == exact && (if range_exact { range.end_number - range.start_number } else { f64::INFINITY }).total_cmp(&span).is_eq()
                    })
                    .map(|range| range.concept_id.clone())
                    .collect();
            }
        }
    }
    let longest = index.prefixes.iter().filter(|(prefix, _)| canonical.starts_with(prefix)).map(|(prefix, _)| prefix.len()).max();
    longest.map(|length| index.prefixes.iter().filter(|(prefix, _)| prefix.len() == length && canonical.starts_with(prefix)).map(|(_, id)| id.clone()).collect()).unwrap_or_default()
}

pub(super) fn normalize_source_label(value: &str) -> String {
    compact_whitespace(value).to_lowercase()
}

fn default_runtime() -> Arc<UnifiedTaxonomyRuntime> {
    Arc::new(build_runtime(default_concepts()).expect("embedded unified taxonomy must be valid"))
}

#[cfg(not(target_arch = "wasm32"))]
fn default_concepts() -> Vec<UnifiedConceptDefinition> {
    crate::taxonomy_sqlite::read_embedded_unified_taxonomy().expect("embedded unified taxonomy SQLite must be readable")
}

#[cfg(target_arch = "wasm32")]
fn default_concepts() -> Vec<UnifiedConceptDefinition> {
    let (rows, _) = bincode::decode_from_slice::<Vec<SnapshotRow>, _>(include_bytes!(concat!(env!("OUT_DIR"), "/unified-taxonomy-v2.bin")), bincode::config::standard()).expect("embedded unified taxonomy snapshot must be readable");
    rows.into_iter()
        .map(|row| {
            let split = |value: &str| value.split('|').map(str::trim).filter(|value| !value.is_empty()).map(ToOwned::to_owned).collect::<Vec<_>>();
            let mut source_selectors = BTreeMap::new();
            for (system, selectors) in [(super::BISAC_SYSTEM_ID, row.bisac_codes), (DDC_SYSTEM_ID, row.ddc_codes), (LCC_SYSTEM_ID, row.lcc_codes)] {
                let selectors = split(&selectors);
                if !selectors.is_empty() {
                    source_selectors.insert(system.to_owned(), selectors);
                }
            }
            let parent_ids = split(&row.parent_ids);
            let parent_sort_orders = if row.parent_sort_orders.trim().is_empty() { vec![0; parent_ids.len()] } else { split(&row.parent_sort_orders).into_iter().map(|value| value.parse().expect("numeric sort order")).collect() };
            UnifiedConceptDefinition { concept_id: row.concept_id, preferred_label: row.preferred_label, parent_ids, parent_sort_orders, source_selectors, source_labels: split(&row.source_labels) }
        })
        .collect()
}

fn build_runtime(concepts: Vec<UnifiedConceptDefinition>) -> Result<UnifiedTaxonomyRuntime, String> {
    validate(&concepts)?;
    let mut fingerprint = String::new();
    for concept in &concepts {
        use std::fmt::Write as _;
        write!(&mut fingerprint, "{}\u{1f}{}\u{1f}{}\u{1f}{}", concept.concept_id, concept.preferred_label, concept.parent_ids.join("|"), concept.parent_sort_orders.iter().map(i64::to_string).collect::<Vec<_>>().join("|")).unwrap();
        for (system, selectors) in &concept.source_selectors {
            write!(&mut fingerprint, "\u{1f}{system}={}", selectors.join("|")).unwrap();
        }
        writeln!(&mut fingerprint, "\u{1f}{}", concept.source_labels.join("|")).unwrap();
    }
    let concepts = Arc::new(concepts);
    let routes = Arc::new(build_routes(&concepts)?);
    let mut paths_by_concept = BTreeMap::<String, Vec<String>>::new();
    for route in routes.values() {
        paths_by_concept.entry(route.concept_id.clone()).or_default().push(route.path.clone());
    }
    let selectors = Arc::new(build_selectors(&concepts)?);
    let mut source_labels = BTreeMap::<String, BTreeSet<String>>::new();
    for concept in concepts.iter() {
        for label in &concept.source_labels {
            source_labels.entry(normalize_source_label(label)).or_default().insert(concept.concept_id.clone());
        }
    }
    const NAMESPACE: uuid::Uuid = uuid::Uuid::from_bytes([0xa7, 0xed, 0x9f, 0x7d, 0xa1, 0x2a, 0x59, 0xf5, 0xb5, 0x3d, 0x67, 0x4d, 0xe0, 0x80, 0x7c, 0x66]);
    let release_id = format!("{UNIFIED_VERSION}:{}", uuid::Uuid::new_v5(&NAMESPACE, fingerprint.as_bytes()));
    Ok(UnifiedTaxonomyRuntime { concepts, routes, paths_by_concept: Arc::new(paths_by_concept), selectors, source_labels: Arc::new(source_labels), release_id })
}

fn validate(concepts: &[UnifiedConceptDefinition]) -> Result<(), String> {
    if concepts.iter().any(|concept| concept.concept_id.trim().is_empty() || concept.preferred_label.trim().is_empty()) {
        return Err("every unified concept requires an ID and preferred label".to_owned());
    }
    let ids = concepts.iter().map(|concept| concept.concept_id.as_str()).collect::<BTreeSet<_>>();
    if ids.len() != concepts.len() {
        return Err("unified taxonomy concept IDs must be unique".to_owned());
    }
    let labels_by_id = concepts.iter().map(|concept| (concept.concept_id.as_str(), normalized_concept_label(&concept.preferred_label))).collect::<BTreeMap<_, _>>();
    let mut sibling_labels = BTreeMap::<(&str, String), &str>::new();
    let mut selector_labels = BTreeMap::<(&str, &str, String), &str>::new();
    for concept in concepts {
        if concept.parent_ids.len() != concept.parent_sort_orders.len() {
            return Err(format!("unified concept {} must provide one sort order per parent", concept.concept_id));
        }
        for parent in &concept.parent_ids {
            if !ids.contains(parent.as_str()) {
                return Err(format!("unified concept {} references missing parent {parent}", concept.concept_id));
            }
            let label = labels_by_id.get(concept.concept_id.as_str()).expect("concept label exists");
            let parent_label = labels_by_id.get(parent.as_str()).expect("parent label exists");
            if label == parent_label {
                return Err(format!("unified concept {} duplicates the preferred label of direct parent {parent}", concept.concept_id));
            }
            if let Some(existing) = sibling_labels.insert((parent.as_str(), label.clone()), concept.concept_id.as_str()) {
                if existing != concept.concept_id {
                    return Err(format!("unified concepts {existing} and {} duplicate a preferred label beneath parent {parent}", concept.concept_id));
                }
            }
        }
        let label = labels_by_id.get(concept.concept_id.as_str()).expect("concept label exists");
        for (system, selectors) in &concept.source_selectors {
            for selector in selectors {
                if let Some(existing) = selector_labels.insert((system.as_str(), selector.as_str(), label.clone()), concept.concept_id.as_str()) {
                    if existing != concept.concept_id {
                        return Err(format!("unified concepts {existing} and {} duplicate source selector {system}:{selector} with the same preferred label", concept.concept_id));
                    }
                }
            }
        }
    }
    let mut reachable = concepts.iter().filter(|concept| concept.parent_ids.is_empty()).map(|concept| concept.concept_id.as_str()).collect::<BTreeSet<_>>();
    loop {
        let before = reachable.len();
        for concept in concepts {
            if concept.parent_ids.iter().all(|parent| reachable.contains(parent.as_str())) {
                reachable.insert(&concept.concept_id);
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
    let mut child_counts = HashMap::<String, usize>::new();
    for concept in concepts {
        for parent_id in &concept.parent_ids {
            *child_counts.entry(parent_id.clone()).or_insert(0) += 1;
        }
    }

    let mut routes = BTreeMap::new();
    let mut known = BTreeMap::<String, Vec<String>>::new();
    let mut routed_concepts = BTreeSet::new();

    let is_root_with_no_children = |concept: &UnifiedConceptDefinition| concept.parent_ids.is_empty() && child_counts.get(concept.concept_id()).copied().unwrap_or_default() == 0;

    for concept in concepts.iter().filter(|concept| concept.parent_ids.is_empty()) {
        if is_root_with_no_children(concept) {
            continue;
        }
        let path = concept.preferred_label.clone();
        routes.insert(path.clone(), UnifiedConceptRoute { concept_id: concept.concept_id.clone(), path: path.clone(), parent_path: String::new(), label: concept.preferred_label.clone(), sort_order: 0 });
        known.entry(concept.concept_id.clone()).or_default().push(path);
        routed_concepts.insert(concept.concept_id.clone());
    }

    loop {
        let mut added = false;
        for concept in concepts.iter().filter(|concept| !concept.parent_ids.is_empty()) {
            if is_root_with_no_children(concept) {
                continue;
            }
            for (index, parent_id) in concept.parent_ids.iter().enumerate() {
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
                    routes.insert(path.clone(), UnifiedConceptRoute { concept_id: concept.concept_id.clone(), path: path.clone(), parent_path, label: concept.preferred_label.clone(), sort_order: concept.parent_sort_orders[index] });
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
                } else if system_id == DDC_SYSTEM_ID && selector.contains('-') {
                    let (start, end) = selector.split_once('-').unwrap();
                    let start = start.parse::<u16>().map_err(|_| format!("invalid DDC selector {selector}"))?;
                    let end = end.parse::<u16>().map_err(|_| format!("invalid DDC selector {selector}"))?;
                    if start > end {
                        return Err(format!("reversed DDC selector {selector}"));
                    }
                    index.numeric_ranges.push(NumericSelector { start, end, concept_id: concept.concept_id.clone() });
                } else if system_id == LCC_SYSTEM_ID && selector.contains("..") {
                    let (bounds, priority) = selector.rsplit_once('@').ok_or_else(|| format!("LCC range without priority: {selector}"))?;
                    let (start, end) = bounds.split_once("..").ok_or_else(|| format!("invalid LCC selector {selector}"))?;
                    let start = parse_lcc_call(start).ok_or_else(|| format!("invalid LCC range start {start}"))?;
                    let end = parse_lcc_call(end).ok_or_else(|| format!("invalid LCC range end {end}"))?;
                    index.lcc_ranges.push(LccSelector {
                        start_letters: start.letters,
                        start_number: start.number,
                        end_letters: end.letters,
                        end_number: end.number,
                        priority: priority.parse().map_err(|_| format!("invalid LCC priority {priority}"))?,
                        concept_id: concept.concept_id.clone(),
                    });
                } else {
                    index.exact.entry(selector).or_default().insert(concept.concept_id.clone());
                }
            }
        }
    }
    Ok(indexes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn concept(id: &str, label: &str, parents: &[&str], lcc: &[&str]) -> UnifiedConceptDefinition {
        let source_selectors = if lcc.is_empty() { BTreeMap::new() } else { BTreeMap::from([(LCC_SYSTEM_ID.to_owned(), lcc.iter().map(|value| (*value).to_owned()).collect())]) };
        UnifiedConceptDefinition::new(id.to_owned(), label.to_owned(), parents.iter().map(|value| (*value).to_owned()).collect(), source_selectors, Vec::new())
    }

    #[test]
    fn validation_rejects_parallel_taxonomy_structures() {
        let redundant_wrapper = vec![concept("root", "History", &[], &[]), concept("child", "History.", &["root"], &[])];
        assert!(validate(&redundant_wrapper).unwrap_err().contains("direct parent"));

        let duplicate_siblings = vec![concept("root", "Humanities", &[], &[]), concept("left", "By period", &["root"], &[]), concept("right", "By Period.", &["root"], &[])];
        assert!(validate(&duplicate_siblings).unwrap_err().contains("beneath parent"));

        let duplicate_source_record = vec![
            concept("root", "Humanities", &[], &[]),
            concept("left", "Ethnography", &["root"], &["GN301..GN674@4"]),
            concept("other-root", "Social Sciences", &[], &[]),
            concept("right", "Ethnography.", &["other-root"], &["GN301..GN674@4"]),
        ];
        assert!(validate(&duplicate_source_record).unwrap_err().contains("duplicate source selector"));
    }

    #[test]
    fn validation_preserves_meaningful_programming_language_punctuation() {
        let programming_languages =
            vec![concept("root", "Programming Languages", &[], &[]), concept("c", "C", &["root"], &[]), concept("c-sharp", "C#", &["root"], &[]), concept("c-plus-plus", "C++", &["root"], &[]), concept("dot-net", ".NET", &["root"], &[])];

        assert!(validate(&programming_languages).is_ok());
    }

    #[test]
    fn broad_ranges_do_not_count_as_exact_local_code_coverage() {
        let taxonomy =
            UnifiedTaxonomy::from_concepts(vec![concept("root", "Social Sciences", &[], &[]), concept("business", "Business", &["root"], &["HF1..HF6182@1"]), concept("accounting", "Accounting", &["business"], &["HF5601"])]).unwrap();

        let broad_only = UnifiedTaxonomy::from_concepts(vec![concept("root", "Social Sciences", &[], &[]), concept("business", "Business", &["root"], &["HF1..HF6182@1"])]).unwrap();
        assert!(!broad_only.matching_concept_ids(LCC_SYSTEM_ID, "HF5601").is_empty());
        assert!(!broad_only.has_exact_source_code(LCC_SYSTEM_ID, "HF5601"));
        assert!(taxonomy.has_exact_source_code(LCC_SYSTEM_ID, "hf5601"));
    }

    #[test]
    fn similarity_keys_follow_unified_taxonomy_branches_across_source_systems() {
        let greek_lcc = unified_subject_similarity_keys(LCC_SYSTEM_ID, "PA3404");
        let greek_ddc = unified_subject_similarity_keys(DDC_SYSTEM_ID, "888");
        let french_lcc = unified_subject_similarity_keys(LCC_SYSTEM_ID, "PQ3434");

        assert!(!greek_lcc.is_empty());
        assert!(greek_lcc.iter().any(|key| greek_ddc.contains(key)));
        assert!(greek_lcc.iter().all(|key| !french_lcc.contains(key)));
    }

    #[test]
    fn detailed_lcc_business_ranges_follow_the_curated_hierarchy() {
        let expected = [
            ("HF5601", "Business / Accounting"),
            ("HF5469.3", "Business / Retail Trade / Convenience Stores"),
            ("HF5548.75", "Business / Business Organization & Administration / Personnel Management / Industrial Psychology"),
            ("HF5801", "Business / Marketing / Advertising"),
            ("HF5351", "Business / Reference & Forms / General Works & Textbooks"),
            ("HF6201", "Business / Special Lines of Business"),
        ];

        for (call_number, suffix) in expected {
            let paths = unified_lcc_subject_paths(call_number);
            assert!(paths.iter().any(|path| path.ends_with(suffix)), "{call_number} did not resolve beneath {suffix}: {paths:?}");
        }
    }

    #[test]
    fn subject_bibliography_topics_are_contextual_and_grouped() {
        let expected = [
            ("Z6658", "Subject bibliography / Medicine, Health & Disability / Medicine"),
            ("Z6201", "Subject bibliography / Humanities & Culture / History"),
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
    fn protestant_denomination_detail_reuses_concepts_and_skips_filing_guides() {
        let expected = [
            ("BX6201", "Other Protestant denominations / Historic Denominational Traditions / Baptist"),
            ("BX7601", "Other Protestant denominations / Brethren, Friends & Peace Churches / Quaker"),
            ("BX8762", "Other Protestant denominations / Holiness, Pentecostal & Apostolic Churches / Pentecostal & Charismatic"),
            ("BX9884", "Other Protestant denominations / Reformed & Congregational Traditions / United Church of Christ"),
        ];

        for (call_number, suffix) in expected {
            let paths = unified_lcc_subject_paths(call_number);
            assert!(paths.iter().any(|path| path.ends_with(suffix)), "{call_number} did not resolve beneath {suffix}: {paths:?}");
        }

        let filing_guide = unified_lcc_subject_paths("BX6510.A");
        assert!(filing_guide.iter().all(|path| !path.ends_with("Baptists - Catholic Apostolic Church")));
    }

    #[test]
    fn decorative_arts_lcc_detail_routes_to_existing_concepts() {
        let expected = [
            ("NK3700", "Other arts and art industries / Ceramics"),
            ("NK5100", "Other arts and art industries / Glass"),
            ("NK6400", "Other arts and art industries / Art metalwork"),
            ("NK8555", "Other arts and art industries / Papercrafts"),
            ("NK8800", "Other arts and art industries / Textile arts"),
            ("NK9600", "Other arts and art industries / Woodwork"),
        ];

        for (call_number, suffix) in expected {
            let paths = unified_lcc_subject_paths(call_number);
            assert!(paths.iter().any(|path| path.ends_with(suffix)), "{call_number} did not resolve beneath {suffix}: {paths:?}");
        }
    }

    #[test]
    fn electricity_and_magnetism_expand_as_physics_not_engineering() {
        let expected = [
            ("QC570", "Electricity & Electromagnetism / Electricity / Electrostatics & Electric Current / Electrostatics & Frictional Electricity"),
            ("QC679", "Electricity / Electrodynamics, Waves & Quantum Effects / Quantum Electrodynamics"),
            ("QC717.6", "Electricity / Discharge, Plasma & Particle Physics / Plasma Physics & Ionized Gases"),
            ("QC721", "Electricity / Discharge, Plasma & Particle Physics / Nuclear & Particle Physics"),
            ("QC759.6", "Magnetism / Magnetic Phenomena / Electromagnetism"),
            ("QC761.5", "Magnetism / Magnetic Phenomena / Ferromagnetism & Antiferromagnetism"),
        ];

        for (call_number, suffix) in expected {
            let paths = unified_lcc_subject_paths(call_number);
            assert!(paths.iter().any(|path| path.ends_with(suffix)), "{call_number} did not resolve beneath {suffix}: {paths:?}");
            assert!(paths.iter().all(|path| !path.contains("Electrical, Electronics & Communications")), "{call_number} incorrectly resolved into electrical engineering: {paths:?}");
        }

        let quantum_paths = unified_lcc_subject_paths("QC679");
        assert!(quantum_paths.iter().any(|path| path.ends_with("Quantum Theory / Quantum Electrodynamics")));
    }

    #[test]
    fn veterinary_medicine_is_unified_and_contextually_expanded() {
        let bisac_paths = unified_subject_paths("bisac", "MED089000");
        assert!(bisac_paths.iter().any(|path| path.ends_with("Clinical Specialties / Veterinary Medicine")));
        assert!(bisac_paths.iter().any(|path| path.ends_with("Veterinary Health & Surgery / Veterinary Medicine")));

        let expected = [
            ("SF757.2", "Veterinary Medicine / Genetics, Behavior & Biomedical Foundations / Veterinary Immunology"),
            ("SF780.4", "Veterinary Medicine / Microbiology & Parasitology / Veterinary Virology"),
            ("SF914.5", "Veterinary Medicine / Pharmacology & Therapies / Veterinary Acupuncture"),
            ("SF911", "Veterinary Medicine / Surgery"),
        ];
        for (call_number, suffix) in expected {
            let paths = unified_lcc_subject_paths(call_number);
            assert!(paths.iter().any(|path| path.ends_with(suffix)), "{call_number} did not resolve beneath {suffix}: {paths:?}");
        }

        let concepts = unified_concept_definitions();
        assert!(concepts.iter().all(|concept| concept.concept_id() != "unified:3d0a37fc-a3aa-58e5-bba9-677c9ec8f8e6"));
        assert!(concepts.iter().any(|concept| concept.concept_id() == "unified:e3a8f859-86c0-5d2c-920c-609508aeccc8" && concept.preferred_label() == "Veterinary Law & Regulation"));
    }

    #[test]
    fn library_science_has_distinct_discipline_and_collection_routes() {
        let expected = [
            ("Z666.5", "Library & Information Science / Organization & Information Systems / Information Organization"),
            ("Z674.2", "Library & Information Science / Organization & Information Systems / Information services. Information centers"),
            ("Z678", "Library & Information Science / Administration, Facilities & Governance / Library & Archive Operations"),
            ("Z679", "Library & Information Science / Administration, Facilities & Governance / Library Buildings & Physical Plant"),
            ("Z711", "Library & Information Science / Collections, Access & Public Services / Public & Reference Services"),
            ("Z718.1", "Library & Information Science / Education & User Groups / Children's Libraries & Departments"),
        ];

        for (call_number, suffix) in expected {
            let paths = unified_lcc_subject_paths(call_number);
            assert!(paths.iter().any(|path| path.ends_with(suffix)), "{call_number} did not resolve beneath {suffix}: {paths:?}");
            assert!(paths.iter().all(|path| !path.contains("Libraries & Collections / Library & Information Science")), "redundant library route remains for {call_number}: {paths:?}");
        }
    }
}
