//! Shared bibliographic normalization for ebook and audiobook discovery.
use std::collections::BTreeSet;
use std::sync::OnceLock;
use serde_json::{from_str, Value};
use crate::{EditionIdentityMatch, EditionIdentityQuery};
mod author;
mod normalized;
pub use author::author_name_match_key;
use author::initial_expansion_variants;
pub use normalized::{author_suffix, matching_author_count, matching_author_count_with_title, NormalizedAuthor, NormalizedTitle};
use unicode_normalization::UnicodeNormalization;

pub fn bibliographic_match_key(value: &str) -> String {
    use unicode_normalization::char::is_combining_mark;
    let mut key = String::new();
    let mut separator = false;
    for character in value.nfkd().flat_map(char::to_lowercase) {
        if is_combining_mark(character) {
            continue;
        }
        if character.is_alphanumeric() {
            if separator && !key.is_empty() {
                key.push(' ');
            }
            key.push(character);
            separator = false;
        } else {
            separator = true;
        }
    }
    key
}

/// Match punctuation variants such as Owner's/Owners without merging words
/// separated by other punctuation. Keep the legacy key for stored-index queries.
pub fn title_apostrophe_variant(value: &str) -> String {
    let chars = value.chars().collect::<Vec<_>>();
    chars
        .iter()
        .enumerate()
        .filter_map(|(i, &c)| {
            let internal = matches!(c, '\'' | '’' | 'ʼ') && i > 0 && i + 1 < chars.len() && chars[i - 1].is_alphanumeric() && chars[i + 1].is_alphanumeric();
            (!internal).then_some(c)
        })
        .collect()
}

pub fn bibliographic_title_match_key(value: &str) -> String {
    bibliographic_match_key(&title_apostrophe_variant(value))
}

fn base_author_match_keys(value: &str) -> BTreeSet<String> {
    let mut names = vec![value.to_owned()];
    if let Some((family, given)) = value.split_once(',') {
        names.push(format!("{given} {family}"));
    }
    let mut keys = BTreeSet::new();
    for name in names {
        for variant in initial_expansion_variants(&name) {
            let key = author_name_match_key(&variant);
            if let Some(without_article) = key.strip_prefix("the ").filter(|rest| collective_author_name(rest)) {
                keys.insert(without_article.to_owned());
            }
            keys.insert(key);
        }
    }
    keys.remove("");
    keys
}

fn collective_author_name(key: &str) -> bool {
    key.split_whitespace().any(|word| matches!(word, "initiates" | "committee" | "commission" | "society" | "association" | "company" | "institute" | "foundation" | "board" | "council" | "team" | "collective" | "group"))
}

pub fn matching_edition_author_count(query: &EditionIdentityQuery, candidate: &EditionIdentityMatch) -> usize {
    matching_author_count_with_title(&query.authors, &candidate.authors, &query.title, &candidate.full_title())
}

pub fn author_match_keys(value: &str) -> BTreeSet<String> {
    let mut keys = base_author_match_keys(value);
    // Some authority names explicitly include a Latin alias after a name in
    // another script. Roles and suffixes on Latin names are not aliases.
    if let Some((native, rest)) = value.split_once('(') {
        if let Some(alias) = rest.strip_suffix(')') {
            let native_key = bibliographic_match_key(native);
            let alias_key = bibliographic_match_key(alias);
            if native_key.chars().any(|c| c.is_alphabetic() && !c.is_ascii())
                && !native_key.chars().any(|c| c.is_ascii_alphabetic())
                && alias_key.split_whitespace().count() >= 2
                && alias_key.chars().all(|c| c.is_ascii())
            {
                keys.extend(base_author_match_keys(alias));
            }
        }
    }
    static ALIASES: OnceLock<Vec<BTreeSet<String>>> = OnceLock::new();
    for group in ALIASES.get_or_init(|| {
        let records: Value = from_str(include_str!("../data/reviewed-author-aliases.json")).expect("reviewed author aliases must be valid JSON");
        records.as_array().expect("author aliases must be an array").iter().map(|record| {
            record["aliases"].as_array().expect("aliases must be an array").iter().flat_map(|alias| base_author_match_keys(alias.as_str().expect("alias must be text"))).collect()
        }).collect()
    }) {
        if !keys.is_disjoint(group) {
            keys.extend(group.iter().cloned());
        }
    }
    keys
}

