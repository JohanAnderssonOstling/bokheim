use std::ops::Range;

use super::{DelimiterAnnotation, DelimiterFamily, DelimiterRole, QuotationStyle};
use super::super::{NodeAnnotation, NodeRef, SegmentTree, TokenUnitId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DelimiterFactRef<'a> {
    node: NodeRef<'a>,
    annotation: DelimiterAnnotation,
}

impl<'a> DelimiterFactRef<'a> {
    fn new(node: NodeRef<'a>, annotation: DelimiterAnnotation) -> Self { Self { node, annotation } }
    pub fn family(self) -> DelimiterFamily { self.annotation.family() }
    pub fn role(self) -> DelimiterRole { self.annotation.role() }
    pub fn preferred_quotation_style(self) -> Option<QuotationStyle> { self.annotation.preferred_quotation_style() }
    pub fn text(self) -> &'a str { self.node.text() }
    pub fn span(self) -> Range<usize> { self.node.span() }
    pub fn token_unit_ids(self) -> Vec<TokenUnitId> { self.node.token_unit_ids() }
}

impl SegmentTree {
    pub fn delimiter_facts(&self) -> impl Iterator<Item = DelimiterFactRef<'_>> + '_ {
        self.iter_nodes().flat_map(|node| node.annotations().iter().filter_map(move |annotation| match annotation {
            NodeAnnotation::Delimiter(annotation) => Some(DelimiterFactRef::new(node, *annotation)),
            _ => None,
        }))
    }
}
