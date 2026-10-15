//! A sibling fallback needs agreement between directly classified editions.
use super::subject_identity::{self, Candidate};
use metadata_contract::{classification_consensus_notation, ClassificationEvidence, ClassificationScheme, ClassificationSource, EditionIdentityQuery, SiblingSubjectConsensus, SiblingSubjectSupport};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, OnceLock};
use subject_projection::{most_specific_concept_ids, unified_concept_id, unified_subject_paths, unified_taxonomy_revision_id, unified_taxonomy_routes, LCC_SYSTEM_ID};

const MIN_PERCENT: usize = 20;
const SUPPORT_EPSILON: f64 = 1e-12;

#[derive(Default)]
struct EditionObservation {
    subjects: BTreeMap<i64, BTreeSet<ClassificationEvidence>>,
    unmapped_codes: BTreeSet<String>,
}

#[derive(Default)]
struct SubjectSupport {
    weight: f64,
    voters: BTreeMap<String, BTreeSet<ClassificationEvidence>>,
}

struct TaxonomyGraph {
    revision: String,
    parents: BTreeMap<i64, BTreeSet<i64>>,
    paths: BTreeMap<i64, BTreeSet<String>>,
}

impl TaxonomyGraph {
    fn ancestors(&self, concept: i64, ids: &mut BTreeSet<i64>) {
        if ids.insert(concept) {
            if let Some(parents) = self.parents.get(&concept) {
                for parent in parents {
                    self.ancestors(*parent, ids);
                }
            }
        }
    }
}

fn taxonomy_graph() -> Arc<TaxonomyGraph> {
    static GRAPH: OnceLock<Mutex<Option<Arc<TaxonomyGraph>>>> = OnceLock::new();
    let revision = unified_taxonomy_revision_id();
    let mut cached = GRAPH.get_or_init(|| Mutex::new(None)).lock().expect("consensus taxonomy cache lock");
    if let Some(graph) = cached.as_ref().filter(|graph| graph.revision == revision) {
        return graph.clone();
    }
    let routes = unified_taxonomy_routes();
    let identities = routes.iter().map(|route| (route.path.as_str(), route.concept_id)).collect::<BTreeMap<_, _>>();
    let mut graph = TaxonomyGraph { revision, parents: BTreeMap::new(), paths: BTreeMap::new() };
    for route in &routes {
        graph.paths.entry(route.concept_id).or_default().insert(route.path.clone());
        if let Some(parent) = identities.get(route.parent_path.as_str()) {
            graph.parents.entry(route.concept_id).or_default().insert(*parent);
        }
    }
    let graph = Arc::new(graph);
    *cached = Some(graph.clone());
    graph
}

