use structure::{SegmentTree, VersionPrefixStyle, VersionSemantic};

use crate::node_facts::{NodeFactRef, VersionFactKind};

pub type SegmentVersionFactRef<'a> = NodeFactRef<'a, VersionFactKind>;

impl<'a> NodeFactRef<'a, VersionFactKind> {
    fn semantic(&self) -> &'a VersionSemantic { self.node().version_semantic().expect("typed version fact must retain its semantics") }
    pub fn prefix_style(&self) -> VersionPrefixStyle { self.semantic().prefix_style() }
    pub fn components(&self) -> &'a [String] { self.semantic().components() }
    pub fn prerelease(&self) -> Option<&'a str> { self.semantic().prerelease() }
    pub fn build_metadata(&self) -> Option<&'a str> { self.semantic().build_metadata() }
    pub fn canonical(&self) -> &'a str { self.semantic().canonical() }
}

pub fn version_facts(tree: &SegmentTree) -> impl Iterator<Item = SegmentVersionFactRef<'_>> + Clone { tree.preorder_iter().filter_map(NodeFactRef::new) }
