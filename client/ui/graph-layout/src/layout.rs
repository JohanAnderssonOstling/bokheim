//! Column layout of a tree of sized nodes.
//!
//! The caller measures each node in each role, since it draws them; the layout
//! only places boxes. In columns the root is centred between two columns of
//! children, and each expanded node grows one more column outward on its side.
//! A column slot fits every role its node may take and reserves height for its
//! descendants, so expanded branches do not overlap.

use std::f32::consts::PI;
use std::sync::Arc;

use gpui::{Bounds, Point, Size, point};

use super::tree::GraphTree;

/// What a node is to the selection, for the caller to draw it by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NodeRole {
    Root,
    SelectedRoot,
    Path,
    Selection,
    Fan,
    Context,
}

impl NodeRole {
    pub const ALL: [NodeRole; 6] = [NodeRole::Root, NodeRole::SelectedRoot, NodeRole::Path, NodeRole::Selection, NodeRole::Fan, NodeRole::Context];
}

#[derive(Clone, Copy, Debug)]
pub struct LayoutParams {
    pub column_gap: f32,
    pub row_gap: f32,
}

impl LayoutParams {
    pub fn desktop() -> Self {
        Self { column_gap: 40.0, row_gap: 6.0 }
    }

    pub fn compact() -> Self {
        Self { column_gap: 32.0, row_gap: 12.0 }
    }
}

#[derive(Clone, Debug)]
pub struct PlacedNode {
    pub node: usize,
    pub parent: Option<usize>,
    pub role: NodeRole,
    pub center: Point<f32>,
    /// Zero points right; π points left. Used to frame compact screens.
    pub angle: f32,
    pub bounds: Bounds<f32>,
    pub opacity: f32,
}

#[derive(Clone, Debug)]
pub struct GraphEdge {
    pub from: usize,
    pub to: usize,
    pub start: Point<f32>,
    pub end: Point<f32>,
    pub on_path: bool,
    pub opacity: f32,
}

#[derive(Clone)]
pub struct GraphLayout {
    pub(super) nodes: Vec<PlacedNode>,
    pub(super) edges: Arc<Vec<GraphEdge>>,
    pub(super) path: Vec<usize>,
    node_index: Vec<Option<usize>>,
}

impl GraphLayout {
    pub fn nodes(&self) -> &[PlacedNode] {
        &self.nodes
    }

    pub fn edges(&self) -> &[GraphEdge] {
        &self.edges
    }

    /// Shares connector geometry with render closures without copying it.
    pub fn shared_edges(&self) -> Arc<Vec<GraphEdge>> {
        self.edges.clone()
    }

    pub fn path(&self) -> &[usize] {
        &self.path
    }

    pub fn retain_edges(&mut self, keep: impl FnMut(&GraphEdge) -> bool) {
        Arc::make_mut(&mut self.edges).retain(keep);
    }

    pub fn node(&self, node: usize) -> Option<&PlacedNode> {
        self.node_index.get(node).copied().flatten().and_then(|index| self.nodes.get(index))
    }

    pub fn retain_nodes(&mut self, mut keep: impl FnMut(&PlacedNode) -> bool) {
        self.nodes.retain(|node| keep(node));
        self.node_index.fill(None);
        for (index, node) in self.nodes.iter().enumerate() {
            self.node_index[node.node] = Some(index);
        }
    }

    /// Supplies a node's position and opacity together. Nodes without a pose
    /// keep their settled geometry, with connectors updated from that geometry.
    pub fn posed(&self, mut pose: impl FnMut(&PlacedNode) -> Option<(Point<f32>, f32)>) -> Self {
        let mut posed = self.clone();
        for node in &mut posed.nodes {
            if let Some((center, opacity)) = pose(node) {
                let delta = point(center.x - node.center.x, center.y - node.center.y);
                node.center = center;
                node.bounds = shifted(node.bounds, delta);
                node.opacity = opacity;
            }
        }
        let nodes = &posed.nodes;
        let node_index = &posed.node_index;
        Arc::make_mut(&mut posed.edges).retain_mut(|edge| {
            let placed = |id: usize| node_index.get(id).copied().flatten().and_then(|index| nodes.get(index));
            let (Some(from), Some(to)) = (placed(edge.from), placed(edge.to)) else { return false };
            edge.opacity = to.opacity;
            let (Some(old_from), Some(old_to)) = (self.node(edge.from), self.node(edge.to)) else { return false };
            edge.start = point(edge.start.x + from.center.x - old_from.center.x, edge.start.y + from.center.y - old_from.center.y);
            edge.end = point(edge.end.x + to.center.x - old_to.center.x, edge.end.y + to.center.y - old_to.center.y);
            true
        });
        posed
    }
}

