use std::ops::Range;

use super::{SemanticUnitId, TokenUnitId};
use super::super::{SegmentTree, TagData, TokenUnit, UnitKind};

#[derive(Clone, Copy, Debug)]
pub struct UnitRef<'a> {
    tree: &'a SegmentTree,
    index: usize,
}

impl<'a> UnitRef<'a> {
    pub(in super::super) fn new(tree: &'a SegmentTree, index: usize) -> Self { Self { tree, index } }
    pub(in crate::tree) fn token(&self) -> &'a TokenUnit { &self.tree.units[self.index] }
    pub fn id(&self) -> TokenUnitId { self.token().token_id().expect("tree units must have stable token ids") }
    pub fn kind(&self) -> UnitKind { self.token().kind() }
    pub fn text(&self) -> &'a str { self.tree.text.get(self.token().span()).unwrap_or("") }
    pub fn tag(&self) -> Option<&'a TagData> { self.tree.unit_tag(self.index) }
    pub fn semantic_id(&self) -> Option<SemanticUnitId> { self.tree.unit_semantic_id(self.index) }
    pub fn inline_placeable_id(&self) -> Option<Option<&'a str>> { self.tree.inline_placeable_id(self.index) }
    pub fn is_tag(&self) -> bool { self.token().is_tag() }
    pub fn len(&self) -> usize { self.token().len() }
    pub fn span(&self) -> Range<usize> { self.token().span() }
}
