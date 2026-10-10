//! Column and adaptive radial layouts of a tree of sized nodes.
//!
//! The caller measures each node in each role, since it draws them; the layout
//! only places boxes. In columns the root is centred between two columns of
//! children, and each node on the open path grows one more column outward on
//! its side. A column slot fits every role its node may take, so selecting a
//! sibling does not move its column.

use std::f32::consts::PI;
use std::sync::Arc;

use gpui::{Bounds, Point, Size, point};

use super::tree::GraphTree;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphLayoutKind {
    Columns,
    AdaptiveRadial,
}

/// What a node is to the selection, for the caller to draw it by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NodeRole {
    Root,
    Path,
    Selection,
    Fan,
    Context,
}

impl NodeRole {
    pub const ALL: [NodeRole; 5] = [NodeRole::Root, NodeRole::Path, NodeRole::Selection, NodeRole::Fan, NodeRole::Context];
}

#[derive(Clone, Copy, Debug)]
pub struct LayoutParams {
    pub column_gap: f32,
    pub row_gap: f32,
    pub root_radius: f32,
    pub fan_angle_step: f32,
    pub fan_max_angle: f32,
    pub radial_gap: f32,
    pub radial_step: f32,
    pub path_parent_gap: f32,
}

impl LayoutParams {
    pub fn desktop() -> Self {
        Self { column_gap: 72.0, row_gap: 10.0, root_radius: 270.0, fan_angle_step: 28.0, fan_max_angle: 160.0, radial_gap: 22.0, radial_step: 12.0, path_parent_gap: 40.0 }
    }

    pub fn compact() -> Self {
        Self { column_gap: 44.0, row_gap: 18.0, root_radius: 180.0, fan_angle_step: 24.0, fan_max_angle: 160.0, radial_gap: 16.0, radial_step: 10.0, path_parent_gap: 32.0 }
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
    pub kind: GraphLayoutKind,
    pub nodes: Vec<PlacedNode>,
    pub edges: Arc<Vec<GraphEdge>>,
    pub path: Vec<usize>,
    node_index: Vec<Option<usize>>,
}

impl GraphLayout {
    pub fn node(&self, node: usize) -> Option<&PlacedNode> {
        self.node_index.get(node).copied().flatten().map(|index| &self.nodes[index])
    }

    pub fn retain_nodes(&mut self, mut keep: impl FnMut(&PlacedNode) -> bool) {
        self.nodes.retain(|node| keep(node));
        self.node_index.fill(None);
        for (index, node) in self.nodes.iter().enumerate() {
            self.node_index[node.node] = Some(index);
        }
    }

    /// A display pose of this layout, preserving the target's node identities
    /// while moving nodes and their connectors to the supplied centres.
    pub fn with_pose(&self, centers: &[Point<f32>], opacity: &[f32]) -> Self {
        let mut posed = self.clone();
        for node in &mut posed.nodes {
            if let Some(&center) = centers.get(node.node) {
                let delta = point(center.x - node.center.x, center.y - node.center.y);
                node.center = center;
                node.bounds = shifted(node.bounds, delta);
            }
            node.opacity = opacity.get(node.node).copied().unwrap_or(1.0);
        }
        let nodes = &posed.nodes;
        let node_index = &posed.node_index;
        for edge in Arc::make_mut(&mut posed.edges) {
            let from = &nodes[node_index[edge.from].expect("edge parent is placed")];
            let to = &nodes[node_index[edge.to].expect("edge child is placed")];
            edge.opacity = to.opacity;
            if self.kind == GraphLayoutKind::AdaptiveRadial {
                let (dx, dy) = (to.center.x - from.center.x, to.center.y - from.center.y);
                let a = ray_box_fraction(from.bounds, dx, dy);
                let b = ray_box_fraction(to.bounds, dx, dy);
                if a + b >= 1.0 {
                    edge.opacity = 0.0;
                }
                edge.start = point(from.center.x + dx * a, from.center.y + dy * a);
                edge.end = point(to.center.x - dx * b, to.center.y - dy * b);
            } else {
                let old_from = self.node(edge.from).expect("edge parent is placed");
                let old_to = self.node(edge.to).expect("edge child is placed");
                edge.start = point(edge.start.x + from.center.x - old_from.center.x, edge.start.y + from.center.y - old_from.center.y);
                edge.end = point(edge.end.x + to.center.x - old_to.center.x, edge.end.y + to.center.y - old_to.center.y);
            }
        }
        posed
    }
}

/// Lays out the root, the path to `selection`, and the children of each, with
/// each node the size `size` gives it in its role.
pub fn graph_layout(kind: GraphLayoutKind, tree: &GraphTree, selection: usize, size: &dyn Fn(usize, NodeRole) -> Size<f32>, params: &LayoutParams) -> GraphLayout {
    let path = tree.ancestry(selection);
    let mut builder = Builder { tree, size, params, selection, path: &path, nodes: Vec::new(), node_index: vec![None; tree.parents.len()], edges: Vec::new() };
    builder.place(0, NodeRole::Root, None, point(0.0, 0.0), 0.0);
    match kind {
        GraphLayoutKind::Columns => builder.columns(0, 0.0, 0.0),
        GraphLayoutKind::AdaptiveRadial => {
            builder.radial(0);
            builder.radial_edges();
        }
    }
    let Builder { nodes, edges, node_index, .. } = builder;
    GraphLayout { kind, nodes, edges: Arc::new(edges), path, node_index }
}

struct RadialChild {
    node: usize,
    role: NodeRole,
    center: Point<f32>,
    angle: f32,
    bounds: Bounds<f32>,
}

struct Builder<'a> {
    tree: &'a GraphTree,
    size: &'a dyn Fn(usize, NodeRole) -> Size<f32>,
    params: &'a LayoutParams,
    selection: usize,
    path: &'a [usize],
    nodes: Vec<PlacedNode>,
    node_index: Vec<Option<usize>>,
    edges: Vec<GraphEdge>,
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