/// Lays out every node in the visible tree, with each node the size `size`
/// gives it in its role. The selection determines emphasis and the open path.
/// `root_sides` assigns each root child to the right (`true`) or left (`false`),
/// in child order. Returns an error for an invalid selection or side assignment.
pub fn graph_layout(tree: &GraphTree, selection: usize, root_sides: &[bool], size: &dyn Fn(usize, NodeRole) -> Size<f32>, params: &LayoutParams) -> Result<GraphLayout, &'static str> {
    let path = tree.ancestry(selection).ok_or("Graph selection is out of bounds")?;
    if root_sides.len() != tree.children(0).len() {
        return Err("Graph root sides do not match its children");
    }
    let mut builder = Builder { tree, root_sides, size, params, selection, path: &path, nodes: Vec::new(), node_index: vec![None; tree.parents.len()], edges: Vec::new(), column_slots: Vec::new() };
    builder.place(0, if selection == 0 { NodeRole::SelectedRoot } else { NodeRole::Root }, None, point(0.0, 0.0), 0.0);
    builder.reserve_columns();
    builder.columns(0, 0.0, 0.0);
    let Builder { nodes, edges, node_index, .. } = builder;
    Ok(GraphLayout { nodes, edges: Arc::new(edges), path, node_index })
}

struct Builder<'a> {
    tree: &'a GraphTree,
    root_sides: &'a [bool],
    size: &'a dyn Fn(usize, NodeRole) -> Size<f32>,
    params: &'a LayoutParams,
    selection: usize,
    path: &'a [usize],
    nodes: Vec<PlacedNode>,
    node_index: Vec<Option<usize>>,
    edges: Vec<GraphEdge>,
    column_slots: Vec<Size<f32>>,
}