pub(super) fn consensus(query: &EditionIdentityQuery, candidates: &[Candidate]) -> SiblingSubjectConsensus {
    let mut result = SiblingSubjectConsensus::default();
    if query.authors.is_empty() {
        return result;
    }
    let works = subject_identity::verified_works(query, candidates);
    let compatible = candidates.iter().map(|(_, _, _, candidate)| candidate)
        .filter(|candidate| subject_identity::title_agrees(query, candidate, &works)
            && !candidate.open_library_edition_id.is_empty()
            && subject_identity::author_count(query, candidate, &works) > 0)
        .collect::<Vec<_>>();
    // Unclassified editions can expose an author conflict, but cannot vote.
    if subject_identity::conflicting_authors(query, &compatible) {
        tracing::debug!("Sibling consensus abstained because author credits conflict");
        return result;
    }
    let eligible = compatible.into_iter().filter(|candidate| candidate.classifications.iter().any(|code|
        code.scheme == ClassificationScheme::LibraryOfCongress && code.source == ClassificationSource::ExactEdition
            && classification_consensus_notation(code.scheme, &code.notation).is_some())).collect::<Vec<_>>();
    // Extra credits on an unclassified edition must not disqualify directly
    // classified editions that credit the author but omit a translator/editor.
    let best = eligible.iter().map(|candidate| subject_identity::author_count(query, candidate, &works)).max().unwrap_or(0);
    let eligible = eligible.into_iter().filter(|candidate| subject_identity::author_count(query, candidate, &works) == best).collect::<Vec<_>>();
    let graph = taxonomy_graph();
    let mut editions = BTreeMap::<String, EditionObservation>::new();
    for candidate in eligible {
        // Duplicate ISBN records are one observation; absent ISBNs use OL IDs.
        let identity = if candidate.canonical_isbn13.is_empty() { candidate.open_library_edition_id.clone() } else { candidate.canonical_isbn13.clone() };
        for classification in &candidate.classifications {
            if classification.scheme != ClassificationScheme::LibraryOfCongress || classification.source != ClassificationSource::ExactEdition || classification_consensus_notation(classification.scheme, &classification.notation).is_none() {
                continue;
            }
            // Unmapped numeric codes still count in the denominator. Inherited
            // work unions and unclassified records cannot create observations.
            let observation = editions.entry(identity.clone()).or_default();
            let mut concepts = BTreeSet::new();
            // Map the complete call number before climbing the taxonomy, so
            // author cutters and other meaningful suffixes retain their scope.
            for path in unified_subject_paths(LCC_SYSTEM_ID, &classification.notation) {
                if let Some(concept) = unified_concept_id(&path) {
                    concepts.insert(concept);
                }
            }
            let evidence = ClassificationEvidence {
                method: format!("direct_edition_lcc:{}", classification.notation),
                isbn13: candidate.canonical_isbn13.clone(),
                open_library_work_id: candidate.open_library_work_id.clone().unwrap_or_default(),
                open_library_edition_id: candidate.open_library_edition_id.clone(),
            };
            if concepts.is_empty() {
                observation.unmapped_codes.insert(classification.notation.clone());
            }
            for concept in concepts {
                observation.subjects.entry(concept).or_default().insert(evidence.clone());
            }
        }
    }
    result.classified_editions = editions.len();
    let mut support = BTreeMap::<i64, SubjectSupport>::new();
    for (identity, observation) in &editions {
        // Equivalent codes and redundant parents cannot multiply a vote.
        // An edition with eight distinct leaves contributes 1/8 to each.
        // Unmapped codes consume their share without supporting a concept.
        let leaves = most_specific_concept_ids(observation.subjects.keys().copied());
        let assignments = leaves.len() + observation.unmapped_codes.len();
        if assignments == 0 {
            continue;
        }
        let weight = 1.0 / assignments as f64;
        for leaf in leaves {
            let evidence = &observation.subjects[&leaf];
            let mut ancestors = BTreeSet::new();
            graph.ancestors(leaf, &mut ancestors);
            for concept in ancestors {
                let supported = support.entry(concept).or_default();
                supported.weight += weight;
                supported.voters.entry(identity.clone()).or_default().extend(evidence.iter().cloned());
            }
        }
    }
    let eligible = support
        .iter()
        .filter(|(concept, _)| graph.paths.get(concept).is_some_and(|paths| paths.iter().all(|path| path.contains(" / "))))
        .collect::<Vec<_>>();
    let mut qualifying = eligible
        .iter()
        .filter(|(_, supported)| supported.weight / editions.len() as f64 + SUPPORT_EPSILON >= MIN_PERCENT as f64 / 100.0)
        .map(|(concept, _)| **concept)
        .collect::<BTreeSet<_>>();
    if qualifying.is_empty() {
        let maximum = eligible.iter().map(|(_, supported)| supported.weight).fold(0.0_f64, f64::max);
        if maximum > 0.0 {
            qualifying.extend(eligible.iter().filter(|(_, supported)| (supported.weight - maximum).abs() / editions.len() as f64 <= SUPPORT_EPSILON).map(|(concept, _)| **concept));
            result.used_highest_support_fallback = true;
        }
    }
    for concept in most_specific_concept_ids(qualifying) {
        let supported = support.remove(&concept).expect("qualifying concept votes");
        let Some(path) = graph.paths.get(&concept).and_then(|paths| paths.iter().next()).cloned() else { continue };
        result.subjects.push(SiblingSubjectSupport {
            path,
            supporting_editions: supported.voters.len(),
            support_fraction: supported.weight / editions.len() as f64,
            evidence: supported.voters.into_values().flatten().collect(),
        });
    }
    result
}
