use std::collections::HashMap;
use std::ops::Range;

use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use segment_lexer::word_token_spans;

use super::{NodeKind, SegmentTree, TokenUnit, TokenUnitId, UnitKind};

#[path = "word_model/view.rs"]
mod view;
pub use view::{SegmentWordRef, SegmentWords};

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct SegmentWordAttributes {
    alphabetic_count: u16,
    capitalized: bool,
    has_uppercase: bool,
}

impl SegmentWordAttributes {
    fn new(text: &str) -> Self {
        let capitalized = text.chars().next().is_some_and(char::is_uppercase);
        let (alphabetic_count, has_uppercase) = text.chars().fold((0u16, false), |(count, uppercase), character| {
            (count.saturating_add(u16::from(character.is_alphabetic())), uppercase || character.is_uppercase())
        });
        Self { alphabetic_count, capitalized, has_uppercase }
    }
}

#[repr(C)]
#[derive(Clone, Debug, PartialEq, Eq)]
struct StoredSegmentWord {
    normalized: Option<Box<str>>,
    token_unit_id: TokenUnitId,
    attributes: SegmentWordAttributes,
    start: u16,
    end: u16,
    whitespace_before: bool,
}

/// Dictionary readings of a word reduced to what lexical matching needs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DictionaryAnalysis {
    lemmas: Box<[Box<str>]>,
    components: Box<[Box<str>]>,
    authoritative: bool,
}

