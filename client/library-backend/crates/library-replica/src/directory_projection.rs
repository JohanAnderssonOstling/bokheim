use crate::DirectoryLifecycleState;
use std::collections::{BTreeSet, HashMap, HashSet};
use sync_common::{DirId, FileName, ROOT_DIR_ID};

#[derive(Debug)]
pub struct DirectoryIntent {
    pub id: DirId,
    pub parent_id: DirId,
    pub name: FileName,
    pub lifecycle: DirectoryLifecycleState,
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

/// A parent chain that loops back on itself is cut by moving its largest id
/// to the root, so every device repairs the same cycle the same way.
fn break_cycles(intents: &HashMap<DirId, DirectoryIntent>, suppressed: &HashSet<DirId>, parents: &mut HashMap<DirId, DirId>) {
    let mut visited = HashSet::new();
    let mut path = Vec::new();
    for &start in intents.keys() {
        if suppressed.contains(&start) || visited.contains(&start) {
            continue;
        }
        path.clear();
        let mut current = start;
        loop {
            if suppressed.contains(&current) || current == ROOT_DIR_ID {
                break;
            }
            if !visited.insert(current) {
                // A repeated node in this path is a cycle; otherwise the walk
                // joined a parent chain that was already inspected.
                if let Some(position) = path.iter().position(|id| *id == current) {
                    if let Some(loser) = path[position..].iter().max().copied() {
                        parents.insert(loser, ROOT_DIR_ID);
                    }
                }
                break;
            }
            let Some(parent) = parents.get(&current) else {
                break;
            };
            path.push(current);
            current = *parent;
        }
    }
}

/// Propagate deleted and missing-parent state through their descendants once.
fn suppress_orphans(children: &HashMap<DirId, Vec<DirId>>, suppressed: &mut HashSet<DirId>) {
    let mut pending: Vec<_> = suppressed.iter().copied().collect();
    while let Some(parent) = pending.pop() {
        for child in children.get(&parent).into_iter().flatten() {
            if suppressed.insert(*child) {
                pending.push(*child);
            }
        }
    }
}

pub fn project_directories(intents: &HashMap<DirId, DirectoryIntent>) -> DirectoryProjection {
    let mut suppressed: HashSet<_> = intents.values().filter(|intent| intent.lifecycle != DirectoryLifecycleState::Present).map(|intent| intent.id).collect();
    let mut parents = intents.iter().map(|(id, intent)| (*id, intent.parent_id)).collect();
    break_cycles(intents, &suppressed, &mut parents);
    let mut children: HashMap<DirId, Vec<DirId>> = HashMap::new();
    for (&id, &parent) in &parents {
        if parent != ROOT_DIR_ID {
            children.entry(parent).or_default().push(id);
            if !parents.contains_key(&parent) {
                suppressed.insert(id);
            }
        }
    }
    suppress_orphans(&children, &mut suppressed);
    let names = {
        let mut groups: HashMap<(DirId, String), Vec<&DirectoryIntent>> = HashMap::new();
        let mut occupied: HashMap<DirId, HashSet<String>> = HashMap::new();
        let mut names: HashMap<_, _> = intents.values().map(|intent| (intent.id, crate::project_folder_name(&intent.name))).collect();
        for intent in intents.values().filter(|intent| !suppressed.contains(&intent.id)) {
            let key = crate::portable_name_key(&names[&intent.id]);
            occupied.entry(parents[&intent.id]).or_default().insert(key.clone());
            groups.entry((parents[&intent.id], key)).or_default().push(intent);
        }
        let mut collisions: Vec<_> = groups.into_iter().filter(|(_, group)| group.len() > 1).collect();
        collisions.sort_by(|left, right| left.0.cmp(&right.0));
        for ((parent_id, _), mut group) in collisions {
            // Stable identity, not timestamps: recovery cannot reshuffle names.
            group.sort_by(|left, right| right.id.cmp(&left.id));
            let base = names[&group[0].id].clone();
            let parent_occupied = occupied.entry(parent_id).or_default();
            for (offset, loser) in group.into_iter().skip(1).enumerate() {
                let mut number = offset + 2;
                loop {
                    let candidate = crate::numbered_folder_name(&base, number);
                    if parent_occupied.insert(crate::portable_name_key(&candidate)) {
                        names.insert(loser.id, candidate);
                        break;
                    }
                    number += 1;
                }
            }
        }
        names
    };

    let active_count = intents.len() - suppressed.len();
    let mut ready: BTreeSet<_> = parents.iter().filter_map(|(id, parent)| (*parent == ROOT_DIR_ID && !suppressed.contains(id)).then_some(*id)).collect();
    let mut ordered_active = Vec::with_capacity(active_count);
    while let Some(id) = ready.pop_first() {
        ordered_active.push(id);
        ready.extend(children.get(&id).into_iter().flatten().copied().filter(|child| !suppressed.contains(child)));
    }
    assert_eq!(ordered_active.len(), active_count, "cycles were repaired and unreachable parents were suppressed");
    DirectoryProjection { ordered_active, suppressed, parents, names }
}

#[cfg(test)]
mod traversal_tests {
    use super::*;
    use DirectoryLifecycleState::{Deleted, Present, Purged};

