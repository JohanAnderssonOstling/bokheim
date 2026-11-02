//! Shared lookup representations. Display text and persisted identity keys stay unchanged.
use super::{author_match_keys, author_words_compatible, bibliographic_match_key, bibliographic_title_match_key, omitted_given_names, title_qualifiers, title_substring_keys};
use std::collections::BTreeSet;

pub fn author_suffix(word: &str) -> Option<&'static str> {
    match word {
        "jr" | "junior" => Some("jr"),
        "sr" | "senior" => Some("sr"),
        "ii" => Some("ii"),
        "iii" => Some("iii"),
        "iv" => Some("iv"),
        "v" => Some("v"),
        _ => None,
    }
}

#[derive(Clone, Debug)]
pub struct NormalizedAuthor {
    pub keys: BTreeSet<String>,
    pub variants: Vec<AuthorVariant>,
}
#[derive(Clone, Debug)]
pub struct AuthorVariant {
    pub words: Vec<String>,
    pub suffix: Option<&'static str>,
}
impl NormalizedAuthor {
    pub fn new(value: &str) -> Self {
        let mut variants = author_match_keys(value);
        let parts = value.split(',').map(str::trim).collect::<Vec<_>>();
        if let [family, given, suffix] = parts.as_slice() {
            if let Some(suffix) = author_suffix(&bibliographic_match_key(suffix)) {
                variants.extend(author_match_keys(&format!("{given} {family} {suffix}")));
            }
        }
        let keys: BTreeSet<String> = variants
            .into_iter()
            .map(|key| {
                let mut words = key.split_whitespace().map(str::to_owned).collect::<Vec<_>>();
                if let Some(last) = words.last_mut() {
                    if let Some(suffix) = author_suffix(last) {
                        *last = suffix.to_owned();
                    }
                }
                words.join(" ")
            })
            .collect();
        let variants = keys
            .iter()
            .map(|key: &String| {
                let mut words = key.split_whitespace().map(str::to_owned).collect::<Vec<_>>();
                let suffix = words.last().and_then(|word| author_suffix(word));
                if suffix.is_some() {
                    words.pop();
                }
                AuthorVariant { words, suffix }
            })
            .collect();
        Self { keys, variants }
    }

    pub(super) fn compatible(&self, other: &Self, allow_omitted_names: bool) -> bool {
        if !self.keys.is_disjoint(&other.keys) {
            return true;
        }
        self.variants.iter().any(|a| {
            other.variants.iter().any(|b| {
                if a.suffix.is_some() && b.suffix.is_some() && a.suffix != b.suffix {
                    return false;
                }
                author_words_compatible(&a.words, &b.words) || (allow_omitted_names && omitted_given_names(&a.words, &b.words))
            })
        })
    }

    pub fn candidate_tokens(&self) -> BTreeSet<String> {
        self.keys.iter().flat_map(|key| key.split_whitespace()).filter(|word| word.len() > 1 && author_suffix(word).is_none()).map(str::to_owned).collect()
    }
}

/// Number of distinct compatible author credits. Extra or missing credits do not
/// veto a match; each credit can contribute at most one vote.
pub fn matching_author_count(local: &[impl AsRef<str>], remote: &[impl AsRef<str>]) -> usize {
    matching_author_count_inner(local, remote, false)
}

pub fn matching_author_count_with_title(local: &[impl AsRef<str>], remote: &[impl AsRef<str>], title: &str, remote_title: &str) -> usize {
    matching_author_count_inner(local, remote, author_titles_agree(title, remote_title))
}

pub(super) fn author_titles_agree(title: &str, remote_title: &str) -> bool {
    let local = NormalizedTitle::new(title);
    let remote = NormalizedTitle::new(remote_title);
    fn key(value: &str) -> &str { value.strip_prefix("the ").unwrap_or(value) }
    !local.full.is_empty() && local.qualifiers == remote.qualifiers
        && (key(&local.full) == key(&remote.full) || (local.subtitle.is_none() && key(&local.full) == key(&remote.main)))
}

