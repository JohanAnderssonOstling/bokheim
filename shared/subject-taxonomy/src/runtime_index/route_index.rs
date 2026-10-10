//! Compact matcher navigation. Display paths are constructed only at API boundaries.
use super::{UnifiedConceptRoute, ROOT_SUBJECT_PATH};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Route {
    concept: usize,
    parent: Option<usize>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Concept {
    id: i64,
    label: String,
    sort_order: i64,
    routes: Vec<usize>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct RouteIndex {
    routes: Vec<Route>,
    concepts: Vec<Concept>,
    // A sorted vector avoids a string key and tree allocation per route. Hashes
    // only select candidates; the complete ancestry must match, even on collision.
    // The explicit hash is identical in the host build script and native/Wasm runtime.
    by_path_hash: Vec<(u64, usize)>,
}

fn path_hash(path: &str) -> u64 {
    // FNV-1a selects a bucket; it is never used as identity or trusted for equality.
    path.bytes().fold(0xcbf29ce484222325, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3))
}

impl RouteIndex {
    pub fn compile(source: BTreeMap<String, UnifiedConceptRoute>) -> Self {
        let ids = source.keys().enumerate().map(|(id, path)| (path.as_str(), id)).collect::<BTreeMap<_, _>>();
        let mut routes = Vec::with_capacity(source.len());
        let mut concepts =
            source.values().map(|route| (route.concept_id, Concept { id: route.concept_id, label: route.label.clone(), sort_order: route.sort_order, routes: Vec::new() })).collect::<BTreeMap<_, _>>().into_values().collect::<Vec<_>>();
        let mut by_path_hash = Vec::with_capacity(source.len());
        for (id, (path, route)) in source.iter().enumerate() {
            let parent = (path != &route.label).then(|| ids[route.parent_path.as_str()]);
            let concept = concepts.binary_search_by_key(&route.concept_id, |concept| concept.id).unwrap();
            routes.push(Route { concept, parent });
            concepts[concept].routes.push(id);
            by_path_hash.push((path_hash(path), id));
        }
        by_path_hash.sort_unstable();
        Self { routes, concepts, by_path_hash }
    }

    pub fn paths_for_concept(&self, concept: i64) -> impl Iterator<Item = String> + '_ {
        let routes = self.concepts.binary_search_by_key(&concept, |node| node.id).map(|index| self.concepts[index].routes.as_slice()).unwrap_or_default();
        routes.iter().map(|id| self.path(*id))
    }

    pub fn parent(&self, id: usize) -> Option<usize> {
        self.routes[id].parent
    }
    pub fn label(&self, id: usize) -> &str {
        &self.concepts[self.routes[id].concept].label
    }
    pub fn concept_id(&self, id: usize) -> i64 {
        self.concepts[self.routes[id].concept].id
    }
    pub fn sort_order(&self, id: usize) -> i64 {
        self.concepts[self.routes[id].concept].sort_order
    }

    pub fn path(&self, id: usize) -> String {
        let mut labels = Vec::new();
        let mut current = Some(id);
        while let Some(route) = current {
            labels.push(self.label(route));
            current = self.parent(route);
        }
        labels.reverse();
        labels.join(" / ")
    }

    fn matches_path(&self, mut id: usize, mut path: &str) -> bool {
        loop {
            let Some(prefix) = path.strip_suffix(self.label(id)) else { return false };
            match self.parent(id) {
                Some(parent) => {
                    let Some(rest) = prefix.strip_suffix(" / ") else { return false };
                    id = parent;
                    path = rest;
                }
                None => return prefix.is_empty(),
            }
        }
    }

    pub fn find(&self, path: &str) -> Option<usize> {
        let hash = path_hash(path);
        let start = self.by_path_hash.partition_point(|(candidate, _)| *candidate < hash);
        self.by_path_hash[start..].iter().take_while(|(candidate, _)| *candidate == hash).find_map(|(_, id)| self.matches_path(*id, path).then_some(*id))
    }

    pub fn values(&self) -> impl Iterator<Item = UnifiedConceptRoute> + '_ {
        (0..self.routes.len()).map(|id| UnifiedConceptRoute {
            concept_id: self.concept_id(id),
            path: self.path(id),
            parent_path: self.parent(id).map(|parent| self.path(parent)).unwrap_or_else(|| ROOT_SUBJECT_PATH.to_owned()),
            label: self.label(id).to_owned(),
            sort_order: self.sort_order(id),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> BTreeMap<String, UnifiedConceptRoute> {
        [
            (1, "History", "Subject", "History"),
            (2, "History / Myanmar / Burma", "History", "Myanmar / Burma"),
            (3, "Places", "Subject", "Places"),
            (2, "Places / Myanmar / Burma", "Places", "Myanmar / Burma"),
            (4, "Subject", "Subject", "Subject"),
            (5, "Subject / Leaf", "Subject", "Leaf"),
        ]
        .into_iter()
        .map(|(concept_id, path, parent_path, label)| (path.to_owned(), UnifiedConceptRoute { concept_id, path: path.to_owned(), parent_path: parent_path.to_owned(), label: label.to_owned(), sort_order: concept_id * 2 }))
        .collect()
    }


    #[test]
    fn hash_collisions_never_resolve_to_a_different_route() {
        let mut index = RouteIndex::compile(source());
        let path = "Places / Myanmar / Burma";
        let expected = index.find(path).unwrap();
        let hash = path_hash(path);
        index.by_path_hash = (0..index.routes.len()).map(|id| (hash, id)).collect();
        assert_eq!(index.find(path), Some(expected));
        let missing = "Places / Missing";
        index.by_path_hash = (0..index.routes.len()).map(|id| (path_hash(missing), id)).collect();
        assert_eq!(index.find(missing), None);
    }
}
