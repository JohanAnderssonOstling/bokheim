use crate::DirectoryLifecycleState;
use crate::VersionKey;
use std::collections::{HashMap, HashSet};
use sync_common::{DirId, FileName, ROOT_DIR_ID};

#[derive(Debug)]
pub struct DirectoryIntent {
    pub id: DirId,
    pub parent_id: DirId,
    pub name: FileName,
    pub lifecycle: DirectoryLifecycleState,
    pub version: VersionKey,
}

#[derive(Debug, PartialEq, Eq)]
pub struct DirectoryProjection {
    /// Live directories in parent-before-child materialization order.
    pub ordered_active: Vec<DirId>,
    /// Deleted or unreachable directories.
    pub suppressed: HashSet<DirId>,
    /// Effective parents; cycle repairs never overwrite synchronized intents.
    pub parents: HashMap<DirId, DirId>,
    /// Filesystem-safe, case-insensitively unique display names. Concurrent
    /// sibling collisions are retained and numbered instead of disappearing.
    pub names: HashMap<DirId, FileName>,
}

pub fn project_directories(intents: &HashMap<DirId, DirectoryIntent>) -> DirectoryProjection {
    let mut suppressed: HashSet<_> = intents.values().filter(|intent| intent.lifecycle != DirectoryLifecycleState::Present).map(|intent| intent.id).collect();
    let mut parents = intents.iter().map(|(id, intent)| (*id, intent.parent_id)).collect();
    repair_cycles(intents, &suppressed, &mut parents);
    cascade_unreachable(intents, &parents, &mut suppressed);
    let names = resolve_path_collisions(intents, &parents, &suppressed);

    let mut remaining: Vec<_> = intents.keys().filter(|id| !suppressed.contains(*id)).copied().collect();
    remaining.sort();
    let mut active = HashSet::new();
    let mut ordered_active = Vec::with_capacity(remaining.len());
    while !remaining.is_empty() {
        let position = remaining
            .iter()
            .position(|id| {
                let parent = parents[id];
                parent == ROOT_DIR_ID || active.contains(&parent)
            })
            .expect("cycles were repaired and unreachable parents were suppressed");
        let id = remaining.remove(position);
        active.insert(id);
        ordered_active.push(id);
    }
    DirectoryProjection { ordered_active, suppressed, parents, names }
}

fn cascade_unreachable(intents: &HashMap<DirId, DirectoryIntent>, parents: &HashMap<DirId, DirId>, suppressed: &mut HashSet<DirId>) {
    loop {
        let newly_suppressed: Vec<_> = intents
            .values()
            .filter(|intent| !suppressed.contains(&intent.id))
            .filter(|intent| parents[&intent.id] != ROOT_DIR_ID && (!intents.contains_key(&parents[&intent.id]) || suppressed.contains(&parents[&intent.id])))
            .map(|intent| intent.id)
            .collect();
        if newly_suppressed.is_empty() {
            break;
        }
        suppressed.extend(newly_suppressed);
    }
}

fn repair_cycles(intents: &HashMap<DirId, DirectoryIntent>, suppressed: &HashSet<DirId>, parents: &mut HashMap<DirId, DirId>) {
    let mut ids: Vec<_> = intents.keys().copied().collect();
    ids.sort();
    for start in ids {
        if suppressed.contains(&start) {
            continue;
        }
        let mut path = Vec::new();
        let mut positions = HashMap::new();
        let mut current = start;
        loop {
            if suppressed.contains(&current) || current == ROOT_DIR_ID {
                break;
            }
            if let Some(position) = positions.get(&current).copied() {
                if let Some(loser) = path[position..].iter().max().copied() {
                    parents.insert(loser, ROOT_DIR_ID);
                }
                break;
            }
            let Some(parent) = parents.get(&current) else {
                break;
            };
            positions.insert(current, path.len());
            path.push(current);
            current = *parent;
        }
    }
}