fn matching_author_count_inner(local: &[impl AsRef<str>], remote: &[impl AsRef<str>], allow_omitted_names: bool) -> usize {
    fn unique(values: &[impl AsRef<str>]) -> Vec<NormalizedAuthor> {
        let mut names: Vec<NormalizedAuthor> = Vec::new();
        for name in super::author_credits(values) {
            let normalized = NormalizedAuthor::new(&name);
            if normalized.keys.is_empty() || matches!(name.trim().to_ascii_lowercase().as_str(), "unknown" | "untitled") {
                continue;
            }
            if !names.iter().any(|old| !old.keys.is_disjoint(&normalized.keys)) {
                names.push(normalized);
            }
        }
        names
    }
    // Maximum bipartite matching avoids counting one remote person twice and
    // makes the result independent of the order of initials/full-name variants.
    fn assign(i: usize, edges: &[Vec<usize>], seen: &mut [bool], owners: &mut [Option<usize>]) -> bool {
        for &j in &edges[i] {
            if seen[j] {
                continue;
            }
            seen[j] = true;
            if owners[j].is_none_or(|owner| assign(owner, edges, seen, owners)) {
                owners[j] = Some(i);
                return true;
            }
        }
        false
    }
    let local = unique(local);
    let remote = unique(remote);
    let edges = local.iter().map(|a| remote.iter().enumerate().filter_map(|(i, b)| a.compatible(b, allow_omitted_names).then_some(i)).collect()).collect::<Vec<Vec<usize>>>();
    let mut owners = vec![None; remote.len()];
    (0..local.len()).filter(|&i| assign(i, &edges, &mut vec![false; remote.len()], &mut owners)).count()
}

#[derive(Clone, Debug)]
pub struct NormalizedTitle {
    pub text: String,
    pub full: String,
    pub main: String,
    pub subtitle: Option<String>,
    pub qualifiers: BTreeSet<String>,
    original: String,
}

fn title_parts(text: &str) -> (&str, Option<&str>) {
    let colon = text.find(':').map(|offset| (offset, 1));
    let slash = text.find(" / ").map(|offset| (offset, 3));
    match colon.into_iter().chain(slash).min_by_key(|(offset, _)| *offset) {
        Some((offset, separator)) => (&text[..offset], Some(&text[offset + separator..])),
        None => (text, None),
    }
}

impl NormalizedTitle {
    pub fn new(value: &str) -> Self {
        // Catalogue titles carry no filename shapes; the device cleans those
        // out of a book's own titles before asking.
        let text = super::title_without_parenthetical_segments(&value.trim().replace(';', ": "), false);
        let full = bibliographic_title_match_key(&text);
        let (main, subtitle) = title_parts(&text);
        let (main, subtitle) = (bibliographic_title_match_key(main), subtitle.map(bibliographic_title_match_key));
        Self { text, full, main, subtitle, qualifiers: title_qualifiers(value), original: value.to_owned() }
    }
    pub fn matches(&self, other: &Self) -> bool {
        !self.full.is_empty() && self.full == other.full && self.qualifiers == other.qualifiers
    }
    /// Query both current and legacy punctuation keys without rewriting immutable snapshots.
    pub fn lookup_keys(&self) -> Vec<String> {
        let mut keys = Vec::new();
        for text in [self.text.as_str(), self.original.as_str(), title_parts(&self.text).0] {
            for key in [bibliographic_title_match_key(text), bibliographic_match_key(text)] {
                if !key.is_empty() && !keys.contains(&key) {
                    keys.push(key);
                }
            }
        }
        keys
    }
    pub fn candidate_keys(&self, limit: usize) -> Vec<(String, usize)> {
        let mut keys = self
            .lookup_keys()
            .into_iter()
            .map(|key| {
                let n = key.chars().count();
                (key, n)
            })
            .collect::<Vec<_>>();
        // Initial articles vary between catalog entries. Keep these as lookup
        // alternatives; persisted keys and exact edition comparisons stay intact.
        for text in [&self.full, &self.main] {
            if text.is_empty() { continue; }
            let key = text.strip_prefix("the ").map_or_else(|| format!("the {text}"), str::to_owned);
            if !keys.iter().any(|(old, _)| old == &key) {
                keys.push((key, text.chars().count()));
            }
        }
        for item in title_substring_keys(&self.text, limit) {
            if !keys.iter().any(|(key, _)| key == &item.0) {
                keys.push(item);
            }
        }
        keys
    }
}