    fn intents(rows: &[(u128, u128, DirectoryLifecycleState)]) -> HashMap<DirId, DirectoryIntent> {
        rows.iter()
            .map(|(id, parent, lifecycle)| {
                let id = DirId::from_u128(*id);
                (id, DirectoryIntent { id, parent_id: DirId::from_u128(*parent), name: format!("Folder {id}"), lifecycle: lifecycle.clone() })
            })
            .collect()
    }

    #[test]
    fn materialization_chooses_the_smallest_ready_id_after_every_parent() {
        let input = intents(&[(1, 5, Present), (2, 9, Present), (3, 5, Present), (4, 9, Present), (5, 0, Present), (6, 0, Present), (9, 0, Present)]);
        let projection = project_directories(&input);
        assert_eq!(projection.ordered_active, [5, 1, 3, 6, 9, 2, 4].map(DirId::from_u128));
        assert!(projection.suppressed.is_empty());
    }

    #[test]
    fn cycle_repairs_ignore_feeders_shared_tails_and_suppressed_cycles() {
        let mut rows = vec![
            (100, 2, Present),
            (101, 3, Present),
            (102, 100, Present),
            (103, 102, Present),
            (2, 3, Present),
            (3, 4, Present),
            (4, 2, Present),
            (200, 10, Present),
            (10, 20, Present),
            (20, 10, Present),
            (30, 30, Present),
            (40, 41, Deleted),
            (41, 40, Present),
            (42, 41, Present),
            (50, 99, Present),
            (51, 50, Present),
            (60, 0, Present),
            (61, 60, Present),
        ];
        let mut expected_parents: HashMap<_, _> = rows.iter().map(|(id, parent, _)| (DirId::from_u128(*id), DirId::from_u128(*parent))).collect();
        for id in [4, 20, 30] {
            expected_parents.insert(DirId::from_u128(id), ROOT_DIR_ID);
        }
        for _ in 0..rows.len() {
            let projection = project_directories(&intents(&rows));
            assert_eq!(projection.parents, expected_parents);
            assert_eq!(projection.suppressed, [40, 41, 42, 50, 51].map(DirId::from_u128).into_iter().collect());
            assert_eq!(projection.ordered_active, [4, 3, 2, 20, 10, 30, 60, 61, 100, 101, 102, 103, 200].map(DirId::from_u128));
            rows.rotate_left(1);
        }
    }

    #[test]
    fn suppression_propagates_through_missing_deleted_and_purged_parents_without_hiding_repaired_cycles() {
        let mut rows =
            vec![(1, 0, Present), (2, 1, Present), (10, 0, Deleted), (11, 10, Present), (12, 11, Present), (20, 99, Present), (21, 20, Present), (22, 21, Present), (30, 0, Purged), (31, 30, Present), (40, 41, Present), (41, 40, Present)];
        let projection = project_directories(&intents(&rows));
        assert_eq!(projection.suppressed, [10, 11, 12, 20, 21, 22, 30, 31].map(DirId::from_u128).into_iter().collect());
        assert_eq!(projection.ordered_active, [1, 2, 41, 40].map(DirId::from_u128));
        assert_eq!(projection.parents[&DirId::from_u128(41)], ROOT_DIR_ID);
        rows.reverse();
        assert_eq!(project_directories(&intents(&rows)), projection);
    }
}

#[cfg(test)]
mod safe_component_tests {
    use super::*;