/// Whether two credits name the same person: the names agree once accents,
/// honorifics, life dates and surname-first comma notation are set aside.
/// Arbitrary word permutations belong to `author_names_compatible` fallback.
pub fn same_author(left: &str, right: &str) -> bool {
    !author_match_keys(left).is_disjoint(&author_match_keys(right))
}

fn sorted_author_words(words: &[String]) -> Vec<&str> {
    let mut words = words.iter().map(String::as_str).collect::<Vec<_>>();
    words.sort_unstable();
    words
}

fn author_words_compatible(left: &[String], right: &[String]) -> bool {
    if !left.is_empty() && sorted_author_words(left) == sorted_author_words(right) {
        return true;
    }
    if left.len() < 2 || right.len() < 2 || !left.iter().any(|word| word.chars().count() > 1 && right.contains(word)) {
        return false;
    }
    let mut left = left.iter().map(String::as_str).collect::<Vec<_>>();
    let mut right = right.iter().map(String::as_str).collect::<Vec<_>>();
    // Match explicit shared tokens first. Otherwise John J. Smith could match
    // James J. Smith by exchanging the full given names with their initials.
    let mut i = 0;
    while i < left.len() {
        if let Some(j) = right.iter().position(|word| *word == left[i]) {
            left.remove(i);
            right.remove(j);
        } else {
            i += 1;
        }
    }
    let (short, long) = if left.len() <= right.len() { (left, right) } else { (right, left) };
    let edges = short.iter().map(|a| long.iter().enumerate().filter_map(|(i, b)| {
        ((a.chars().count() == 1 && b.starts_with(a)) || (b.chars().count() == 1 && a.starts_with(b))).then_some(i)
    }).collect::<Vec<_>>()).collect::<Vec<_>>();
    fn assign(i: usize, edges: &[Vec<usize>], seen: &mut [bool], owners: &mut [Option<usize>]) -> bool {
        for &j in &edges[i] {
            if seen[j] { continue; }
            seen[j] = true;
            if owners[j].is_none() || assign(owners[j].unwrap(), edges, seen, owners) {
                owners[j] = Some(i);
                return true;
            }
        }
        false
    }
    let mut owners = vec![None; long.len()];
    (0..short.len()).all(|i| assign(i, &edges, &mut vec![false; long.len()], &mut owners))
        && long.iter().enumerate().all(|(i, word)| owners[i].is_some() || word.chars().count() == 1)
}

pub fn author_match_tokens(value: &str) -> BTreeSet<String> {
    NormalizedAuthor::new(value).candidate_tokens()
}

/// Compare author credits in any word order, allowing initials and omitted
/// initials. Conflicting full names, stated initials and suffixes stay distinct.
pub fn author_names_compatible(left: &str, right: &str) -> bool {
    compatible_author_names(left, right, false)
}

/// Omitted given names need agreement on the complete title or its main title;
/// strict name matching still applies to fragment-only title evidence.
pub fn author_names_compatible_with_title(left: &str, right: &str, title: &str, remote_title: &str) -> bool {
    compatible_author_names(left, right, normalized::author_titles_agree(title, remote_title))
}

/// Compare omitted given names after the caller has verified the work identity.
/// Conflicting explicit names, initials and generational suffixes stay distinct.
pub fn author_names_with_omitted_names(left: &str, right: &str) -> bool {
    compatible_author_names(left, right, true)
}

fn compatible_author_names(left: &str, right: &str, allow_omitted_names: bool) -> bool {
    let left = NormalizedAuthor::new(left);
    let right = NormalizedAuthor::new(right);
    if !left.keys.is_disjoint(&right.keys) {
        return true;
    }
    left.variants.iter().any(|a| {
        right.variants.iter().any(|b| {
            if a.suffix.is_some() && b.suffix.is_some() && a.suffix != b.suffix {
                return false;
            }
            author_words_compatible(&a.words, &b.words) || (allow_omitted_names && omitted_given_names(&a.words, &b.words))
        })
    })
}

fn omitted_given_names(left: &[String], right: &[String]) -> bool {
    let (short, long) = if left.len() < right.len() { (left, right) } else { (right, left) };
    if short.len() < 2 || short.len() == long.len() || short.last() != long.last() || short.last().is_none_or(|word| word.chars().count() < 2) {
        return false;
    }
    let short = &short[..short.len() - 1];
    let long = &long[..long.len() - 1];
    // At least one shared full given name is required; a surname and initials
    // alone cannot justify dropping additional full names.
    if !short.iter().any(|word| word.chars().count() > 1 && !matches!(word.as_str(), "of" | "the" | "de" | "del" | "di" | "da" | "van" | "von") && long.contains(word)) {
        return false;
    }
    let mut cursor = 0;
    for word in short {
        let Some(next) = long[cursor..].iter().position(|other| word == other || (word.chars().count() == 1 && other.starts_with(word)) || (other.chars().count() == 1 && word.starts_with(other))) else { return false };
        cursor += next + 1;
    }
    true
}