    fn placed(&self, node: usize) -> &PlacedNode {
        &self.nodes[self.node_index[node].expect("parent is placed before children")]
    }

    /// Use the nearest clear point on each spoke, then give every open
    /// non-root node with children more room along its incoming edge.
    fn radial(&mut self, node: usize) {
        let children = self.tree.children[node].clone();
        if children.is_empty() {
            return;
        }
        let parent = self.placed(node).clone();
        let root = node == 0;
        let path_fan = !root && self.path.contains(&node);
        let count = children.len();
        let span = if root { std::f32::consts::TAU } else { ((count - 1) as f32 * self.params.fan_angle_step).min(self.params.fan_max_angle).to_radians() };
        let start = if root { -PI * 0.62 } else { parent.center.y.atan2(parent.center.x) - span / 2.0 };
        let mut placed = Vec::with_capacity(count);
        for (index, &child) in children.iter().enumerate() {
            let role = self.role(child, node);
            let angle = start
                + if root {
                    span * index as f32 / count as f32
                } else if count == 1 {
                    0.0
                } else {
                    span * index as f32 / (count - 1) as f32
                };
            let candidate = |radius: f32| {
                let center = point(parent.center.x + radius * angle.cos(), parent.center.y + radius * angle.sin());
                (center, self.bounds(child, role, center))
            };
            let clear = |radius: f32, placed: &[RadialChild]| {
                let (_, bounds) = candidate(radius);
                self.nodes.iter().all(|other| path_fan && other.node != node || !overlaps(bounds, other.bounds, self.params.radial_gap))
                    && placed.iter().all(|other| !overlaps(bounds, other.bounds, self.params.radial_gap))
            };
            let mut radius = if root { self.params.root_radius } else { 1.0 };
            let mut last_blocked = radius;
            while !clear(radius, &placed) {
                last_blocked = radius;
                radius += self.params.radial_step;
            }
            if radius > last_blocked {
                let (mut lo, mut hi) = (last_blocked, radius);
                for _ in 0..10 {
                    let mid = (lo + hi) / 2.0;
                    if clear(mid, &placed) {
                        hi = mid;
                    } else {
                        lo = mid;
                    }
                }
                radius = hi;
            }
            let (center, bounds) = candidate(radius);
            placed.push(RadialChild { node: child, role, center, angle, bounds });
        }

        if path_fan {
            self.lengthen_path_node(node, &mut placed);
        }
        for child in placed {
            self.place(child.node, child.role, Some(node), child.center, child.angle);
        }
        for child in children {
            if self.path.contains(&child) {
                self.radial(child);
            }
        }
    }

