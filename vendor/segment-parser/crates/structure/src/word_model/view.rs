use std::ops::Range;

use super::{DictionaryAnalysis, DictionaryWordAnnotation, SegmentWordStorage, StoredSegmentWord};
use super::super::{PartOfSpeech, SegmentTree, TokenUnitId, UnitKind};

#[derive(Clone, Copy, Debug)]
pub struct SegmentWords<'a> {
    tree: &'a SegmentTree,
    storage: &'a SegmentWordStorage,
}

#[derive(Clone, Copy, Debug)]
pub struct SegmentWordRef<'a> {
    words: SegmentWords<'a>,
    word: &'a StoredSegmentWord,
}

impl<'a> SegmentWordRef<'a> {
    #[inline]
    fn new(words: SegmentWords<'a>, word: &'a StoredSegmentWord) -> Self { Self { words, word } }
    #[inline]
    pub fn text(self) -> &'a str { self.words.word_text(self.word).unwrap_or("") }
    #[inline]
    pub fn normalized(self) -> &'a str { self.word.normalized.as_deref().unwrap_or_else(|| self.text()) }
    #[inline]
    pub fn token_unit_id(self) -> TokenUnitId { self.word.token_unit_id }
    #[inline]
    pub fn span(self) -> Range<usize> { self.word.start as usize..self.word.end as usize }
    #[inline]
    pub fn has_at_least_alphabetic(self, minimum: u16) -> bool { self.word.attributes.alphabetic_count >= minimum }
    #[inline]
    pub fn is_capitalized(self) -> bool { self.word.attributes.capitalized }
    #[inline]
    pub fn whitespace_before(self) -> bool { self.word.whitespace_before }
    #[inline]
    pub fn dictionary_known(self) -> bool { self.words.storage.dictionary_known_words.contains(self.normalized()) }
    pub fn baseform(self) -> Option<&'a str> { self.dictionary_annotations()?.iter().find_map(super::super::NodeAnnotation::dictionary_baseform) }
    pub fn dictionary_analysis(self) -> Option<&'a DictionaryAnalysis> { self.dictionary_annotations()?.iter().find_map(super::super::NodeAnnotation::dictionary_analysis) }
    fn dictionary_annotations(self) -> Option<&'a [super::super::NodeAnnotation]> { let DictionaryWordAnnotation::Annotated(index) = self.words.storage.dictionary_known_words.annotation(self.normalized())? else { return None; }; self.words.tree.node_annotations.get(index as usize).map(|annotations| annotations.as_ref()) }
    pub fn part_of_speech(self) -> Option<PartOfSpeech> { self.words.tree.token_index(self.token_unit_id()).and_then(|index| self.words.tree.unit(index)).filter(|unit| unit.kind() == UnitKind::Word).and_then(|_| self.words.tree.part_of_speech_for_word_id(self.token_unit_id())) }
    pub(in crate) fn tree(self) -> &'a SegmentTree { self.words.tree }
}

impl<'a> SegmentWords<'a> {
    fn new(tree: &'a SegmentTree) -> Self { Self { tree, storage: &tree.segment_words } }
    fn word(&self, index: usize) -> Option<&'a StoredSegmentWord> { self.storage.words.get(index) }
    #[inline]
    fn word_text(&self, word: &StoredSegmentWord) -> Option<&'a str> { self.tree.text.get(word.start as usize..word.end as usize) }
    pub fn len(&self) -> usize { self.storage.words.len() }
    pub fn is_empty(&self) -> bool { self.storage.words.is_empty() }
    #[inline]
    pub fn iter(self) -> impl ExactSizeIterator<Item = SegmentWordRef<'a>> + DoubleEndedIterator + Clone + 'a { self.storage.words.iter().map(move |word| SegmentWordRef::new(self, word)) }
    pub fn get(self, index: usize) -> Option<SegmentWordRef<'a>> { self.word(index).map(|word| SegmentWordRef::new(self, word)) }
    pub fn has_dictionary_annotations(&self) -> bool { self.storage.has_dictionary_annotations }
    pub fn dictionary_knows(&self, normalized: &str) -> bool { self.storage.dictionary_known_words.contains(normalized) }

    #[cfg(test)]
    pub(in super::super) fn shares_storage_with(&self, other: Self) -> bool { std::ptr::eq(self.storage, other.storage) }
}

impl SegmentTree {
    pub fn segment_words(&self) -> SegmentWords<'_> { SegmentWords::new(self) }
}
