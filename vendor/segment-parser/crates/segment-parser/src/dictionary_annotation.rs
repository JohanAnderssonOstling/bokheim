use std::collections::HashMap;

use structure::parser_api::{Node, NodeAnnotation, NodeSemantic};
use structure::{DictionaryAnalysis, NodeKind, NumberFormat, NumberSemantic, SegmentTree};

pub trait DictionaryLookup: Send {
    fn is_dictionary_word(&self, word: &str) -> bool;
    fn dictionary_baseform(&self, word: &str) -> Option<String>;
    fn has_dictionary(&self) -> bool;
    /// Lemmas and compound components from the dictionary's morphological analysis, when it has one.
    fn dictionary_analysis(&self, _word: &str) -> Option<DictionaryAnalysis> { None }
}

/// Remembers every lookup of the wrapped dictionary, for callers that annotate the same vocabulary
/// repeatedly (a session re-annotating targets after each edit). Single-threaded by design.
pub struct MemoizedDictionary {
    inner: Box<dyn DictionaryLookup>,
    words: std::cell::RefCell<HashMap<String, bool>>,
    baseforms: std::cell::RefCell<HashMap<String, Option<String>>>,
    analyses: std::cell::RefCell<HashMap<String, Option<DictionaryAnalysis>>>,
}

impl MemoizedDictionary {
    pub fn new(inner: Box<dyn DictionaryLookup>) -> Self { Self { inner, words: Default::default(), baseforms: Default::default(), analyses: Default::default() } }
}

impl DictionaryLookup for MemoizedDictionary {
    fn is_dictionary_word(&self, word: &str) -> bool { *self.words.borrow_mut().entry(word.to_string()).or_insert_with(|| self.inner.is_dictionary_word(word)) }
    fn dictionary_baseform(&self, word: &str) -> Option<String> { self.baseforms.borrow_mut().entry(word.to_string()).or_insert_with(|| self.inner.dictionary_baseform(word)).clone() }
    fn has_dictionary(&self) -> bool { self.inner.has_dictionary() }
    fn dictionary_analysis(&self, word: &str) -> Option<DictionaryAnalysis> { self.analyses.borrow_mut().entry(word.to_string()).or_insert_with(|| self.inner.dictionary_analysis(word)).clone() }
}

#[derive(Clone, Debug, Default)]
struct DictionaryAnnotationCache {
    entries: HashMap<String, Vec<NodeAnnotation>>,
}

impl DictionaryAnnotationCache {
    fn get(&self, word: &str) -> Option<Vec<NodeAnnotation>> { self.entries.get(&word.to_lowercase()).cloned() }
    fn insert_aliases<'a>(&mut self, aliases: impl IntoIterator<Item = &'a str>, annotations: &[NodeAnnotation]) { aliases.into_iter().filter(|alias| !alias.is_empty()).for_each(|alias| { self.entries.insert(alias.to_lowercase(), annotations.to_vec()); }); }
}

const RECOVERABLE_PREFIXES: &[&str] = &[
    "counter", "wieder", "vorbei", "hinter", "pseudo", "kontra", "hetero", "penta", "under", "super", "supra", "inter", "intra", "hyper", "ultra", "infra", "mikro", "micro", "makro", "macro", "retro", "trans",
    "anti", "multi", "mono", "poly", "hypo", "deci", "deka", "semi", "post", "tele", "astro", "auto", "homo", "euro", "omni", "mobil", "hexa", "tetra", "steno", "miss", "över", "till", "fram", "bort", "kvar", "unter",
    "durch", "pre", "non", "over", "out", "sub", "geo", "bio", "mega", "giga", "tera", "för", "sam", "van", "sär", "med", "ned", "upp", "ent", "emp", "auf", "aus", "ein", "mit", "nach", "wider", "mid",
    "un", "be", "av", "ut", "om", "an", "re", "in", "ge", "er", "ver", "zer", "ab", "um", "vor", "zu", "de", "mis", "co", "im", "il", "ir", "o",
];