fn resolve_path_collisions(intents: &HashMap<DirId, DirectoryIntent>, parents: &HashMap<DirId, DirId>, suppressed: &HashSet<DirId>) -> HashMap<DirId, FileName> {
    let mut groups: HashMap<(DirId, String), Vec<&DirectoryIntent>> = HashMap::new();
    let mut occupied: HashMap<DirId, HashSet<String>> = HashMap::new();
    let mut names = HashMap::new();
    for intent in intents.values().filter(|intent| !suppressed.contains(&intent.id)) {
        let key = crate::portable_name_key(&intent.name);
        occupied.entry(parents[&intent.id]).or_default().insert(key.clone());
        groups.entry((parents[&intent.id], key)).or_default().push(intent);
        names.insert(intent.id, intent.name.clone());
    }
    let mut collisions: Vec<_> = groups.into_iter().filter(|(_, group)| group.len() > 1).collect();
    collisions.sort_by(|left, right| left.0.cmp(&right.0));
    for ((parent_id, _), mut group) in collisions {
        group.sort_by(|left, right| right.version.cmp(&left.version).then_with(|| right.id.cmp(&left.id)));
        let base = group[0].name.clone();
        let parent_occupied = occupied.entry(parent_id).or_default();
        for (offset, loser) in group.into_iter().skip(1).enumerate() {
            let mut number = offset + 2;
            loop {
                let candidate = format!("{base} {number}");
                if parent_occupied.insert(crate::portable_name_key(&candidate)) {
                    names.insert(loser.id, candidate);
                    break;
                }
                number += 1;
            }
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use sync_common::MutationId;
    use test_support::{fixture_dir_id, fixture_replica_id};

    fn version() -> VersionKey {
        VersionKey::new(1, 0, fixture_replica_id("projection"), 1, MutationId::parse("00000000-0000-0000-0000-000000000001").unwrap())
    }

    fn intent(id: &str, parent: &str, name: &str) -> DirectoryIntent {
        DirectoryIntent { id: fixture_dir_id(id), parent_id: fixture_dir_id(parent), name: name.to_owned(), lifecycle: DirectoryLifecycleState::Present, version: version() }
    }

    fn intents(values: impl IntoIterator<Item = DirectoryIntent>) -> HashMap<DirId, DirectoryIntent> {
        values.into_iter().map(|intent| (intent.id, intent)).collect()
    }

    #[test]
    fn orders_parents_before_children_and_cascades_deletion() {
        let mut parent = intent("parent", "00000000-0000-0000-0000-000000000000", "Parent");
        parent.lifecycle = DirectoryLifecycleState::Deleted;
        let child = intent("child", "parent", "Child");
        let projection = project_directories(&intents([child, parent]));
        assert!(projection.ordered_active.is_empty());
        assert_eq!(projection.suppressed.len(), 2);
    }

    #[test]
    fn deterministically_repairs_cycles_and_numbers_case_insensitive_collisions() {
        let a = intent("a", "b", "A");
        let b = intent("b", "a", "B");
        let first = intent("first", "00000000-0000-0000-0000-000000000000", "Same");
        let second = intent("second", "00000000-0000-0000-0000-000000000000", "same");
        let projection = project_directories(&intents([a, b, first, second]));
        assert!(projection.suppressed.is_empty());
        assert_eq!(projection.parents[&fixture_dir_id("b")], ROOT_DIR_ID);
        assert!(!projection.suppressed.contains(&fixture_dir_id("first")));
        assert!(projection.ordered_active.contains(&fixture_dir_id("second")));
        assert_eq!(projection.names[&fixture_dir_id("second")], "same");
        assert_eq!(projection.names[&fixture_dir_id("first")], "same 2");
    }
    #[test]
    fn repairs_long_and_self_cycles_before_resolving_root_names() {
        let make = || vec![
            intent("a", "b", "A"),
            intent("b", "c", "B"),
            intent("c", "a", "Shelf"),
            intent("child", "a", "Child"),
            intent("self", "self", "Self"),
            intent("existing", "00000000-0000-0000-0000-000000000000", "shelf"),
        ];
        let input = intents(make());
        let projection = project_directories(&input);
        assert_eq!(projection, project_directories(&intents(make().into_iter().rev())));
        assert_eq!(projection, project_directories(&input));
        assert!(projection.suppressed.is_empty());
        let cycle_root = [fixture_dir_id("a"), fixture_dir_id("b"), fixture_dir_id("c")].into_iter().max().unwrap();
        assert_eq!(cycle_root, fixture_dir_id("c"));
        assert_eq!(projection.parents[&cycle_root], ROOT_DIR_ID);
        assert_eq!(projection.parents[&fixture_dir_id("self")], ROOT_DIR_ID);
        assert_eq!(input[&cycle_root].parent_id, fixture_dir_id("a"));
        let mut occupied = HashSet::new();
        let mut active = HashSet::from([ROOT_DIR_ID]);
        for id in &projection.ordered_active {
            let parent = projection.parents[id];
            assert!(active.contains(&parent));
            assert!(occupied.insert((parent, crate::portable_name_key(&projection.names[id]))));
            active.insert(*id);
        }
    }

}
