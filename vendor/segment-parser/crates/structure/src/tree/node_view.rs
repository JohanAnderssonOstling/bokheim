use std::ops::Range;

use super::{NodeAnnotation, NodeKind, NodeSemantic, PartOfSpeech, PlaceholderSemantic, ProtectedKind, SegmentTree, StoredNode, TokenUnitId, UnitKind, UnitRef, VersionSemantic};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NodeRef<'a> {
    tree: &'a SegmentTree,
    node_address: u16,
}

impl<'a> NodeRef<'a> {
    pub(super) fn new(tree: &'a SegmentTree, node_address: u16) -> Self { Self { tree, node_address } }
    pub fn tree(&self) -> &'a SegmentTree { self.tree }
    fn stored(&self) -> Option<&'a StoredNode> { self.tree.nodes.get(self.node_address as usize) }
    fn unit(&self) -> Option<UnitRef<'a>> { self.tree.virtual_unit_index(self.node_address).and_then(|index| self.tree.unit(index)) }
    pub fn kind(&self) -> NodeKind { self.stored().map(|stored| stored.kind).or_else(|| self.unit().map(|unit| unit.kind().node_kind())).expect("node address must be valid") }
    pub fn protected_kind(&self) -> Option<ProtectedKind> { self.kind().protected_kind() }
    pub fn is_protected(&self) -> bool { self.kind().is_protected() }
    pub fn text(&self) -> &'a str { self.stored().and_then(|stored| self.tree.text.get(stored.span())).or_else(|| self.unit().map(|unit| unit.text())).unwrap_or("") }
    pub fn tag(&self) -> Option<&'a super::TagData> { self.stored().and_then(StoredNode::tag_index).and_then(|index| self.tree.tags.get(index)).or_else(|| self.unit().and_then(|unit| unit.tag())) }
    pub(crate) fn annotations(&self) -> &'a [NodeAnnotation] { self.stored().and_then(StoredNode::annotations_index).and_then(|index| self.tree.node_annotations.get(index)).map(Box::as_ref).unwrap_or(&[]) }
    pub(in crate::tree) fn annotations_index(&self) -> Option<usize> { self.stored().and_then(StoredNode::annotations_index) }
    pub(crate) fn semantic(&self) -> Option<&'a NodeSemantic> { let semantic = self.stored().and_then(StoredNode::semantic_index).and_then(|index| self.tree.node_semantics.get(index)); assert!(semantic.map(NodeSemantic::node_kind).is_none_or(|semantic_kind| semantic_kind == self.kind()), "stored node kind must agree with semantic payload"); semantic }
    pub(in crate::tree) fn has_annotations(&self) -> bool { !self.annotations().is_empty() }
    pub fn dictionary_known(&self) -> bool { self.annotations().iter().any(NodeAnnotation::is_dictionary_known) }
    pub fn dictionary_unknown(&self) -> bool { self.annotations().iter().any(NodeAnnotation::is_dictionary_unknown) }
    pub fn dictionary_baseform(&self) -> Option<&'a str> { self.annotations().iter().find_map(NodeAnnotation::dictionary_baseform) }
    pub fn dictionary_analysis(&self) -> Option<&'a super::DictionaryAnalysis> { self.annotations().iter().find_map(NodeAnnotation::dictionary_analysis) }
    pub fn placeholder_semantic(&self) -> Option<&'a PlaceholderSemantic> { self.semantic().and_then(NodeSemantic::placeholder) }
    pub fn version_semantic(&self) -> Option<&'a VersionSemantic> { self.semantic().and_then(NodeSemantic::version) }
    pub fn part_of_speech(&self) -> Option<PartOfSpeech> {
        (self.kind() == NodeKind::Word).then(|| self.tree.iter_units().find(|unit| unit.kind() == UnitKind::Word && unit.span() == self.span()).and_then(|unit| self.tree.part_of_speech_for_word_id(unit.id()))).flatten()
    }
    pub fn iter_children(&self) -> impl DoubleEndedIterator<Item = NodeRef<'a>> + '_ { self.tree.node_child_addresses(self.node_address).iter().copied().map(move |address| Self::new(self.tree, address)) }
    pub fn child(&self, index: usize) -> Option<NodeRef<'a>> { self.tree.node_child_addresses(self.node_address).get(index).copied().map(|address| Self::new(self.tree, address)) }
    pub fn is_leaf(&self) -> bool { self.stored().is_none_or(|stored| stored.child_count == 0) }
    pub fn len(&self) -> usize { self.stored().map(StoredNode::len).or_else(|| self.unit().map(|unit| unit.len())).unwrap_or(0) }
    pub fn is_empty(&self) -> bool { self.len() == 0 }
    pub fn span(&self) -> Range<usize> { self.stored().map(StoredNode::span).or_else(|| self.unit().map(|unit| unit.span())).unwrap_or(0..0) }
    pub fn token_unit_ids(&self) -> Vec<TokenUnitId> { self.tree.token_unit_ids_for_span(self.span()) }
}

#[derive(Clone)]
pub(super) struct PreorderIter<'a> {
    tree: &'a SegmentTree,
    stack: Vec<u16>,
}

impl<'a> PreorderIter<'a> {
    pub(super) fn new(tree: &'a SegmentTree) -> Self { Self { tree, stack: vec![0] } }
}

impl<'a> Iterator for PreorderIter<'a> {
    type Item = NodeRef<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let node_address = self.stack.pop()?;
        self.stack.extend(self.tree.node_child_addresses(node_address).iter().rev().copied());
        Some(NodeRef::new(self.tree, node_address))
    }
}