/// Permissive membership: "would a spellchecker accept this?". Compounding
/// languages (Swedish, German) accept productively formed compounds that were
/// never listed — `husflygplan`, `Autohausding` — because that is correct for
/// spellchecking prose. Use this for the `DictionaryKnown`/`DictionaryUnknown`
/// annotation, where flagging a well-formed compound would be noise.
pub fn dictionary_matches(dictionary: &dyn DictionaryLookup, word: &str) -> bool {
    if !dictionary.has_dictionary() || word.is_empty() {
        return false;
    }
    if dictionary.is_dictionary_word(word) || dictionary.is_dictionary_word(&word.to_lowercase()) {
        return true;
    }
    dictionary.dictionary_baseform(word).is_some_and(|base| !base.is_empty() && (dictionary.is_dictionary_word(&base) || dictionary.is_dictionary_word(&base.to_lowercase())))
}

/// Strict-intent membership: "is this a real word of the language, rather than
/// a product identifier?". Used when *demoting* a `NonTranslatable` node back
/// to an ordinary `Word`, where a false positive silently removes the
/// protection the node existed to provide.
///
/// It currently delegates to [`dictionary_matches`] because no reliable
/// stricter test exists across the supported languages. The obvious candidate
/// was rejected on evidence: requiring the word to stem to *itself*
/// (distinguishing a listed entry from a productive compound) separates
/// perfectly in Swedish — 9/9 listed words, 6/6 invented compounds — but fails
/// in German, where the dictionary returns an empty stem for genuine entries
/// like `Bahnhof`, `Krankenhaus` and `Zahnarzt` (5 of 7 misclassified). Do not
/// retry that approach without new evidence.
///
/// Exposure is limited by the caller: a node only reaches declassification if
/// it already looked like an identifier (underscore, or an internal
/// lower->upper transition), so ordinary compounds never arrive here. The
/// residual risk is an ALLCAPS token whose lowercase form is a well-formed
/// compound. Kept as a separate name so the conflicting requirement is visible
/// and has one place to be tightened.
pub fn is_listed_dictionary_word(dictionary: &dyn DictionaryLookup, word: &str) -> bool {
    dictionary_matches(dictionary, word)
}

pub fn apply_dictionary(tree: &mut SegmentTree, dictionary: &dyn DictionaryLookup) {
    if !dictionary.has_dictionary() {
        return;
    }
    let root = declassify_dictionary_words(structure::parser_api::root_node(tree), tree.text(), dictionary);
    let mut cache = DictionaryAnnotationCache::default();
    let root = annotate_node_words(tree.text(), root, dictionary, &mut cache);
    structure::parser_api::replace_root_node(tree, root);
}

fn annotate_node_words(text: &str, node: Node, dictionary: &dyn DictionaryLookup, cache: &mut DictionaryAnnotationCache) -> Node {
    let children = node.iter_children().cloned().map(|child| annotate_node_words(text, child, dictionary, cache)).collect();
    let node = node.with_children(children);
    if node.kind() == NodeKind::Term {
        return annotate_term_fragment(text, node, dictionary);
    }
    if node.kind() != NodeKind::Word {
        return node;
    }
    let word = text.get(node.span()).unwrap_or("");
    let mut annotations = node.annotations().iter().filter(|annotation| !matches!(annotation, NodeAnnotation::DictionaryKnown | NodeAnnotation::DictionaryUnknown | NodeAnnotation::DictionaryBaseform(_))).cloned().collect::<Vec<_>>();
    annotations.extend(dictionary_annotations(word, dictionary, cache));
    node.with_annotations(annotations)
}

/// A term hit inside a longer word (`konto` in `kontor`) becomes its own unit, so no word node keeps the
/// whole word; the term node records the whole word's analysis instead, for terminology checks to consult.
fn annotate_term_fragment(text: &str, node: Node, dictionary: &dyn DictionaryLookup) -> Node {
    let span = node.span();
    if crate::stream_util::is_whole_word_match(text, &span) {
        return node;
    }
    let containing = text.get(crate::stream_util::containing_word_span(text, &span)).unwrap_or("");
    let annotations = node.annotations().iter().filter(|annotation| !matches!(annotation, NodeAnnotation::DictionaryAnalysis(_))).cloned().chain(dictionary.dictionary_analysis(containing).map(|analysis| NodeAnnotation::DictionaryAnalysis(Box::new(analysis)))).collect();
    node.with_annotations(annotations)
}

