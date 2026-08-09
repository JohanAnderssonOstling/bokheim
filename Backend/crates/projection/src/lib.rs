use std::collections::{HashMap, HashSet};
use bokheim_domain::VersionKey;
use sync_common::{DirId, FileName};

#[derive(Clone, Debug)]
pub struct DirectoryIntent {
    pub id: DirId,
    pub parent_id: DirId,
    pub name: FileName,
    pub deleted: bool,
    pub version: Option<VersionKey>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectoryProjection {
    /// Live directories in parent-before-child materialization order.
    pub ordered_active: Vec<DirId>,
    /// Deleted, unreachable, cyclic, or path-collision-losing directories.
    pub suppressed: HashSet<DirId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProjectionError {
    DuplicateDirectory(DirId),
    UnreachableCycle,
}

impl std::fmt::Display for ProjectionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateDirectory(id) => write!(formatter, "duplicate directory intent {id}"),
            Self::UnreachableCycle => formatter.write_str("directory projection contains an unreachable cycle"),
        }
    }
}

impl std::error::Error for ProjectionError {}

pub fn project_directories(intents: impl IntoIterator<Item = DirectoryIntent>) -> Result<DirectoryProjection, ProjectionError> {
    let mut by_id = HashMap::new();
    for intent in intents {
        let id = intent.id.clone();
        if by_id.insert(id.clone(), intent).is_some() {
            return Err(ProjectionError::DuplicateDirectory(id));
        }
    }

    let mut suppressed: HashSet<_> = by_id.values().filter(|intent| intent.deleted).map(|intent| intent.id.clone()).collect();
    suppress_cycles(&by_id, &mut suppressed);
    cascade_unreachable(&by_id, &mut suppressed);
    suppress_path_collisions(&by_id, &mut suppressed);
    cascade_unreachable(&by_id, &mut suppressed);

    let mut remaining: Vec<_> = by_id.keys().filter(|id| !suppressed.contains(*id)).cloned().collect();
    remaining.sort();
    let mut active = HashSet::new();
    let mut ordered_active = Vec::with_capacity(remaining.len());
    while !remaining.is_empty() {
        let Some(position) = remaining.iter().position(|id| {
            let parent = &by_id[id].parent_id;
            parent.as_str() == "__root__" || active.contains(parent)
        }) else {
            return Err(ProjectionError::UnreachableCycle);
        };
        let id = remaining.remove(position);
        active.insert(id.clone());
        ordered_active.push(id);
    }
    Ok(DirectoryProjection { ordered_active, suppressed })
}

fn cascade_unreachable(intents: &HashMap<DirId, DirectoryIntent>, suppressed: &mut HashSet<DirId>) {
    loop {
        let newly_suppressed: Vec<_> = intents
            .values()
            .filter(|intent| !suppressed.contains(&intent.id))
            .filter(|intent| intent.parent_id.as_str() != "__root__" && (!intents.contains_key(&intent.parent_id) || suppressed.contains(&intent.parent_id)))
            .map(|intent| intent.id.clone())
            .collect();
        if newly_suppressed.is_empty() {
            break;
        }
        suppressed.extend(newly_suppressed);
    }
}

fn suppress_cycles(intents: &HashMap<DirId, DirectoryIntent>, suppressed: &mut HashSet<DirId>) {
    let mut ids: Vec<_> = intents.keys().cloned().collect();
    ids.sort();
    for start in ids {
        if suppressed.contains(&start) {
            continue;
        }
        let mut path = Vec::new();
        let mut positions = HashMap::new();
        let mut current = start;
        loop {
            if suppressed.contains(&current) || current.as_str() == "__root__" {
                break;
            }
            if let Some(position) = positions.get(&current).copied() {
                if let Some(loser) = path[position..].iter().max().cloned() {
                    suppressed.insert(loser);
                }
                break;
            }
            let Some(intent) = intents.get(&current) else {
                break;
            };
            positions.insert(current.clone(), path.len());
            path.push(current);
            current = intent.parent_id.clone();
        }
    }
}

fn suppress_path_collisions(intents: &HashMap<DirId, DirectoryIntent>, suppressed: &mut HashSet<DirId>) {
    let mut groups: HashMap<(DirId, FileName), Vec<&DirectoryIntent>> = HashMap::new();
    for intent in intents.values().filter(|intent| !suppressed.contains(&intent.id)) {
        groups.entry((intent.parent_id.clone(), intent.name.clone())).or_default().push(intent);
    }
    for group in groups.values().filter(|group| group.len() > 1) {
        let winner = group.iter().max_by(|left, right| left.version.cmp(&right.version).then_with(|| left.id.cmp(&right.id))).expect("collision group is non-empty");
        suppressed.extend(group.iter().filter(|intent| intent.id != winner.id).map(|intent| intent.id.clone()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn intent(id: &str, parent: &str, name: &str) -> DirectoryIntent {
        DirectoryIntent { id: DirId::parse(id).unwrap(), parent_id: DirId::parse(parent).unwrap(), name: FileName::parse(name).unwrap(), deleted: false, version: None }
    }

    #[test]
    fn orders_parents_before_children_and_cascades_deletion() {
        let mut parent = intent("parent", "__root__", "Parent");
        parent.deleted = true;
        let child = intent("child", "parent", "Child");
        let projection = project_directories([child, parent]).unwrap();
        assert!(projection.ordered_active.is_empty());
        assert_eq!(projection.suppressed.len(), 2);
    }

    #[test]
    fn deterministically_suppresses_a_cycle_and_path_collision() {
        let a = intent("a", "b", "A");
        let b = intent("b", "a", "B");
        let first = intent("first", "__root__", "Same");
        let second = intent("second", "__root__", "Same");
        let projection = project_directories([a, b, first, second]).unwrap();
        assert!(projection.suppressed.contains(&DirId::parse("b").unwrap()));
        assert!(projection.suppressed.contains(&DirId::parse("first").unwrap()));
        assert!(projection.ordered_active.contains(&DirId::parse("second").unwrap()));
    }
}
