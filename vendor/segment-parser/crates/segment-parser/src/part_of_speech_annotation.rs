use std::collections::HashMap;
use std::ops::Range;
use structure::{PartOfSpeech, PosAnnotation, SegmentTree, UnitKind};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PosTaggedToken {
    span: Range<usize>,
    part_of_speech: PartOfSpeech,
}

impl PosTaggedToken {
    pub fn new(span: Range<usize>, part_of_speech: PartOfSpeech) -> Self { Self { span, part_of_speech } }
    pub fn span(&self) -> &Range<usize> { &self.span }
    pub fn part_of_speech(&self) -> PartOfSpeech { self.part_of_speech }
}

pub fn annotate_part_of_speech(tree: &mut SegmentTree, tokens: impl IntoIterator<Item = PosTaggedToken>) {
    let tags = tokens.into_iter().map(|token| (token.span, token.part_of_speech)).collect::<HashMap<_, _>>();
    let annotations = tree.segment_words().iter().filter_map(|word| {
        let unit = tree.token_index(word.token_unit_id()).and_then(|index| tree.unit(index))?;
        (unit.kind() == UnitKind::Word).then(|| tags.get(&word.span()).copied().map(|part_of_speech| PosAnnotation::new(word.token_unit_id(), part_of_speech))).flatten()
    }).collect::<Vec<_>>();
    tree.replace_pos_annotations(annotations);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_plain_words_are_tagged_and_existing_annotations_survive() {
        let mut tree = crate::parse_text("Close door");
        annotate_part_of_speech(&mut tree, [PosTaggedToken::new(0..5, PartOfSpeech::Verb), PosTaggedToken::new(6..10, PartOfSpeech::Noun)]);
        let words = tree.segment_words().iter().collect::<Vec<_>>();

        assert_eq!(words.iter().map(|word| word.part_of_speech()).collect::<Vec<_>>(), vec![Some(PartOfSpeech::Verb), Some(PartOfSpeech::Noun)]);
    }

    #[test]
    fn non_exact_tokens_are_skipped() {
        let mut tree = crate::parse_text("well-known value");
        annotate_part_of_speech(&mut tree, [PosTaggedToken::new(0..10, PartOfSpeech::Adj), PosTaggedToken::new(11..16, PartOfSpeech::Noun)]);
        let words = tree.segment_words().iter().collect::<Vec<_>>();

        assert_eq!(words.iter().filter_map(|word| word.part_of_speech()).collect::<Vec<_>>(), vec![PartOfSpeech::Noun]);
    }
}