impl Builder<'_> {
    fn role(&self, node: usize, parent: usize) -> NodeRole {
        if node == self.selection {
            NodeRole::Selection
        } else if self.path.contains(&node) {
            NodeRole::Path
        } else if parent == self.selection {
            NodeRole::Fan
        } else {
            NodeRole::Context
        }
    }

    fn bounds(&self, node: usize, role: NodeRole, center: Point<f32>) -> Bounds<f32> {
        let size = (self.size)(node, role);
        Bounds::new(point(center.x - size.width / 2.0, center.y - size.height / 2.0), size)
    }

    fn place(&mut self, node: usize, role: NodeRole, parent: Option<usize>, center: Point<f32>, angle: f32) {
        let bounds = self.bounds(node, role, center);
        self.node_index[node] = Some(self.nodes.len());
        self.nodes.push(PlacedNode { node, parent, role, center, angle, bounds, opacity: 1.0 });
    }

    fn placed(&self, node: usize) -> Option<&PlacedNode> {
        self.node_index.get(node).copied().flatten().and_then(|index| self.nodes.get(index))
    }

    /// Reserve a vertical interval for each complete subtree before placing it.
    fn reserve_columns(&mut self) {
        self.column_slots = vec![Size { width: 0.0, height: 0.0 }; self.tree.parents.len()];
        for node in self.tree.traversal().rev() {
            let mut slot = if node == 0 { self.nodes[0].bounds.size } else { self.label_slot(node) };
            let children = self.tree.children(node);
            let height = children.clone().map(|child| self.column_slots[child].height).sum::<f32>() + self.params.row_gap * children.len().saturating_sub(1) as f32;
            slot.height = slot.height.max(height);
            self.column_slots[node] = slot;
        }
    }

    fn label_slot(&self, node: usize) -> Size<f32> {
        [NodeRole::Selection, NodeRole::Path, NodeRole::Fan, NodeRole::Context]
            .into_iter()
            .map(|role| (self.size)(node, role))
            .fold(Size { width: 0.0_f32, height: 0.0_f32 }, |slot, size| Size { width: slot.width.max(size.width), height: slot.height.max(size.height) })
    }

    /// `side` is zero at the root, then +1 or -1. `outer` is the far edge of
    /// the parent's column.
    fn columns(&mut self, node: usize, side: f32, outer: f32) {
        let tree = self.tree;
        let children = tree.children(node);
        if children.len() == 0 {
            return;
        }
        let Some(parent) = self.placed(node) else { return };
        let (parent_center, parent_width) = (parent.center, parent.bounds.size.width);

        let root_sides = self.root_sides;
        let first_child = children.start;
        let directions = if side == 0.0 { [1.0, -1.0] } else { [side, 0.0] };
        for direction in directions {
            if direction == 0.0 {
                continue;
            }
            // Filtering preserves the original sibling order on each side.
            let group = children.clone().filter(|child| side != 0.0 || root_sides[*child - first_child] == (direction > 0.0));
            let count = group.clone().count();
            if count == 0 {
                continue;
            }
            let column_height: f32 = group.clone().map(|kid| self.column_slots[kid].height).sum::<f32>() + self.params.row_gap * (count - 1) as f32;
            let parent_outer = if side == 0.0 { parent_center.x + direction * parent_width / 2.0 } else { outer };
            let edge = parent_outer + direction * self.params.column_gap;
            let column_outer = edge + direction * group.clone().map(|kid| self.column_slots[kid].width).fold(0.0, f32::max);
            let mut y = parent_center.y - column_height / 2.0;
            for kid in group.clone() {
                let slot = self.column_slots[kid];
                let role = self.role(kid, node);
                let width = (self.size)(kid, role).width;
                let center = point(edge + direction * slot.width / 2.0, y + slot.height / 2.0);
                self.edges.push(GraphEdge {
                    from: node,
                    to: kid,
                    start: point(parent_center.x + direction * parent_width / 2.0, parent_center.y),
                    end: point(center.x - direction * width / 2.0, center.y),
                    on_path: self.path.contains(&node) && self.path.contains(&kid),
                    opacity: 1.0,
                });
                self.place(kid, role, Some(node), center, if direction > 0.0 { 0.0 } else { PI });
                y += slot.height + self.params.row_gap;
            }
            for kid in group {
                self.columns(kid, direction, column_outer);
            }
        }
    }
}

fn shifted(bounds: Bounds<f32>, delta: Point<f32>) -> Bounds<f32> {
    Bounds::new(point(bounds.origin.x + delta.x, bounds.origin.y + delta.y), bounds.size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framing_camera;
    use gpui::{px, size};

    #[test]
    fn an_invalid_selection_is_rejected_before_measuring_nodes() {
        let tree = GraphTree::new(vec![None]).unwrap();
        assert!(graph_layout(&tree, 1, &[], &|_, _| panic!("invalid selection must not request layout"), &LayoutParams::desktop()).is_err());
    }

    #[test]
    fn a_filtered_layout_can_be_posed_and_framed_without_its_selection() {
        let tree = GraphTree::new(vec![None, Some(0), Some(0)]).unwrap();
        let mut layout = graph_layout(&tree, 1, &[true, false], &|_, _| size(20.0, 20.0), &LayoutParams::desktop()).unwrap();
        layout.retain_nodes(|node| node.node != 1);
        let posed = layout.posed(|node| Some(([point(0.0, 0.0), point(50.0, 50.0), point(100.0, 100.0)][node.node], 1.0)));
        assert_eq!(posed.edges.len(), 1);
        assert!(posed.edges.iter().all(|edge| posed.node(edge.from).is_some() && posed.node(edge.to).is_some()));
        assert_eq!(posed.node(2).unwrap().center, point(100.0, 100.0));

        let area = Bounds::new(point(px(0.0), px(0.0)), size(px(800.0), px(600.0)));
        let camera = framing_camera(&posed, area, false, 1.0);
        assert_eq!(camera.pan, area.center());
        assert_eq!(camera.zoom, 1.0);
        layout.path.clear();
        assert_eq!(framing_camera(&layout, area, true, 1.0), camera);
    }
}
