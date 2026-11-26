//! The tree a graph draws.

use std::ops::Range;

/// Breadth-first nodes: node zero is the root, and every other node names an
/// earlier parent. Each parent's children form one consecutive group.
pub struct GraphTree {
    pub(super) parents: Vec<Option<usize>>,
    first_child: Vec<usize>,
}

impl GraphTree {
    /// Rejects empty trees, extra roots and parents outside breadth-first order.
    pub fn new(parents: Vec<Option<usize>>) -> Result<Self, &'static str> {
        if parents.first() != Some(&None) {
            return Err("Graph must have a root at index zero");
        }
        let mut first_child = vec![0; parents.len()];
        let mut previous = 0;
        for (node, parent) in parents.iter().enumerate().skip(1) {
            let Some(parent) = *parent else { return Err("Each non-root graph node must have a parent") };
            if parent >= node || parent < previous {
                return Err("Graph parents must follow breadth-first order");
            }
            first_child[parent] += 1;
            previous = parent;
        }
        let mut next = 1;
        for offset in &mut first_child {
            let count = *offset;
            *offset = next;
            next += count;
        }
        Ok(Self { parents, first_child })
    }

    pub(super) fn children(&self, node: usize) -> Range<usize> {
        self.first_child[node]..self.first_child.get(node + 1).copied().unwrap_or(self.parents.len())
    }

    pub(super) fn traversal(&self) -> Range<usize> {
        0..self.parents.len()
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
    fn breadth_first_nodes_have_a_bounded_path_to_the_root() {
        let tree = GraphTree::new(vec![None, Some(0), Some(0), Some(1)]).unwrap();
        assert_eq!(tree.ancestry(3), Some(vec![0, 1, 3]));
        assert_eq!(tree.ancestry(4), None);
    }
}