fn dictionary_annotations(word: &str, dictionary: &dyn DictionaryLookup, cache: &mut DictionaryAnnotationCache) -> Vec<NodeAnnotation> {
    if !dictionary.has_dictionary() {
        return Vec::new();
    }
    if let Some(annotations) = cache.get(word) {
        return annotations;
    }
    let baseform = dictionary.dictionary_baseform(word);
    let candidate = baseform.as_deref().unwrap_or(word);
    let direct_match = dictionary_matches(dictionary, candidate);
    let prefix_match = (!direct_match).then(|| dictionary_match_after_prefix_removal(candidate, dictionary)).flatten();
    let known = direct_match || prefix_match.is_some();
    let annotations = if known {
        let mut annotations = vec![NodeAnnotation::DictionaryKnown];
        baseform.iter().filter(|baseform| !crate::language_parser::language_text_eq(baseform, word)).cloned().for_each(|baseform| annotations.push(NodeAnnotation::DictionaryBaseform(baseform)));
        annotations.extend(dictionary.dictionary_analysis(word).map(|analysis| NodeAnnotation::DictionaryAnalysis(Box::new(analysis))));
        annotations
    } else {
        vec![NodeAnnotation::DictionaryUnknown]
    };
    if known && annotations.iter().any(|annotation| matches!(annotation, NodeAnnotation::DictionaryBaseform(_) | NodeAnnotation::DictionaryAnalysis(_))) {
        cache.insert_aliases([word], &annotations);
    } else if known {
        cache.insert_aliases([Some(word), (candidate != word).then_some(candidate), prefix_match].into_iter().flatten(), &annotations);
    } else {
        cache.insert_aliases([word], &annotations);
    }
    annotations
}

fn dictionary_match_after_prefix_removal<'a>(word: &'a str, dictionary: &dyn DictionaryLookup) -> Option<&'a str> {
    RECOVERABLE_PREFIXES
        .iter()
        .filter_map(|prefix| word.get(..prefix.len()).filter(|observed| crate::language_parser::language_text_eq(observed, prefix)).and_then(|_| word.get(prefix.len()..)).map(|remainder| (*prefix, remainder)))
        .filter(|(_, remainder)| remainder.chars().count() >= 4 && dictionary_matches(dictionary, remainder))
        .max_by_key(|(prefix, _)| prefix.chars().count())
        .map(|(_, remainder)| remainder)
}

fn declassify_dictionary_words(node: Node, text: &str, dictionary: &dyn DictionaryLookup) -> Node {
    let children = node.iter_children().cloned().flat_map(|child| declassified_dictionary_nodes(child, text, dictionary)).collect();
    node.with_children(children)
}

fn declassified_dictionary_nodes(node: Node, text: &str, dictionary: &dyn DictionaryLookup) -> Vec<Node> {
    let node = declassify_dictionary_words(node, text, dictionary);
    if node.kind() != NodeKind::Opaque {
        return vec![node];
    }
    let value = text.get(node.span()).unwrap_or("");
    if is_listed_dictionary_word(dictionary, value) {
        return vec![node.with_kind(NodeKind::Word)];
    }
    dictionary_word_reference_nodes(value, node.start(), dictionary).unwrap_or_else(|| vec![node])
}

fn dictionary_word_reference_nodes(value: &str, absolute_start: usize, dictionary: &dyn DictionaryLookup) -> Option<Vec<Node>> {
    let digit_start = value.char_indices().rev().take_while(|(_, character)| character.is_ascii_digit()).last().map(|(index, _)| index)?;
    let (word, digits) = value.split_at(digit_start);
    if word.chars().count() < 4 || !word.chars().all(char::is_alphabetic) || !(1..=2).contains(&digits.len()) || !is_listed_dictionary_word(dictionary, word) {
        return None;
    }
    let reference = digits.parse::<i128>().ok()?.checked_mul(crate::parser::NORMALIZED_VALUE_SCALE as i128)?;
    let split = absolute_start + digit_start;
    let word = Node::new(NodeKind::Word, absolute_start..split, None, Vec::new());
    let number = Node::with_semantic(split..absolute_start + value.len(), None, NodeSemantic::Number(NumberSemantic::new(reference, NumberFormat::default())), Vec::new());
    Some(vec![word, number])
}