impl DictionaryAnalysis {
    pub fn new(lemmas: Vec<String>, components: Vec<String>, authoritative: bool) -> Self { Self { lemmas: lemmas.into_iter().map(String::into_boxed_str).collect(), components: components.into_iter().map(String::into_boxed_str).collect(), authoritative } }
    /// Every lemma the word can be an inflection of (`planet` → `planet`, `plan`).
    pub fn lemmas(&self) -> impl Iterator<Item = &str> { self.lemmas.iter().map(AsRef::as_ref) }
    /// Compound parts as written and as lemmas (`kontorsstolen` → `kontor`, `stol`).
    pub fn components(&self) -> impl Iterator<Item = &str> { self.components.iter().map(AsRef::as_ref) }
    /// The word is a full lexicon entry, so its lemmas and components are complete and look-alike heuristics must not add to them.
    pub fn is_authoritative(&self) -> bool { self.authoritative }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DictionaryWordAnnotation {
    Plain,
    Annotated(u16),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct DictionaryWordIndex {
    words: HashMap<String, DictionaryWordAnnotation>,
}

impl DictionaryWordIndex {
    fn insert(&mut self, word: String, annotation: DictionaryWordAnnotation) { self.words.entry(word).and_modify(|stored| { if matches!(annotation, DictionaryWordAnnotation::Annotated(_)) { *stored = annotation; } }).or_insert(annotation); }
    fn contains(&self, word: &str) -> bool { self.words.contains_key(word) }
    fn annotation(&self, word: &str) -> Option<DictionaryWordAnnotation> { self.words.get(word).copied() }
}

impl Serialize for DictionaryWordIndex {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> { serializer.collect_seq(self.words.keys()) }
}

impl<'de> Deserialize<'de> for DictionaryWordIndex {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> { Ok(Self { words: Vec::<String>::deserialize(deserializer)?.into_iter().map(|word| (word, DictionaryWordAnnotation::Plain)).collect() }) }
}

#[derive(Deserialize)]
struct SerializedSegmentWord {
    token_unit_id: TokenUnitId,
    start: u16,
    end: u16,
    normalized: Option<Box<str>>,
    attributes: SegmentWordAttributes,
    whitespace_before: bool,
}

impl Serialize for StoredSegmentWord {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("StoredSegmentWord", 6)?;
        state.serialize_field("token_unit_id", &self.token_unit_id)?;
        state.serialize_field("start", &self.start)?;
        state.serialize_field("end", &self.end)?;
        state.serialize_field("normalized", &self.normalized)?;
        state.serialize_field("attributes", &self.attributes)?;
        state.serialize_field("whitespace_before", &self.whitespace_before)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for StoredSegmentWord {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let stored = SerializedSegmentWord::deserialize(deserializer)?;
        Ok(Self { normalized: stored.normalized, token_unit_id: stored.token_unit_id, attributes: stored.attributes, start: stored.start, end: stored.end, whitespace_before: stored.whitespace_before })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct SegmentWordStorage {
    words: Box<[StoredSegmentWord]>,
    has_dictionary_annotations: bool,
    dictionary_known_words: DictionaryWordIndex,
}

impl SegmentWordStorage {
    pub(super) fn new(text: &str, units: &[TokenUnit]) -> Self {
        let mut words = Vec::with_capacity(units.len());
        let mut saw_word = false;
        let mut saw_separator = false;
        let mut separator_is_whitespace = true;
        units.iter().for_each(|unit| {
            let spans = match unit.kind() {
                UnitKind::Word => vec![unit.span()],
                UnitKind::Term => word_token_spans(text.get(unit.span()).unwrap_or("")).map(|(span, _)| unit.start() + span.start..unit.start() + span.end).collect(),
                _ => Vec::new(),
            };
            if spans.is_empty() {
                if saw_word {
                    saw_separator = true;
                    separator_is_whitespace &= unit.kind() == UnitKind::Whitespace;
                }
                return;
            }
            let token_unit_id = unit.token_id().expect("tree units must have stable token ids");
            let mut previous_end = None;
            spans.into_iter().for_each(|span| {
                let internal_whitespace = previous_end.is_some_and(|end| end < span.start && text.get(end..span.start).is_some_and(|gap| gap.chars().all(char::is_whitespace)));
                let whitespace_before = if previous_end.is_some() { internal_whitespace } else { saw_word && saw_separator && separator_is_whitespace };
                words.push(stored_segment_word(text, token_unit_id, span.clone(), whitespace_before));
                previous_end = Some(span.end);
                saw_word = true;
                saw_separator = false;
                separator_is_whitespace = true;
            });
            if let Some(end) = previous_end.filter(|end| *end < unit.end()) {
                saw_separator = true;
                separator_is_whitespace = text.get(end..unit.end()).is_some_and(|gap| gap.chars().all(char::is_whitespace));
            }
        });
        Self { words: words.into_boxed_slice(), has_dictionary_annotations: false, dictionary_known_words: DictionaryWordIndex::default() }
    }

    pub(super) fn set_dictionary_annotations(&mut self, has_dictionary_annotations: bool, dictionary_known_words: DictionaryWordIndex) {
        self.has_dictionary_annotations = has_dictionary_annotations;
        self.dictionary_known_words = dictionary_known_words;
    }
}

fn stored_segment_word(text: &str, token_unit_id: TokenUnitId, span: Range<usize>, whitespace_before: bool) -> StoredSegmentWord {
    let word_text = text.get(span.clone()).unwrap_or("");
    let attributes = SegmentWordAttributes::new(word_text);
    let normalized = attributes.has_uppercase.then(|| word_text.to_lowercase().into_boxed_str());
    StoredSegmentWord { token_unit_id, start: u16::try_from(span.start).expect("word start exceeds u16 range"), end: u16::try_from(span.end).expect("word end exceeds u16 range"), normalized, attributes, whitespace_before }
}

pub(super) fn segment_word_dictionary_annotations(tree: &SegmentTree) -> (bool, DictionaryWordIndex) {
    tree.preorder_iter().filter(|node| node.kind() == NodeKind::Word && node.has_annotations()).fold((false, DictionaryWordIndex::default()), |(_, mut words), node| {
        if node.dictionary_known() {
            let annotated = node.dictionary_baseform().is_some() || node.dictionary_analysis().is_some();
            let annotation = annotated.then(|| node.annotations_index()).flatten().map(|index| DictionaryWordAnnotation::Annotated(u16::try_from(index).expect("word annotation index exceeds u16 range"))).unwrap_or(DictionaryWordAnnotation::Plain);
            words.insert(node.text().to_lowercase(), annotation);
        }
        (true, words)
    })
}

#[cfg(test)]
pub(super) fn stored_segment_word_size() -> usize { std::mem::size_of::<StoredSegmentWord>() }
