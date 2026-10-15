//! Internal construction bridge used by `segment-parser`.
//!
//! Rust has no friend-crate visibility, so this module is public for the parser
//! crate but hidden from the supported `structure` API. Other consumers should
//! use read-only `SegmentTree` views or parser-derived facts from
//! `segment-parser`.

use super::{NodeRef, SegmentTree, TagData, WhitespaceTypographyFact};

#[path = "parser_api/model.rs"]
mod model;
pub use model::{Node, NodeAnnotation, NodeSemantic, TokenUnit};

pub fn units(tree: &SegmentTree) -> &[TokenUnit] { &tree.units }
pub fn unit_start(tree: &SegmentTree, index: usize) -> Option<usize> { if index == tree.units.len() { Some(tree.text.len()) } else { tree.units.get(index).map(TokenUnit::start) } }
pub fn unit_end(tree: &SegmentTree, index: usize) -> Option<usize> { tree.units.get(index).map(TokenUnit::end) }
pub fn unit_span(tree: &SegmentTree, index: usize) -> Option<std::ops::Range<usize>> { tree.units.get(index).map(TokenUnit::span) }
pub fn unit_text(tree: &SegmentTree, index: usize) -> Option<&str> { unit_span(tree, index).and_then(|span| tree.text.get(span)) }
pub fn root_node(tree: &SegmentTree) -> Node { tree.loaded_root() }
pub fn replace_root_node(tree: &mut SegmentTree, root: Node) { tree.set_root(root); }
pub fn apply_parse_output(tree: &mut SegmentTree, nodes: Vec<Node>) { tree.apply_parse_output_internal(nodes); }
pub fn stored_whitespace_typography_facts(tree: &SegmentTree) -> impl Iterator<Item = WhitespaceTypographyFact> + '_ { tree.stored_whitespace_typography_facts_internal() }
pub fn node_annotations(node: NodeRef<'_>) -> &[NodeAnnotation] { node.annotations() }
pub fn node_semantic(node: NodeRef<'_>) -> Option<&NodeSemantic> { node.semantic() }
pub fn from_tokenized_parts(text: String, units: Vec<TokenUnit>, tags: Vec<TagData>) -> SegmentTree { SegmentTree::from_tokenized_parts_with_semantic_ids(text, units, tags, Vec::new(), None) }

#[derive(Clone, Debug)]
struct NumericNodes<'a> {
    tree: &'a SegmentTree,
    indexed_position: usize,
}

impl<'a> NumericNodes<'a> {
    fn new(tree: &'a SegmentTree) -> Self { Self { tree, indexed_position: 0 } }
}

impl<'a> Iterator for NumericNodes<'a> {
    type Item = NodeRef<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let address = *self.tree.numeric_fact_addresses.get(self.indexed_position)?;
        self.indexed_position += 1;
        Some(NodeRef::new(self.tree, address))
    }
}

pub fn numeric_nodes(tree: &SegmentTree) -> impl Iterator<Item = NodeRef<'_>> + Clone { NumericNodes::new(tree) }
