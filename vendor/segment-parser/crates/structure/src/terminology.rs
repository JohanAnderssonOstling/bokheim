use std::ops::Range;

use crate::{TagScope, TokenUnitId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminologyOccurrence {
    span: Range<usize>,
    token_unit_ids: Vec<TokenUnitId>,
    tag_scope: TagScope,
}

impl TerminologyOccurrence {
    pub fn new(span: Range<usize>, token_unit_ids: Vec<TokenUnitId>, tag_scope: TagScope) -> Self { Self { span, token_unit_ids, tag_scope } }
    pub fn span(&self) -> &Range<usize> { &self.span }
    pub fn token_unit_ids(&self) -> &[TokenUnitId] { &self.token_unit_ids }
    pub fn tag_scope(&self) -> &TagScope { &self.tag_scope }
}