    fn lengthen_path_node(&mut self, node: usize, children: &mut [RadialChild]) {
        let index = self.node_index[node].expect("path node is placed");
        let original = self.nodes[index].clone();
        let parent = self.placed(original.parent.expect("non-root path node has parent")).center;
        let angle = (original.center.y - parent.y).atan2(original.center.x - parent.x);
        let (ux, uy) = (angle.cos(), angle.sin());
        let outside: Vec<Bounds<f32>> = self.nodes.iter().filter(|other| other.node != node).map(|other| other.bounds).collect();
        let clear = |shift: f32| {
            let delta = point(shift * ux, shift * uy);
            let parent_box = shifted(original.bounds, delta);
            !outside.iter().any(|&other| overlaps(parent_box, other, self.params.radial_gap))
                && children.iter().all(|child| {
                    let box_at = shifted(child.bounds, delta);
                    !outside.iter().any(|&other| overlaps(box_at, other, self.params.radial_gap))
                })
        };
        let mut shift = self.params.path_parent_gap;
        let mut last_blocked = shift;
        while !clear(shift) {
            last_blocked = shift;
            shift += self.params.radial_step;
        }
        if shift > last_blocked {
            let (mut lo, mut hi) = (last_blocked, shift);
            for _ in 0..10 {
                let mid = (lo + hi) / 2.0;
                if clear(mid) {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            shift = hi;
        }
        let delta = point(shift * ux, shift * uy);
        self.nodes[index].center = point(original.center.x + delta.x, original.center.y + delta.y);
        self.nodes[index].bounds = shifted(original.bounds, delta);
        for child in children {
            child.center = point(child.center.x + delta.x, child.center.y + delta.y);
            child.bounds = shifted(child.bounds, delta);
        }
    }

    fn radial_edges(&mut self) {
        for child in self.nodes.iter().filter(|node| node.parent.is_some()) {
            let parent = self.placed(child.parent.expect("child has parent"));
            let (dx, dy) = (child.center.x - parent.center.x, child.center.y - parent.center.y);
            let from = ray_box_fraction(parent.bounds, dx, dy);
            let to = ray_box_fraction(child.bounds, dx, dy);
            self.edges.push(GraphEdge {
                from: parent.node,
                to: child.node,
                start: point(parent.center.x + dx * from, parent.center.y + dy * from),
                end: point(child.center.x - dx * to, child.center.y - dy * to),
                on_path: self.path.contains(&parent.node) && self.path.contains(&child.node),
                opacity: 1.0,
            });
        }
    }

    /// `side` is zero at the root, then +1 or -1. `outer` is the far edge of
    /// the parent's column.
    fn columns(&mut self, node: usize, side: f32, outer: f32) {
        let children = self.tree.children[node].clone();
        if children.is_empty() {
            return;
        }
        let parent = self.placed(node);
        let (parent_center, parent_width) = (parent.center, parent.bounds.size.width);
        // Each child's slot fits it in any role it can take below a placed parent.
        let slots: Vec<Size<f32>> = children
            .iter()
            .map(|&kid| {
                [NodeRole::Selection, NodeRole::Path, NodeRole::Fan, NodeRole::Context]
                    .into_iter()
                    .map(|role| (self.size)(kid, role))
                    .fold(Size { width: 0.0_f32, height: 0.0_f32 }, |slot, size| Size { width: slot.width.max(size.width), height: slot.height.max(size.height) })
            })
            .collect();

        // The root's children split into the two columns with the closest heights.
        let split = if side == 0.0 {
            let total: f32 = slots.iter().map(|slot| slot.height + self.params.row_gap).sum();
            let mut sum = 0.0;
            let mut best = (f32::INFINITY, 0);
            for (i, slot) in slots.iter().enumerate() {
                let diff = (total - 2.0 * sum).abs();
                if diff < best.0 {
                    best = (diff, i);
                }
                sum += slot.height + self.params.row_gap;
            }
            if (total - 2.0 * sum).abs() < best.0 { children.len() } else { best.1 }
        } else {
            children.len()
        };

        let groups = if side == 0.0 { [(0, split, 1.0), (split, children.len(), -1.0)] } else { [(0, children.len(), side), (0, 0, side)] };
        for (start, end, direction) in groups {
            if start == end {
                continue;
            }
            let (group, group_slots) = (&children[start..end], &slots[start..end]);
            let column_height: f32 = group_slots.iter().map(|slot| slot.height).sum::<f32>() + self.params.row_gap * (group.len() - 1) as f32;
            let parent_outer = if side == 0.0 { parent_center.x + direction * parent_width / 2.0 } else { outer };
            let edge = parent_outer + direction * self.params.column_gap;
            let column_outer = edge + direction * group_slots.iter().map(|slot| slot.width).fold(0.0, f32::max);
            let mut y = parent_center.y - column_height / 2.0;
            for (&kid, slot) in group.iter().zip(group_slots) {
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
            for &kid in group {
                if self.path.contains(&kid) {
                    self.columns(kid, direction, column_outer);
                }
            }
        }
    }
}

fn shifted(bounds: Bounds<f32>, delta: Point<f32>) -> Bounds<f32> {
    Bounds::new(point(bounds.origin.x + delta.x, bounds.origin.y + delta.y), bounds.size)
}

fn overlaps(a: Bounds<f32>, b: Bounds<f32>, gap: f32) -> bool {
    a.origin.x < b.origin.x + b.size.width + gap && b.origin.x < a.origin.x + a.size.width + gap && a.origin.y < b.origin.y + b.size.height + gap && b.origin.y < a.origin.y + a.size.height + gap
}

fn ray_box_fraction(bounds: Bounds<f32>, dx: f32, dy: f32) -> f32 {
    let fx = if dx.abs() > f32::EPSILON { bounds.size.width / (2.0 * dx.abs()) } else { f32::INFINITY };
    let fy = if dy.abs() > f32::EPSILON { bounds.size.height / (2.0 * dy.abs()) } else { f32::INFINITY };
    fx.min(fy).min(0.5)
}
