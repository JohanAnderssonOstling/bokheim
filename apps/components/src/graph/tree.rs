//! The tree a graph draws.

/// Nodes by index: node 0 is the root, and every other node names its parent.
/// Children keep the order the caller lists them in.
pub struct GraphTree {
    pub parents: Vec<Option<usize>>,
    pub children: Vec<Vec<usize>>,
}

impl GraphTree {
    pub fn new(parents: Vec<Option<usize>>) -> Self {
        let mut children = vec![Vec::new(); parents.len()];
        for (node, parent) in parents.iter().enumerate() {
            if let Some(parent) = parent {
                children[*parent].push(node);
            }
        }
        Self { parents, children }
    }

    /// The path from the root to `id`, both included.
    pub fn ancestry(&self, id: usize) -> Vec<usize> {
        let mut path: Vec<usize> = std::iter::successors(Some(id), |&node| self.parents[node]).collect();
        path.reverse();
        path
    }
}
