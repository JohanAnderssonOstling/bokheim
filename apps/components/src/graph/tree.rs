//! The tree a graph draws.

/// Nodes by index: node 0 is the root, and every other node names its parent.
/// Children keep the order the caller lists them in. Construction validates
/// that every node reaches the root; callers cannot mutate the topology.
pub struct GraphTree {
    pub(super) parents: Vec<Option<usize>>,
    pub(super) children: Vec<Vec<usize>>,
}

impl GraphTree {
    /// Rejects empty trees, invalid parent indices, extra roots, and cycles.
    pub fn new(parents: Vec<Option<usize>>) -> Result<Self, &'static str> {
        if parents.first() != Some(&None) {
            return Err("Graph must have a root at index zero");
        }
        for parent in parents.iter().skip(1) {
            let Some(parent) = parent else { return Err("Each non-root graph node must have a parent") };
            if *parent >= parents.len() {
                return Err("Graph parent index is out of bounds");
            }
        }
        // Parent indices are now in bounds. Mark each chain once so cycle
        // validation is linear and does not recurse on deeply nested trees.
        let mut state = vec![0_u8; parents.len()];
        state[0] = 2;
        for start in 1..parents.len() {
            let mut node = start;
            while state[node] == 0 {
                state[node] = 1;
                let Some(parent) = parents[node] else { return Err("Graph node does not reach the root") };
                node = parent;
            }
            if state[node] == 1 {
                return Err("Graph contains a parent cycle");
            }
            node = start;
            while state[node] == 1 {
                state[node] = 2;
                let Some(parent) = parents[node] else { break };
                node = parent;
            }
        }
        let mut children = vec![Vec::new(); parents.len()];
        for (node, parent) in parents.iter().enumerate() {
            if let Some(parent) = parent {
                children[*parent].push(node);
            }
        }
        Ok(Self { parents, children })
    }

    /// The path from the root to `id`, both included.
    pub fn ancestry(&self, id: usize) -> Option<Vec<usize>> {
        self.parents.get(id)?;
        let mut path: Vec<usize> = std::iter::successors(Some(id), |&node| self.parents[node]).collect();
        path.reverse();
        Some(path)
    }
}

#[cfg(test)]
mod tests {
    use super::GraphTree;

    #[test]
    fn invalid_topologies_are_rejected_before_layout() {
        for parents in [vec![], vec![Some(0)], vec![None, None], vec![None, Some(2)], vec![None, Some(1)], vec![None, Some(2), Some(1)]] {
            assert!(GraphTree::new(parents).is_err());
        }
    }

    #[test]
    fn valid_forward_references_have_a_bounded_path_to_the_root() {
        let tree = GraphTree::new(vec![None, Some(2), Some(0)]).unwrap();
        assert_eq!(tree.ancestry(1), Some(vec![0, 2, 1]));
        assert_eq!(tree.ancestry(3), None);
    }
}
