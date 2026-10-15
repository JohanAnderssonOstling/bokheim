use std::ops::Range;

use super::{TagData, TagKind, TagNumbering};
use super::super::{SegmentTree, TokenUnitId, WhitespaceTypographyKind};

#[derive(Clone, Copy, Debug)]
pub struct SegmentTag<'a> {
    tree: &'a SegmentTree,
    unit_index: usize,
    tag_index: usize,
    local_id: usize,
    occurrence: usize,
}

impl<'a> SegmentTag<'a> {
    fn new(tree: &'a SegmentTree, unit_index: usize, tag_index: usize, local_id: usize, occurrence: usize) -> Self { Self { tree, unit_index, tag_index, local_id, occurrence } }
    pub fn data(&self) -> &'a TagData { &self.tree.tags[self.tag_index] }
    pub fn id(&self) -> &'a str { self.data().id() }
    pub fn kind(&self) -> TagKind { self.data().kind() }
    pub fn span(&self) -> Range<usize> { self.tree.units[self.unit_index].span() }
    pub fn token_unit_id(&self) -> TokenUnitId { self.tree.units[self.unit_index].token_id().expect("tree units must have stable token ids") }
    pub fn occurrence(&self) -> usize { self.occurrence }
    pub fn whitespace_before(&self) -> WhitespaceTypographyKind { self.unit_index.checked_sub(1).and_then(|index| self.tree.unit(index)).and_then(|unit| unit.text().chars().next_back()).map(classify_tag_whitespace).unwrap_or(WhitespaceTypographyKind::Absent) }
    pub fn whitespace_after(&self) -> WhitespaceTypographyKind { self.tree.unit(self.unit_index + 1).and_then(|unit| unit.text().chars().next()).map(classify_tag_whitespace).unwrap_or(WhitespaceTypographyKind::Absent) }
    pub fn label(&self) -> String {
        match self.kind() {
            TagKind::Standalone => format!("<{}/>", self.local_id),
            TagKind::Open => format!("<{}>", self.local_id),
            TagKind::Close => format!("</{}>", self.local_id),
        }
    }
}

fn classify_tag_whitespace(character: char) -> WhitespaceTypographyKind {
    match character {
        '\u{00A0}' => WhitespaceTypographyKind::NonBreaking,
        '\u{202F}' => WhitespaceTypographyKind::NarrowNonBreaking,
        character if character.is_whitespace() => WhitespaceTypographyKind::Regular,
        _ => WhitespaceTypographyKind::Absent,
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SegmentTags<'a> {
    tree: &'a SegmentTree,
}

impl<'a> SegmentTags<'a> {
    fn new(tree: &'a SegmentTree) -> Self { Self { tree } }
    pub fn len(&self) -> usize { self.tree.tags.len() }
    pub fn is_empty(&self) -> bool { self.tree.tags.is_empty() }
    pub fn iter(&self) -> impl Iterator<Item = SegmentTag<'a>> + Clone + '_ {
        let mut numbering = TagNumbering::default();
        self.tree.units.iter().enumerate().filter_map(move |(unit_index, unit)| unit.tag_index().map(|tag_index| { let (local_id, occurrence) = numbering.next(self.tree.tags[tag_index].id()); SegmentTag::new(self.tree, unit_index, tag_index, local_id, occurrence) })).take(self.tree.tags.len())
    }
    pub fn find(&self, id: &str, occurrence: usize) -> Option<SegmentTag<'a>> { self.iter().find(|tag| tag.id() == id && tag.occurrence() == occurrence) }
}

impl SegmentTree {
    pub fn segment_tags(&self) -> SegmentTags<'_> { SegmentTags::new(self) }
}