/// Bounded whole-word title fragments shared with edition candidate lookup.
pub fn title_substring_keys(value: &str, limit: usize) -> Vec<(String, usize)> {
    let normalized = bibliographic_title_match_key(value);
    let words = normalized.split_whitespace().take(limit).collect::<Vec<_>>();
    let mut substrings = std::collections::BTreeMap::<String, usize>::new();
    for start in 0..words.len() {
        for end in start + 1..=words.len() {
            let substring = words[start..end].join(" ");
            let specificity = substring.chars().count();
            substrings.entry(substring).or_insert(specificity);
        }
    }
    let mut substrings = substrings.into_iter().collect::<Vec<_>>();
    substrings.sort_by(|(left_title, left_specificity), (right_title, right_specificity)| right_specificity.cmp(left_specificity).then_with(|| left_title.cmp(right_title)));
    substrings.truncate(limit);
    substrings
}

/// Words whose parenthetical placement must never hide a structural distinction.
pub fn is_title_qualifier_word(word: &str) -> bool {
    matches!(word, "volume" | "vol" | "part" | "book" | "abridged" | "abridgment" | "abridgement" | "adapted" | "adaptation" | "retold" | "retelling" | "workbook" | "activity" | "selections" | "solutions" | "answers")
}

pub fn title_qualifiers(title: &str) -> BTreeSet<String> {
    let normalized = bibliographic_match_key(title);
    let words = normalized.split_whitespace().collect::<Vec<_>>();
    let mut qualifiers = BTreeSet::new();
    for (index, word) in words.iter().enumerate() {
        if matches!(*word, "volume" | "vol" | "part" | "book") {
            if let Some(number) = words.get(index + 1).and_then(|s| match *s {
                "i" | "one" => Some("1"),
                "ii" | "two" => Some("2"),
                "iii" | "three" => Some("3"),
                "iv" | "four" => Some("4"),
                "v" | "five" => Some("5"),
                "vi" | "six" => Some("6"),
                "vii" | "seven" => Some("7"),
                "viii" | "eight" => Some("8"),
                "ix" | "nine" => Some("9"),
                "x" | "ten" => Some("10"),
                s if s.bytes().all(|c| c.is_ascii_digit()) => Some(s),
                _ => None,
            }) {
                qualifiers.insert(format!("{}:{number}", if *word == "vol" { "volume" } else { word }));
            }
        }
        if matches!(*word, "abridged" | "abridgment" | "abridgement" | "adapted" | "adaptation" | "retold" | "retelling" | "workbook" | "activity" | "selections") {
            qualifiers.insert(word.to_string());
        }
    }
    qualifiers
}

/// Split explicit coauthor separators without confusing `Family, Given` names.
pub fn author_credits(values: &[String]) -> Vec<String> {
    let mut credits = Vec::new();
    for value in values {
        if collective_author_name(&author_name_match_key(value)) {
            if !credits.contains(value) {
                credits.push(value.trim().to_owned());
            }
            continue;
        }
        let mut parts = vec![value.trim().to_owned()];
        for separator in [" with ", " and ", " & ", ";"] {
            parts = parts
                .into_iter()
                .flat_map(|part| {
                    let mut result = Vec::new();
                    let mut rest = part.as_str();
                    while let Some(index) = rest.as_bytes().windows(separator.len()).position(|window| window.eq_ignore_ascii_case(separator.as_bytes())) {
                        result.push(rest[..index].trim().to_owned());
                        rest = &rest[index + separator.len()..];
                    }
                    result.push(rest.trim().to_owned());
                    if matches!(separator, " and " | " & ") {
                        expand_shared_surnames(&mut result);
                    }
                    result
                })
                .collect();
        }
        for part in parts {
            let chunks = part.split(',').map(str::trim).collect::<Vec<_>>();
            let is_suffix = |s: &str| matches!(bibliographic_match_key(s).as_str(), "jr" | "sr" | "ii" | "iii" | "iv");
            // A suffix disambiguates an inverted name followed by full coauthor names.
            let expanded = if chunks.len() >= 3 && is_suffix(chunks[2]) && chunks[3..].iter().all(|s| s.split_whitespace().count() >= 2) {
                let mut names = vec![format!("{} {} {}", chunks[1], chunks[0], chunks[2])];
                names.extend(chunks[3..].iter().map(|s| (*s).to_owned()));
                names
            } else {
                vec![part]
            };
            for name in expanded {
                if !name.is_empty() && !credits.contains(&name) {
                    credits.push(name);
                }
            }
        }
    }
    credits
}

