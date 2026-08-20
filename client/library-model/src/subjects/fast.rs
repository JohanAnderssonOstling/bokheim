use super::runtime::{normalize_source_label, UnifiedTaxonomyRuntime};
use super::UnifiedConceptRoute;
use crate::PublicationSubject;
use std::collections::BTreeSet;

pub(super) fn matching_hierarchy_paths(runtime: &UnifiedTaxonomyRuntime, subject: &PublicationSubject) -> BTreeSet<String> {
    let source = subject.source().trim().to_ascii_lowercase();
    let authority = subject.authority().map(str::trim).unwrap_or_default().to_ascii_lowercase();
    if authority != "fast" && !source.ends_with("/#fast") {
        return BTreeSet::new();
    }
    let components = subject.name().split("--").map(normalize_source_label).filter(|component| !component.is_empty()).collect::<Vec<_>>();
    if components.len() < 2 {
        return BTreeSet::new();
    }
    runtime.routes.keys().filter(|path| components_fit_route_suffix(runtime, &components, path)).cloned().collect()
}

fn components_fit_route_suffix(runtime: &UnifiedTaxonomyRuntime, components: &[String], path: &str) -> bool {
    let labels = path.split(" / ").collect::<Vec<_>>();
    if labels.len() < components.len() {
        return false;
    }
    let prefixes = (labels.len() - components.len()..labels.len()).map(|index| labels[..=index].join(" / ")).collect::<Vec<_>>();
    let Some(routes) = prefixes.iter().map(|prefix| runtime.routes.get(prefix)).collect::<Option<Vec<_>>>() else { return false };
    components_match_routes(runtime, components, &routes, 0, &mut vec![false; routes.len()])
}

fn components_match_routes(runtime: &UnifiedTaxonomyRuntime, components: &[String], routes: &[&UnifiedConceptRoute], component_index: usize, used_routes: &mut [bool]) -> bool {
    if component_index == components.len() {
        return true;
    }
    for (route_index, route) in routes.iter().enumerate() {
        if used_routes[route_index] {
            continue;
        }
        let component = &components[component_index];
        let matches = normalize_source_label(route.label()) == *component || runtime.source_labels.get(component).is_some_and(|ids| ids.contains(route.concept_id()));
        if !matches {
            continue;
        }
        used_routes[route_index] = true;
        if components_match_routes(runtime, components, routes, component_index + 1, used_routes) {
            return true;
        }
        used_routes[route_index] = false;
    }
    false
}
