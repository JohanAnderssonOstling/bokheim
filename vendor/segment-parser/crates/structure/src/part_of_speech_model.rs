use serde::{Deserialize, Serialize};

use super::{PartOfSpeech, TokenUnitId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PosAnnotation {
    word_id: TokenUnitId,
    part_of_speech: PartOfSpeech,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct PosAnnotations {
    annotations: Box<[PosAnnotation]>,
}

impl PosAnnotation {
    pub fn new(word_id: TokenUnitId, part_of_speech: PartOfSpeech) -> Self { Self { word_id, part_of_speech } }
    pub fn word_id(self) -> TokenUnitId { self.word_id }
    pub fn part_of_speech(self) -> PartOfSpeech { self.part_of_speech }
}

impl PosAnnotations {
    pub(super) fn new(annotations: impl IntoIterator<Item = PosAnnotation>) -> Option<Self> {
        let mut annotations = annotations.into_iter().collect::<Vec<_>>();
        annotations.sort_unstable_by_key(|annotation| annotation.word_id().value());
        annotations.dedup_by_key(|annotation| annotation.word_id().value());
        (!annotations.is_empty()).then(|| Self { annotations: annotations.into_boxed_slice() })
    }
    pub(super) fn annotations(&self) -> &[PosAnnotation] { &self.annotations }
    pub(super) fn part_of_speech(&self, word_id: TokenUnitId) -> Option<PartOfSpeech> { self.annotations.binary_search_by_key(&word_id.value(), |annotation| annotation.word_id().value()).ok().map(|index| self.annotations[index].part_of_speech()) }
}