fn expand_shared_surnames(names: &mut [String]) {
    let mut surname = None;
    for name in names.iter_mut().rev() {
        if name.contains([',', '(', ')', '[', ']']) {
            surname = None;
            continue;
        }
        let key = author_name_match_key(name);
        let words = key.split_whitespace().collect::<Vec<_>>();
        if collective_author_name(&key) || words.is_empty() || matches!(key.as_str(), "anonymous" | "unknown" | "various" | "untitled") {
            surname = None;
            continue;
        }
        if words.len() == 1 || (words[0].chars().count() > 1 && words[1..].iter().all(|word| word.chars().count() == 1)) {
            if let Some(family) = &surname {
                *name = format!("{name} {family}");
            }
            continue;
        }
        let raw = name.split_whitespace().collect::<Vec<_>>();
        let end = raw.len() - usize::from(raw.last().is_some_and(|word| author_suffix(&bibliographic_match_key(word)).is_some()));
        if end < 2 {
            surname = None;
            continue;
        }
        let mut start = end - 1;
        while start > 1 && matches!(bibliographic_match_key(raw[start - 1]).as_str(), "de" | "del" | "della" | "da" | "di" | "du" | "van" | "von" | "der" | "den" | "la" | "le" | "al" | "bin" | "ibn") {
            start -= 1;
        }
        surname = Some(raw[start..end].join(" "));
    }
}

/// Complete groups can mix parentheses and brackets. Unclosed groups remain
/// intact so cleanup cannot swallow the remaining title.
pub fn delimited_segments(value: &str) -> Vec<(usize, usize)> {
    let mut stack = Vec::new();
    let mut start = 0;
    let mut spans = Vec::new();
    for (index, character) in value.char_indices() {
        match character {
            '(' | '[' => {
                if stack.is_empty() {
                    start = index;
                }
                stack.push(character);
            }
            ')' | ']' if !stack.is_empty() => {
                stack.pop();
                if stack.is_empty() {
                    spans.push((start, index + 1));
                }
            }
            _ => {}
        }
    }
    spans
}

