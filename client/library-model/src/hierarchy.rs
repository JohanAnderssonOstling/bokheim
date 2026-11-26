//! Immutable breadth-first navigation shared by browsing and graph views.

use std::ops::Range;

use serde::{Deserialize, Serialize};

use crate::{BrowsePathSegment, BrowseRow};

/// Node zero is the root. Each parent's children form one consecutive group.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BrowseHierarchy {
    pub nodes: Vec<BrowseHierarchyNode>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BrowseHierarchyNode {
    pub row: BrowseRow,
    pub parent: Option<usize>,
    /// The next node's offset ends this node's immediate-child range.
    pub first_child: usize,
}

impl BrowseHierarchy {
    pub fn new(nodes: Vec<BrowseHierarchyNode>) -> Result<Self, &'static str> {
        if nodes.first().is_none_or(|root| root.parent.is_some() || root.first_child != 1) {
            return Err("Hierarchy must have a root at index zero");
        }
        let result = Self { nodes };
        for (index, node) in result.nodes.iter().enumerate() {
            let end = result.nodes.get(index + 1).map_or(result.nodes.len(), |next| next.first_child);
            if node.first_child <= index || end < node.first_child || end > result.nodes.len() {
                return Err("Hierarchy children are not packed breadth-first");
            }
            for child in node.first_child..end {
                if result.nodes[child].parent != Some(index) {
                    return Err("Hierarchy parent does not match its child range");
                }
            }
        }
        Ok(result)
    }

    pub fn children(&self, node: usize) -> Range<usize> {
        let Some(node_data) = self.nodes.get(node) else { return 0..0 };
        node_data.first_child..self.nodes.get(node + 1).map_or(self.nodes.len(), |next| next.first_child)
    }

    /// Root is omitted, as in a browse page's breadcrumb trail.
    pub fn path(&self, mut node: usize) -> Vec<BrowsePathSegment> {
        let mut path = Vec::new();
        while let Some(entry) = self.nodes.get(node) {
            let Some(parent) = entry.parent else { break };
            path.push(BrowsePathSegment { id: entry.row.id, name: entry.row.name.clone() });
            node = parent;
        }
        path.reverse();
        path
    }
}
