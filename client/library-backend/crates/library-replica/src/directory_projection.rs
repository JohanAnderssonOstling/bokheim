use crate::DirectoryLifecycleState;
use std::collections::{HashMap, HashSet};
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

/// Directories under a missing or suppressed parent are suppressed too,
/// repeated until no more are found.
fn suppress_orphans(intents: &HashMap<DirId, DirectoryIntent>, parents: &HashMap<DirId, DirId>, suppressed: &mut HashSet<DirId>) {
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

pub fn project_directories(intents: &HashMap<DirId, DirectoryIntent>) -> DirectoryProjection {
    let mut suppressed: HashSet<_> = intents.values().filter(|intent| intent.lifecycle != DirectoryLifecycleState::Present).map(|intent| intent.id).collect();
    let mut parents = intents.iter().map(|(id, intent)| (*id, intent.parent_id)).collect();
    break_cycles(intents, &suppressed, &mut parents);
    suppress_orphans(intents, &parents, &mut suppressed);
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