/// Drops parenthetical groups such as series names. Volume and adaptation
/// qualifiers always stay; edition notes stay when `keep_edition_notes`.
pub fn title_without_parenthetical_segments(title: &str, keep_edition_notes: bool) -> String {
    let mut outside = String::new();
    let mut previous = 0;
    for (start, end) in delimited_segments(title) {
        outside.push_str(&title[previous..start]);
        let inside = &title[start + 1..end - 1];
        let key = bibliographic_match_key(inside);
        if key.split_whitespace().any(|word| is_title_qualifier_word(word) || (keep_edition_notes && matches!(word, "edition" | "ed" | "revised" | "revision" | "unabridged" | "translation" | "translated"))) {
            outside.push(' ');
            outside.push_str(&title[start..end]);
        }
        previous = end;
    }
    outside.push_str(&title[previous..]);
    outside.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::{author_name_match_key, author_names_compatible, same_author};

    #[test]
    fn same_author_ignores_order_accents_and_honorifics() {
        assert!(same_author("Yakobson, Alexander", " ALEXANDER  YAKOBSON "));
        assert!(same_author("García, José", "Jose Garcia"));
        assert!(same_author("Dr John Smith", "John Smith"));
        assert!(!same_author("Alexander Yakobson", "Alexander Smith"));
        assert!(!same_author("John Smith", "John A. Smith"));
        assert!(!same_author("...", "---"));
    }

    #[test]
    fn author_keys_remove_baron_and_adjacent_repeated_name_parts() {
        let source = "Baron Thomas Babington Macaulay Macaulay";
        assert_eq!(author_name_match_key(source), "thomas babington macaulay");
        assert!(same_author(source, "Thomas Babington Macaulay"));
        assert!(same_author("Macaulay Macaulay, Baron Thomas Babington", "Thomas Babington Macaulay"));
        assert!(same_author("Baron Thomas Thomas Babington Macaulay", "Thomas Babington Macaulay"));
        assert!(author_names_compatible(source, "Thomas B. Macaulay"));
        assert!(!author_names_compatible(source, "Thomas James Macaulay"));
    }

    #[test]
    fn author_keys_preserve_nonadjacent_repetitions_and_distinct_name_parts() {
        assert_eq!(author_name_match_key("John Smith John"), "john smith john");
        assert_eq!(author_name_match_key("John Baron Smith"), "john smith");
        assert_eq!(author_name_match_key("John Johnson"), "john johnson");
        assert!(!same_author("John Smith John", "John Smith"));
    }

    #[test]
    fn word_order_is_a_fallback_after_strict_author_matching() {
        for (local, remote) in [("da Vinci Leonardo", "Leonardo da Vinci"), ("the Younger Pliny", "Pliny the Younger"), ("Andrew W. Young", "W. Andrew Young")] {
            assert!(!same_author(local, remote));
            assert!(author_names_compatible(local, remote));
            assert!(author_names_compatible(remote, local));
        }
        assert!(author_names_compatible("John Stuart Mill", "Mill J. S."));
        assert!(author_names_compatible("John S. Smith", "J. Stuart Smith"));
        assert!(author_names_compatible("John A. Smith", "Smith John"));
        for (local, remote) in [("John J. Smith", "James J. Smith"), ("John M. Smith", "John S. Smith"), ("John Smith John", "John Smith"), ("Bernard Shaw", "George Bernard Shaw"), ("John Smith Jr", "John Smith Sr")] {
            assert!(!author_names_compatible(local, remote));
            assert!(!author_names_compatible(remote, local));
        }
    }

    #[test]
    fn author_dates_preserve_names_before_and_after_the_years() {
        for name in ["John M. 1851-1929 Tyler", "John M. (1851–1929) Tyler", "Tyler, John M., 1851—1929", "John M. b. 1851 Tyler", "John M. died 1929 Tyler", "John M. 1851- Tyler"] {
            assert!(same_author(name, "John M. Tyler"), "{name}");
        }
        assert!(same_author("Lillian Moller 1878-1972 Gilbreth", "Lillian Moller Gilbreth"));
        assert!(same_author("Reginald R. b. 1848 Sharpe", "Reginald R. Sharpe"));
        assert_eq!(author_name_match_key("John D. Tyler"), "john d tyler");
        assert!(same_author("John B. 1851-1929 Tyler", "John B. Tyler"));
        assert!(same_author("John D. 1851-1929 Tyler", "John D. Tyler"));
        assert_eq!(author_name_match_key("John 12345 Tyler"), "john 12345 tyler");
    }

    #[test]
    fn honorifics_and_dated_biographies_do_not_depend_on_position() {
        for name in ["Sir Edward Shepherd Creasy", "Edward Sir Shepherd Creasy", "Edward Shepherd Creasy, Sir", "Creasy, Edward Shepherd Sir"] {
            assert!(same_author(name, "Edward Shepherd Creasy"), "{name}");
        }
        for name in ["active 6th century Jordanes", "Jordanes, active 6th century", "Jordanes (flourished ca. 550)", "Jordanes, 6th century A.D."] {
            assert!(same_author(name, "Jordanes"), "{name}");
        }
        assert!(same_author("active 3rd century Shao Liu", "Shao Liu"));
        assert!(same_author("Liu, active 3rd century Shao", "Shao Liu"));
        assert!(same_author("John C. 1851-1929 Tyler", "John C. Tyler"));
        assert_eq!(author_name_match_key("Active Smith"), "active smith");
        assert_eq!(author_name_match_key("John 3rd Tyler"), "john 3rd tyler");
        assert!(!author_names_compatible("John Smith Jr", "John Smith Sr"));
    }

    #[test]
    fn authority_aliases_match_without_merging_unrelated_authors_or_roles() {
        assert!(author_names_compatible("Leo Tolstoy", "Лев Толстой"));
        assert!(author_names_compatible("Lev Nikolayevich Tolstoy", "Лев Толстой"));
        assert!(!author_names_compatible("Alexei Tolstoy", "Лев Толстой"));
        assert!(author_names_compatible("Sun Tzu", "孙武 (Sun Tzu)"));
        assert!(author_names_compatible("H. G. Wells", "Уэллс (H. G. Wells)"));
        assert!(!author_names_compatible("Translator", "Sun Tzu (Translator)"));
        assert!(!author_names_compatible("John Smith Sr", "John Smith (Jr)"));
    }
}