    #[test]
    fn collisions_after_sanitizing_and_truncating_reserve_existing_suffixes() {
        let requests = ["A/B".to_owned(), "A\\B".to_owned(), "A_B 2".to_owned(), "..".to_owned(), "Folder".to_owned(), "CON.txt".to_owned(), "a".repeat(255), "a".repeat(256), format!("{} 2", "a".repeat(238))];
        let make = |reverse: bool| {
            let mut values = requests
                .iter()
                .enumerate()
                .map(|(i, name)| {
                    let id = DirId::from_u128(i as u128 + 1);
                    (id, DirectoryIntent { id, parent_id: ROOT_DIR_ID, name: name.clone(), lifecycle: DirectoryLifecycleState::Present })
                })
                .collect::<Vec<_>>();
            if reverse {
                values.reverse();
            }
            values.into_iter().collect::<HashMap<_, _>>()
        };
        let input = make(false);
        let projected = project_directories(&input);
        assert_eq!(projected, project_directories(&make(true)));
        let name = |id| &projected.names[&DirId::from_u128(id)];
        assert_eq!(name(1), "A_B 3");
        assert_eq!(name(2), "A_B");
        assert_eq!(name(3), "A_B 2");
        assert_eq!(name(4), "Folder 3");
        assert_eq!(name(5), "Folder 2");
        assert_eq!(name(6), "Folder");
        assert_eq!(name(7), &format!("{} 3", "a".repeat(238)));
        assert_eq!(name(8), &"a".repeat(240));
        let mut occupied = HashSet::new();
        for name in projected.names.values() {
            assert_eq!(crate::filenames::validate_component(name).unwrap(), *name);
            assert!(occupied.insert(crate::portable_name_key(name)));
        }
        assert_eq!(input[&DirId::from_u128(1)].name, "A/B", "projection must retain the original intent");
    }
}

#[cfg(test)]
mod adversarial_component_properties {
    use super::*;

    #[test]
    fn sanitized_collision_groups_are_portable_unique_and_order_independent() {
        let corpus = [
            "".to_owned(),
            "..".to_owned(),
            "Folder".to_owned(),
            "Folder 2".to_owned(),
            "CON.txt".to_owned(),
            "A/B".to_owned(),
            "A\\B".to_owned(),
            "A_B".to_owned(),
            "A_B 2".to_owned(),
            "Étage".to_owned(),
            "E\u{301}tage".to_owned(),
            "Ｓｈｅｌｆ".to_owned(),
            "shelf".to_owned(),
            "\0\n\u{0085}".to_owned(),
            "界".repeat(100),
            "a".repeat(255),
            format!("{} 2", "a".repeat(238)),
            format!("CON{}X", " ".repeat(237)),
            "... valid ...".to_owned(),
        ];
        for seed in 0..128u64 {
            let mut random = seed;
            let mut input = HashMap::new();
            for index in 1..=40 {
                random = random.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                let id = DirId::from_u128(index);
                let name = corpus[(random >> 32) as usize % corpus.len()].clone();
                input.insert(id, DirectoryIntent { id, parent_id: ROOT_DIR_ID, name, lifecycle: DirectoryLifecycleState::Present });
            }
            let projection = project_directories(&input);
            let mut seen = HashSet::new();
            let mut projected_input = HashMap::new();
            for (id, name) in &projection.names {
                assert_eq!(crate::filenames::validate_component(name).unwrap(), *name, "seed={seed}");
                assert!(seen.insert(crate::portable_name_key(name)), "seed={seed}, duplicate={name}");
                projected_input.insert(*id, DirectoryIntent { id: *id, parent_id: ROOT_DIR_ID, name: name.clone(), lifecycle: DirectoryLifecycleState::Present });
            }
            assert_eq!(project_directories(&projected_input), projection, "seed={seed}, projection is not a fixed point");
            let mut reordered = input.into_iter().collect::<Vec<_>>();
            reordered.sort_by(|a, b| b.0.cmp(&a.0));
            assert_eq!(project_directories(&reordered.into_iter().collect()), projection, "seed={seed}, insertion order changed projection");
        }
    }
}
