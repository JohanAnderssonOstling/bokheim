use std::collections::HashMap;
use std::ops::Range;

use structure::{FormattingKind, SegmentTag, SegmentTree, TagKind, TagScope, TokenUnitId, WhitespaceTypographyKind};

use crate::compact_span::{compact_offset, CompactSpan};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TagScopeKind {
    Paired,
    Standalone,
    UnclosedOpen,
    UnexpectedClose,
    Misnested,
}

impl TagScopeKind {
    pub fn is_valid(self) -> bool { !self.is_invalid() }
    pub fn is_invalid(self) -> bool { matches!(self, Self::UnclosedOpen | Self::UnexpectedClose | Self::Misnested) }
    pub fn is_container(self) -> bool { matches!(self, Self::Paired | Self::UnclosedOpen | Self::Misnested) }
}

#[derive(Clone, Copy, Debug)]
pub struct SegmentTagScopeFactRef<'a> {
    kind: TagScopeKind,
    occurrence: u16,
    nesting_depth: u16,
    first: SegmentTag<'a>,
    second: Option<SegmentTag<'a>>,
    content: CompactSpan,
}

impl<'a> SegmentTagScopeFactRef<'a> {
    fn paired(open: SegmentTag<'a>, close: SegmentTag<'a>, occurrence: usize, nesting_depth: usize, misnested: bool) -> Self { Self::new(if misnested { TagScopeKind::Misnested } else { TagScopeKind::Paired }, occurrence, nesting_depth, open, Some(close), open.span().end..close.span().start) }
    fn standalone(tag: SegmentTag<'a>, occurrence: usize, nesting_depth: usize) -> Self { let position = tag.span().start; Self::new(TagScopeKind::Standalone, occurrence, nesting_depth, tag, None, position..position) }
    fn unclosed(tree: &'a SegmentTree, open: SegmentTag<'a>, occurrence: usize, nesting_depth: usize) -> Self { Self::new(TagScopeKind::UnclosedOpen, occurrence, nesting_depth, open, None, open.span().end..tree.text().len()) }
    fn unexpected_close(close: SegmentTag<'a>, occurrence: usize, nesting_depth: usize) -> Self { let position = close.span().start; Self::new(TagScopeKind::UnexpectedClose, occurrence, nesting_depth, close, None, position..position) }
    fn new(kind: TagScopeKind, occurrence: usize, nesting_depth: usize, first: SegmentTag<'a>, second: Option<SegmentTag<'a>>, content_span: Range<usize>) -> Self { Self { kind, occurrence: compact_offset(occurrence, "tag scope occurrence"), nesting_depth: compact_offset(nesting_depth, "tag scope nesting depth"), first, second, content: CompactSpan::new(content_span, "tag scope content") } }
    pub fn kind(&self) -> TagScopeKind { self.kind }
    pub fn id(&self) -> &'a str { self.first.id() }
    pub fn occurrence(&self) -> usize { self.occurrence as usize }
    pub fn nesting_depth(&self) -> usize { self.nesting_depth as usize }
    pub fn span(&self) -> Range<usize> { self.first.span().start..self.second.map(|tag| tag.span().end).unwrap_or(self.content.range().end) }
    pub fn open_span(&self) -> Option<Range<usize>> { self.open_tag().map(|tag| tag.span()) }
    pub fn close_span(&self) -> Option<Range<usize>> { self.close_tag().map(|tag| tag.span()) }
    pub fn content_span(&self) -> Range<usize> { self.content.range() }
    pub fn first_tag(&self) -> SegmentTag<'a> { self.first }
    pub fn open_tag(&self) -> Option<SegmentTag<'a>> { matches!(self.kind, TagScopeKind::Paired | TagScopeKind::UnclosedOpen | TagScopeKind::Misnested).then_some(self.first) }
    pub fn close_tag(&self) -> Option<SegmentTag<'a>> { matches!(self.kind, TagScopeKind::Paired | TagScopeKind::Misnested).then(|| self.second).flatten().or_else(|| (self.kind == TagScopeKind::UnexpectedClose).then_some(self.first)) }
    pub fn tags(&self) -> Vec<SegmentTag<'a>> { std::iter::once(self.first).chain(self.second).collect() }
    pub fn token_unit_ids(&self) -> Vec<TokenUnitId> { self.tags().into_iter().map(|tag| tag.token_unit_id()).collect() }
    pub fn formatting(&self) -> &'a [FormattingKind] { self.open_tag().and_then(|tag| tag.data().metadata()).map(|metadata| metadata.formatting()).unwrap_or_default() }
    pub fn has_formatting(&self, kind: FormattingKind) -> bool { self.formatting().contains(&kind) }
    pub fn whitespace_before(&self) -> WhitespaceTypographyKind { self.first.whitespace_before() }
    pub fn whitespace_after(&self) -> WhitespaceTypographyKind { self.second.unwrap_or(self.first).whitespace_after() }
    fn primary_order(&self) -> usize { self.first.span().start }
}

#[derive(Clone, Copy)]
struct PendingTagScope<'a> {
    tag: SegmentTag<'a>,
    occurrence: usize,
    nesting_depth: usize,
}

fn next_tag_scope_occurrence<'a>(counters: &mut HashMap<&'a str, usize>, id: &'a str) -> usize { let counter = counters.entry(id).or_default(); let occurrence = *counter; *counter += 1; occurrence }

pub fn tag_scope_facts(tree: &SegmentTree) -> Vec<SegmentTagScopeFactRef<'_>> {
    let mut counters = HashMap::<&str, usize>::new();
    let mut open = Vec::<PendingTagScope<'_>>::new();
    let mut facts = Vec::new();
    tree.segment_tags().iter().for_each(|tag| match tag.kind() {
        TagKind::Open => {
            let occurrence = next_tag_scope_occurrence(&mut counters, tag.id());
            open.push(PendingTagScope { tag, occurrence, nesting_depth: open.len() });
        }
        TagKind::Standalone => {
            let occurrence = next_tag_scope_occurrence(&mut counters, tag.id());
            facts.push(SegmentTagScopeFactRef::standalone(tag, occurrence, open.len()));
        }
        TagKind::Close => {
            let Some(position) = open.iter().rposition(|candidate| candidate.tag.id() == tag.id()) else {
                let occurrence = next_tag_scope_occurrence(&mut counters, tag.id());
                facts.push(SegmentTagScopeFactRef::unexpected_close(tag, occurrence, open.len()));
                return;
            };
            let pending = open.remove(position);
            facts.push(SegmentTagScopeFactRef::paired(pending.tag, tag, pending.occurrence, pending.nesting_depth, position != open.len()));
        }
    });
    facts.extend(open.into_iter().map(|pending| SegmentTagScopeFactRef::unclosed(tree, pending.tag, pending.occurrence, pending.nesting_depth)));
    facts.sort_by_key(SegmentTagScopeFactRef::primary_order);
    facts
}

pub fn tag_scopes_covering_span(tree: &SegmentTree, span: Range<usize>) -> Vec<SegmentTagScopeFactRef<'_>> {
    let mut scopes = tag_scope_facts(tree).into_iter().filter(|scope| !span.is_empty() && scope.kind().is_container() && scope.content_span().start <= span.start && span.end <= scope.content_span().end).collect::<Vec<_>>();
    scopes.sort_by_key(SegmentTagScopeFactRef::nesting_depth);
    scopes
}

pub fn common_tag_scope(tree: &SegmentTree, span: Range<usize>) -> TagScope { TagScope::new(tag_scopes_covering_span(tree, span).into_iter().map(|scope| scope.id().to_string()).collect()) }

#[cfg(test)]
mod tests {
    use super::*;
    use structure::WhitespaceTypographyKind;

    #[test]
    fn tag_scope_facts_expose_pairs_depth_content_formatting_and_standalone_tags() {
        let mut metadata = structure::TagMetadata::default();
        metadata.push_formatting(structure::FormattingKind::Bold);
        let metadata = std::sync::Arc::new(metadata);
        let mut run = structure::Run::new("A B C");
        run.push_tag_at(0, structure::TagData::new("outer".to_string(), "g".to_string(), TagKind::Open, "<g>".to_string(), None, Some(metadata.clone())));
        run.push_tag_at(1, structure::TagData::new("point".to_string(), "x".to_string(), TagKind::Standalone, "<x/>".to_string(), None, None));
        run.push_tag_at(2, structure::TagData::new("inner".to_string(), "g".to_string(), TagKind::Open, "<g>".to_string(), None, None));
        run.push_tag_at(3, structure::TagData::new("inner".to_string(), "g".to_string(), TagKind::Close, "</g>".to_string(), None, None));
        run.push_tag_at(5, structure::TagData::new("outer".to_string(), "g".to_string(), TagKind::Close, "</g>".to_string(), None, Some(metadata)));
        let tree = structure::TokenizedSegment::from_runs([run]).into_tree_for_parser();
        let facts = tag_scope_facts(&tree);
        let outer = facts.iter().find(|scope| scope.id() == "outer").expect("outer scope");
        let inner = facts.iter().find(|scope| scope.id() == "inner").expect("inner scope");
        let point = facts.iter().find(|scope| scope.id() == "point").expect("standalone scope");

        assert_eq!((outer.kind(), outer.nesting_depth(), outer.content_span(), &tree.text()[outer.content_span()]), (TagScopeKind::Paired, 0, 0..5, "A B C"));
        assert!(outer.has_formatting(structure::FormattingKind::Bold));
        assert_eq!((inner.kind(), inner.nesting_depth(), inner.content_span(), &tree.text()[inner.content_span()]), (TagScopeKind::Paired, 1, 2..3, "B"));
        assert_eq!(inner.whitespace_before(), WhitespaceTypographyKind::Regular);
        assert_eq!((point.kind(), point.nesting_depth(), point.content_span()), (TagScopeKind::Standalone, 1, 1..1));
        assert_eq!(common_tag_scope(&tree, 2..3).tag_ids(), &["outer".to_string(), "inner".to_string()]);
        assert_eq!(common_tag_scope(&tree, 0..5).tag_ids(), &["outer".to_string()]);
    }

    #[test]
    fn tag_scope_facts_preserve_misnested_unexpected_and_unclosed_states() {
        let mut crossed = structure::Run::new("Text");
        crossed.push_tag_at(0, structure::TagData::new("1".to_string(), "g".to_string(), TagKind::Open, "<g>".to_string(), None, None));
        crossed.push_tag_at(0, structure::TagData::new("2".to_string(), "g".to_string(), TagKind::Open, "<g>".to_string(), None, None));
        crossed.push_tag_at(4, structure::TagData::new("1".to_string(), "g".to_string(), TagKind::Close, "</g>".to_string(), None, None));
        crossed.push_tag_at(4, structure::TagData::new("2".to_string(), "g".to_string(), TagKind::Close, "</g>".to_string(), None, None));
        let crossed = structure::TokenizedSegment::from_runs([crossed]).into_tree_for_parser();
        let crossed_facts = tag_scope_facts(&crossed);

        assert_eq!(crossed_facts.iter().find(|scope| scope.id() == "1").map(SegmentTagScopeFactRef::kind), Some(TagScopeKind::Misnested));
        assert_eq!(crossed_facts.iter().find(|scope| scope.id() == "2").map(SegmentTagScopeFactRef::kind), Some(TagScopeKind::Paired));

        let mut incomplete = structure::Run::new("Text");
        incomplete.push_tag_at(0, structure::TagData::new("close".to_string(), "g".to_string(), TagKind::Close, "</g>".to_string(), None, None));
        incomplete.push_tag_at(0, structure::TagData::new("open".to_string(), "g".to_string(), TagKind::Open, "<g>".to_string(), None, None));
        let incomplete = structure::TokenizedSegment::from_runs([incomplete]).into_tree_for_parser();
        let kinds = tag_scope_facts(&incomplete).into_iter().map(|scope| (scope.id().to_string(), scope.kind())).collect::<Vec<_>>();

        assert_eq!(kinds, vec![("close".to_string(), TagScopeKind::UnexpectedClose), ("open".to_string(), TagScopeKind::UnclosedOpen)]);
    }
}
