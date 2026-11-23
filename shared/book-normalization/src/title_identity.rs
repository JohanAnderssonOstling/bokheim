//! Normalize ingested bibliographic fields and preserve their original title evidence.

use std::collections::BTreeSet;
use std::path::Path;
use crate::identity_evidence::edition_identity_evidence;
use metadata_contract::books::TitleReading;

pub fn full_title(title: &str, subtitle: Option<&str>) -> String {
    match subtitle {
        Some(subtitle) => format!("{title}: {subtitle}"),
        None => title.to_owned(),
    }
}

/// Reject an author credit only when the complete title matches it.
pub fn unusable_title(title: &str, contributors: &[book_model::Contributor]) -> bool {
    fn key(value: &str) -> String {
        let value = value.trim();
        let value = match value.split_once(',') {
            Some((last, first)) if !first.contains(',') => format!("{first} {last}"),
            _ => value.to_owned(),
        };
        metadata_contract::matching::author_name_match_key(&value)
    }
    let title_key = key(title);
    super::filename_identity::unusable_title(title) || contributors.iter().any(|credit| credit.role_code() == book_model::AUTHOR_MARC_RELATOR_CODE && key(credit.name()) == title_key)
}

fn normalized_author_words(value: &str) -> Vec<String> {
    metadata_contract::matching::author_name_match_key(value).split_whitespace().filter(|word| metadata_contract::matching::author_suffix(word).is_none()).map(str::to_owned).collect()
}

fn filename_title_stem(filename: &str, contributors: &[book_model::Contributor]) -> String {
    let stem = Path::new(filename).file_stem().and_then(|stem| stem.to_str()).unwrap_or(filename);
    let (clean, _) = super::filename_identifiers::clean(stem);
    without_author_credits(crate::identity_evidence::without_leading_bracket_segments(&clean), contributors).to_owned()
}

/// The filename without a leading section that credits the book's own
/// authors, in any of the spellings filenames use for them.
fn without_author_credits<'a>(filename: &'a str, contributors: &[book_model::Contributor]) -> &'a str {
    let mut aliases = BTreeSet::new();
    let mut single_names = BTreeSet::new();
    for credit in contributors.iter().filter(|credit| credit.role_code() == book_model::AUTHOR_MARC_RELATOR_CODE) {
        let name = credit.name();
        let words = normalized_author_words(name);
        if words.len() == 1 {
            single_names.insert(words[0].clone());
        }
        if words.len() < 2 {
            continue;
        }
        aliases.insert(words.concat());
        let given_first = book_model::given_name_first(name).unwrap_or_else(|| name.to_owned());
        let words = normalized_author_words(&given_first);
        aliases.insert(words.concat());
        let (family, given) = words.split_last().unwrap();
        aliases.insert(format!("{}{}", family, given.concat()));
        let initials: String = given.iter().filter_map(|word| word.chars().next()).collect();
        aliases.insert(format!("{family}{initials}"));
        aliases.insert(format!("{initials}{family}"));
    }
    // Only the first spaced hyphen can delimit a filename credit section.
    if let Some((index, dash)) =
        filename.char_indices().find(|(index, character)| matches!(character, '-' | '–' | '—') && filename[..*index].ends_with(char::is_whitespace) && filename[*index + character.len_utf8()..].starts_with(char::is_whitespace))
    {
        let prefix = filename[..index].trim_end();
        // A dangling underscore before the hyphen is an incomplete filename,
        // not evidence that everything before the hyphen is an author section.
        if prefix.ends_with('_') {
            return filename;
        }
        let words = normalized_author_words(prefix);
        let single_match = prefix.len() <= 512
            && prefix.split(['_', ';', ',', '&']).any(|entry| {
                let cleaned = edition_identity_evidence(entry).title;
                let entry_words = normalized_author_words(&cleaned);
                entry_words.len() == 1 && single_names.contains(&entry_words[0])
            });
        let matched = single_match || (prefix.len() <= 512 && (0..words.len()).any(|start| (start + 2..=words.len()).any(|end| aliases.contains(&words[start..end].concat()))));
        let tail = filename[index + dash.len_utf8()..].trim_start();
        if matched && !super::filename_identity::unusable_title(tail) {
            let parsed = crate::identity_evidence::filename_identity_evidence(tail);
            if !super::filename_identity::unusable_title(&parsed.title) {
                return tail;
            }
        }
    }
    let mut rest = filename;
    loop {
        let end = rest
            .char_indices()
            .filter(|(_, c)| c.is_whitespace() || matches!(c, '_' | '-' | '–' | '—' | ',' | '&'))
            .filter(|(i, _)| *i > 0 && *i <= 512)
            .filter_map(|(i, _)| aliases.contains(&normalized_author_words(&rest[..i]).concat()).then_some(i))
            .max();
        let Some(end) = end else {
            return rest;
        };
        let next = rest[end..].trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '_' | '-' | '–' | '—' | ',' | '&'));
        if next.is_empty() {
            return rest;
        }
        rest = next;
    }
}

fn filename_evidence(metadata: &book_model::BookRecord, stem: &str) -> crate::identity_evidence::EditionIdentityEvidence {
    if metadata.subtitle().is_none() && !unusable_title(&metadata.title, &metadata.contributors) {
        crate::identity_evidence::filename_identity_evidence_with_title(stem, &metadata.title)
    } else {
        crate::identity_evidence::filename_identity_evidence(stem)
    }
}

/// Split only author credits; each person keeps their own display name.
/// Author fields from file metadata may hold several names separated by
/// semicolons, a bracketed role or a tool credit such as "calibre". Keeps
/// one credit per name.
fn clean_author_credits(metadata: &mut book_model::BookRecord) -> Result<(), String> {
    use book_model::{Contributor, AUTHOR_MARC_RELATOR_CODE};
    let mut credits = Vec::<Contributor>::new();
    for credit in &metadata.contributors {
        if credit.role_code() != AUTHOR_MARC_RELATOR_CODE {
            credits.push(credit.clone());
            continue;
        }
        let names = credit
            .name()
            .split(';')
            .map(|name| name.split('[').next().unwrap_or(name).trim())
            .filter(|name| !name.is_empty() && !name.to_lowercase().contains("calibre") && !name.contains(".com"));
        for (index, name) in names.enumerate() {
            if credits.iter().any(|known| known.role_code() == AUTHOR_MARC_RELATOR_CODE && known.name() == name) {
                continue;
            }
            let person = if index == 0 && name == credit.name() { Ok(credit.clone()) } else if index == 0 { Contributor::with_id(credit.contributor_id(), name, credit.role_code()) } else { Contributor::new(name, credit.role_code()) }.map_err(|e| e.to_string())?;
            credits.push(person);
        }
    }
    metadata.contributors = credits;
    Ok(())
}

/// An author field written `Family, Given, Family, Given` names several
/// people. Split it only when the filename's author prefix names each of them.
fn split_paired_credits(metadata: &mut book_model::BookRecord, filename: &str) -> Result<(), String> {
    use book_model::{Contributor, AUTHOR_MARC_RELATOR_CODE};
    let stem = Path::new(filename).file_stem().and_then(|s| s.to_str()).unwrap_or(filename);
    let stem = crate::identity_evidence::without_leading_bracket_segments(stem);
    let Some((end, _)) = stem.char_indices().find(|(index, c)| matches!(c, '-' | '–' | '—') && stem[..*index].ends_with(char::is_whitespace) && stem[*index + c.len_utf8()..].starts_with(char::is_whitespace)) else {
        return Ok(());
    };
    let prefix = stem[..end].trim_end();
    if prefix.len() > 512 || prefix.ends_with('_') {
        return Ok(());
    }
    let entries = prefix.split([',', ';', '_', '&']).map(|entry| normalized_author_words(&edition_identity_evidence(entry).title)).collect::<Vec<_>>();
    let mut credits = Vec::new();
    for credit in &metadata.contributors {
        let parts = credit.name().split(',').map(str::trim).collect::<Vec<_>>();
        if credit.role_code() != AUTHOR_MARC_RELATOR_CODE || parts.len() < 4 || parts.len() > 16 || parts.len() % 2 != 0 || parts.iter().any(|part| part.is_empty()) {
            credits.push(credit.clone());
            continue;
        }
        let names = parts.as_chunks::<2>().0.iter().map(|pair| format!("{} {}", pair[1], pair[0])).collect::<Vec<_>>();
        let keys = names.iter().map(|name| normalized_author_words(name)).collect::<Vec<_>>();
        let distinct = keys.iter().collect::<BTreeSet<_>>().len() == keys.len();
        if !distinct || !keys.iter().all(|key| key.len() >= 2 && entries.contains(key)) {
            credits.push(credit.clone());
            continue;
        }
        for (index, name) in names.iter().enumerate() {
            let person = if index == 0 { Contributor::with_id(credit.contributor_id(), name, credit.role_code()) } else { Contributor::new(name, credit.role_code()) }.map_err(|e| e.to_string())?;
            credits.push(person);
        }
    }
    metadata.contributors = credits;
    Ok(())
}

/// Whether the title the filename gives should replace the book's own.
fn filename_title_wins(metadata: &book_model::BookRecord, filename: &str) -> bool {
    let stem = filename_title_stem(filename, &metadata.contributors);
    // Structured archive parsing can discard the identifier-bearing suffix.
    // Validate that resulting candidate below before allowing it to compete.
    if unusable_title(&stem, &metadata.contributors) && !super::filename_identity::contains_long_identifier(&stem) {
        return false;
    }
    let filename_evidence = filename_evidence(metadata, &stem);
    let candidate = &filename_evidence.title;
    if unusable_title(candidate, &metadata.contributors) || super::filename_identity::identifier_shaped_title(candidate) {
        return false;
    }
    let current = full_title(&metadata.title, metadata.subtitle());
    if unusable_title(&current, &metadata.contributors) || super::filename_identity::identifier_shaped_title(&current) {
        return true;
    }
    let current = edition_identity_evidence(&current).title;
    let compact = |title: &str| metadata_contract::matching::bibliographic_match_key(title).replace(' ', "");
    if compact(candidate) == compact(&current) {
        return false;
    }
    // Low-level readers may already have copied/cleaned the filename into the
    // title. Still apply its explicit by-credit syntax when the evidence agrees.
    if filename_evidence.author.is_some() && stem.as_bytes().windows(4).any(|part| part.eq_ignore_ascii_case(b" by ")) && compact(&current) == compact(&edition_identity_evidence(&stem).title) {
        return true;
    }
    edition_rank(candidate) > edition_rank(&current)
}

/// Titles naming an edition ("Second Edition", "3rd ed") or carrying a
/// subtitle rank above those that don't.
fn edition_rank(title: &str) -> (bool, bool) {
    let lower = title.to_lowercase();
    let words = lower.split(|c: char| !c.is_alphanumeric()).filter(|word| !word.is_empty()).collect::<Vec<_>>();
    let edition = words.windows(2).any(|pair| {
        matches!(pair[1], "edition" | "ed")
            && (matches!(pair[0], "first" | "second" | "third" | "fourth" | "fifth" | "sixth" | "seventh" | "eighth" | "ninth" | "tenth" | "revised" | "expanded" | "updated" | "new" | "anniversary")
                || ["st", "nd", "rd", "th"].iter().find_map(|suffix| pair[0].strip_suffix(suffix)).unwrap_or(pair[0]).parse::<u32>().is_ok())
    });
    (edition, title.contains(": "))
}

/// A subtitle ending in "A <author> Omnibus", optionally followed by
/// "- <publisher>", for one of the book's own authors is a collection label.
/// Returns the subtitle without it.
fn without_author_omnibus<'a>(subtitle: &'a str, metadata: &book_model::BookRecord) -> Option<&'a str> {
    let (remaining, mut segment) = subtitle.rsplit_once(": ")?;
    if remaining.trim().is_empty() {
        return None;
    }
    let key = metadata_contract::matching::bibliographic_match_key;
    for (index, _) in segment.char_indices().filter(|(_, c)| matches!(c, '-' | '–' | '—')) {
        let tail = segment[index..].trim_start_matches(['-', '–', '—']).trim();
        if metadata.book.publishers.iter().any(|publisher| key(publisher) == key(tail)) {
            segment = segment[..index].trim_end();
            break;
        }
    }
    let normalized = metadata_contract::matching::author_name_match_key(segment);
    let mut words = normalized.split_whitespace().collect::<Vec<_>>();
    if words.first() != Some(&"a") || words.pop() != Some("omnibus") {
        return None;
    }
    words.remove(0);
    words.retain(|word| !matches!(*word, "jr" | "sr" | "junior" | "senior" | "ii" | "iii" | "iv"));
    if words.len() < 2 {
        return None;
    }
    let surname = *words.last()?;
    if surname.chars().count() < 2 {
        return None;
    }
    let given = &words[..words.len() - 1];
    let matches = metadata.authors().any(|credit| {
        let normalized = metadata_contract::matching::author_name_match_key(credit.name());
        let parts = normalized.split_whitespace().filter(|word| !matches!(*word, "jr" | "sr" | "junior" | "senior" | "ii" | "iii" | "iv")).collect::<Vec<_>>();
        if parts.len() < 2 || !(parts.first() == Some(&surname) || parts.last() == Some(&surname)) {
            return false;
        }
        parts.iter().filter(|word| **word != surname).all(|word| given.iter().any(|other| word == other || ((word.chars().count() == 1 || other.chars().count() == 1) && word.chars().next() == other.chars().next())))
    });
    matches.then_some(remaining.trim_end())
}

/// Prefer usable filename evidence when its parsed title is more descriptive.
pub fn normalize_with_filename(metadata: &mut book_model::BookRecord, filename: &str) -> Result<(), String> {
    let mut next = metadata.clone();
    clean_author_credits(&mut next)?;
    split_paired_credits(&mut next, filename)?;
    let original_stem = Path::new(filename).file_stem().and_then(|stem| stem.to_str()).unwrap_or(filename);
    super::filename_isbn::record(&next.metadata.title, &mut next.metadata.book).map_err(|e| e.to_string())?;
    super::filename_identifiers::record(original_stem, &mut next.book)?;
    if filename_title_wins(&next, filename) {
        let stem = filename_title_stem(filename, &next.contributors);
        let evidence = filename_evidence(&next, &stem);
        next.title = evidence.title;
        if next.contributors.iter().filter(|credit| credit.role_code() == book_model::AUTHOR_MARC_RELATOR_CODE).all(|credit| super::filename_identity::unusable_identity(credit.name())) {
            if let Some(author) = evidence.author {
                let parts = author.split(',').map(str::trim).collect::<Vec<_>>();
                let names = if parts.iter().all(|part| part.split_whitespace().count() >= 2) { parts } else { vec![author.as_str()] };
                let credits = names.into_iter().map(|name| book_model::Contributor::new(name, book_model::AUTHOR_MARC_RELATOR_CODE).map_err(|e| e.to_string())).collect::<Result<Vec<_>, _>>()?;
                next.contributors.retain(|credit| credit.role_code() != book_model::AUTHOR_MARC_RELATOR_CODE);
                next.contributors.extend(credits);
            }
        }
        next.book.year = next.book.year.or(evidence.book_year);
        next.subtitle = None;
    }
    normalize(&mut next)?;
    *metadata = next;
    Ok(())
}

/// Populate the ordinary metadata fields before persistence. Explicit subtitles,
/// usable author credits and an existing year take precedence over inference.
pub fn normalize(metadata: &mut book_model::BookRecord) -> Result<(), String> {
    use book_model::{Contributor, AUTHOR_MARC_RELATOR_CODE};
    clean_author_credits(metadata)?;
    super::filename_isbn::record(&full_title(&metadata.title, metadata.subtitle()), &mut metadata.book).map_err(|e| e.to_string())?;
    if unusable_title(&full_title(&metadata.title, metadata.subtitle()), &metadata.contributors) {
        // Without a usable source filename, do not reinterpret technical junk
        // (for example the colon in a Windows path) as bibliographic fields.
        let original = full_title(&metadata.title, metadata.subtitle());
        if super::filename_identity::kindle_identifier_title(&original)
            || super::filename_identity::standard_title_placeholder(&original)
            || !super::filename_isbn::from_filename(&original).is_empty()
            || original.trim().eq_ignore_ascii_case("true")
            || original.trim().to_ascii_lowercase().starts_with("oup_")
            || !super::filename_identity::unusable_title(&original)
        {
            metadata.title = "Untitled".into();
            metadata.subtitle = None;
        }
        return Ok(());
    }
    let mut next = metadata.clone();
    let explicit_subtitle = next.subtitle.is_some();
    // Normalize matching explicit suffixes first, but parse a fallback subtitle
    // together with its title so credits at the end are extracted once.
    next.normalize_title();
    let parsed_input = if explicit_subtitle { next.title.clone() } else { full_title(&next.title, next.subtitle()) };
    let evidence = edition_identity_evidence(&parsed_input);
    if !evidence.title.trim().is_empty() && evidence.title != parsed_input {
        next.title = evidence.title;
        if !explicit_subtitle {
            next.subtitle = None;
        }
        next.normalize_title();
    }
    if let Some(subtitle) = next.subtitle().and_then(|subtitle| without_author_omnibus(subtitle, &next)).map(str::to_owned) {
        next.subtitle = Some(subtitle);
    }
    if next.contributors.iter().filter(|credit| credit.role_code() == AUTHOR_MARC_RELATOR_CODE).all(|credit| super::filename_identity::unusable_identity(credit.name())) {
        if let Some(author) = evidence.author {
            // Commas between full names delimit credits; surname-first names
            // remain a single credit.
            let parts = author.split(',').map(str::trim).collect::<Vec<_>>();
            let names = if parts.iter().all(|part| part.split_whitespace().count() >= 2) { parts } else { vec![author.as_str()] };
            let inferred = names.into_iter().map(|name| Contributor::new(name, AUTHOR_MARC_RELATOR_CODE).map_err(|e| e.to_string())).collect::<Result<Vec<_>, _>>()?;
            next.contributors.retain(|credit| credit.role_code() != AUTHOR_MARC_RELATOR_CODE);
            next.contributors.extend(inferred);
        }
    }
    next.book.year = next.book.year.or(evidence.book_year);
    *metadata = next;
    Ok(())
}

/// Unconfirmed readings of a book's title and filenames for the metadata
/// server to try: what each filename says on its own, and authors written
/// into a title when the book has no usable credits.
pub fn title_readings(record: &book_model::BookRecord, filenames: &[String]) -> Vec<TitleReading> {
    let uncredited = record.authors().all(|credit| super::filename_identity::unusable_identity(credit.name()));
    let mut readings = Vec::new();
    if uncredited {
        let title = full_title(&record.title, record.subtitle());
        readings.extend(author_prefix_readings(&title));
        readings.extend(surname_reading(&title));
    }
    for filename in filenames {
        let mut parsed = book_model::BookRecord { metadata: book_model::BookDetails { title: String::new(), subtitle: None, contributors: vec![], book: Default::default() }, description: String::new() };
        if normalize_with_filename(&mut parsed, filename).is_ok() {
            let title = full_title(&parsed.title, parsed.subtitle());
            if !super::filename_identity::unusable_title(&title) {
                let authors = parsed.authors().map(|credit| credit.name().to_owned()).collect::<Vec<_>>();
                if uncredited && authors.is_empty() {
                    readings.extend(author_prefix_readings(&title));
                }
                readings.push(TitleReading { title, authors, surname: None, year: parsed.book.year });
            }
        }
        if uncredited {
            let stem = Path::new(filename).file_stem().and_then(|stem| stem.to_str()).unwrap_or(filename);
            readings.extend(surname_reading(stem));
        }
    }
    readings
}

/// `Author - Title`. The server keeps it only when a catalogue record by a
/// compatible author has that title.
fn author_prefix_readings(title: &str) -> Vec<TitleReading> {
    [" - ", " – ", " — "]
        .iter()
        .filter_map(|separator| {
            let (prefix, rest) = title.split_once(separator)?;
            let authors = metadata_contract::matching::author_credits(&[prefix.trim().to_owned()]);
            let title = edition_identity_evidence(&rest.replace('：', ":").replace('_', " ")).title;
            (!authors.is_empty() && metadata_contract::matching::NormalizedTitle::new(&title).full.len() >= 5).then_some(TitleReading { title, authors, surname: None, year: None })
        })
        .collect()
}

/// `Surname_Title_Words`. The server keeps it only when exactly one catalogue
/// record has that title and a full-named author with that surname.
fn surname_reading(title: &str) -> Option<TitleReading> {
    let (surname, rest) = title.trim().split_once('_')?;
    if surname.chars().count() < 2 || !surname.chars().all(char::is_alphabetic) {
        return None;
    }
    let title = edition_identity_evidence(&rest.replace('_', " ")).title;
    (!metadata_contract::matching::bibliographic_match_key(&title).is_empty()).then(|| TitleReading { title, authors: Vec::new(), surname: Some(surname.to_owned()), year: None })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book(title: &str) -> book_model::BookRecord {
        book_model::BookRecord { metadata: book_model::BookDetails { title: title.into(), subtitle: None, contributors: vec![], book: Default::default() }, description: String::new() }
    }


    #[test]
    fn archive_hash_filenames_cannot_override_clean_embedded_titles() {
        for (title, filename) in [
            ("Tysk höst", "Tysk höst -- Dagerman, Stig -- 2009;2010 -- Norstedts -- 9789113031194 -- 1c026c9c84b8c26cfa2e8544ec01cec2 -- Anna’s Archive.epub"),
            (
                "The Well-Educated Mind",
                "The Well-Educated Mind A Guide to the Classical Education -- Bauer, Susan, Wise -- UpdED, 2015;2014 -- W_ W_ Norton & Company -- 9780393253917 -- 928e8c08abc768441bd4965b1aacc4ea -- Anna’s Archive.epub",
            ),
            (
                "Rust Atomics and Locks",
                "Rust atomics and locks _ low-level concurrency in practice -- Mara Bos -- Early release, 2022 -- O'Reilly Media, Incorporated -- 9781098119386 -- 8e1fcd5d26a273d7019692e7729148ac -- Anna’s Archive.epub",
            ),
            ("Effective Rust", "Effective Rust_ 35 Specific Ways to Improve Your Rust Code -- David Drysdale -- 1, PS, 2024 -- O'Reilly Media, Incorporated -- 9781098151409 -- 2b9295592d125afeba4fe5e25e3fc25a -- Anna’s Archive.epub"),
            (
                "An Empire of Wealth",
                "An Empire of Wealth _ The Epic History of American Economic -- John Steele Gordon -- Open Road Integrated Media, Inc_, Pymble, NSW, 2008 -- isbn13 9780060505127 -- 4193ec78914db5b527a5f4dd87dd0512 -- Anna’s Archive.epub",
            ),
            ("The Reckoning", "The Reckoning -- David Halberstam -- Open Road Integrated Media, Inc_, New York, 2012 -- Open Road Integrated Media, Inc_ -- isbn13 9781453286104 -- 00ed60f6f9dc1a7d8717b9329e63df39 -- Anna’s Archive.epub"),
        ] {
            let mut metadata = book(title);
            normalize_with_filename(&mut metadata, filename).unwrap();
            assert_eq!(metadata.title, title);
            assert_eq!(metadata.subtitle(), None);
        }
    }

    #[test]
    fn filename_identifiers_are_preserved_without_vetoing_readable_titles() {
        for (filename, expected, isbn, doi) in [
            (
                "Steven S. Skiena (auth.) - Algorithm Design Manual, The (2012_2008, Springer-Verlag London) [10.1007_978-1-84800-070-4] - libgen.li.pdf",
                "Steven S. Skiena - Algorithm Design Manual, The",
                "9781848000704",
                "10.1007/978-1-84800-070-4",
            ),
            (
                "John J. Mearsheimer_ Sebastian Rosato - How States Think_ The Rationality of Foreign Policy (2023, Yale University Press) [10.12987_9780300274967] - libgen.li.pdf",
                "John J. Mearsheimer Sebastian Rosato - How States Think The Rationality of Foreign Policy",
                "9780300274967",
                "10.12987/9780300274967",
            ),
        ] {
            let mut metadata = book("Untitled");
            normalize_with_filename(&mut metadata, filename).unwrap();
            assert_eq!(metadata.title, expected);
            assert!(metadata.contributors.is_empty());
            for identifier in [isbn, doi] {
                assert!(metadata.book.identifiers.iter().any(|id| id.canonical_value().as_deref() == Some(identifier)));
            }
            let before = metadata.clone();
            normalize_with_filename(&mut metadata, filename).unwrap();
            assert_eq!(metadata, before);
        }
        for filename in ["Readable Title ISBN 978-1-84800-070-4.pdf", "Readable Title [10.1000_example].pdf"] {
            let mut metadata = book("Untitled");
            normalize_with_filename(&mut metadata, filename).unwrap();
            assert_eq!(metadata.title, "Readable Title");
        }
        for filename in ["9781848000704.pdf", "[10.1000_example].pdf"] {
            let mut metadata = book("Untitled");
            normalize_with_filename(&mut metadata, filename).unwrap();
            assert_eq!(metadata.title, "Untitled");
        }
    }


    #[test]
    fn author_only_and_publisher_placeholders_use_filename() {
        for title in ["Quentin Skinner", "Skinner, Quentin", " QUENTIN  SKINNER ", "oup_6555", "OUP_5783", "true", " TRUE ", "ebook", "OMB_ebook"] {
            let mut metadata = book(title);
            metadata.contributors.push(book_model::Contributor::new("Quentin Skinner", book_model::AUTHOR_MARC_RELATOR_CODE).unwrap());
            normalize_with_filename(&mut metadata, "Machiavelli.epub").unwrap();
            assert_eq!(metadata.title, "Machiavelli", "{title}");
        }
        for title in ["Machiavelli", "Quentin Skinner: A Biography", "True Grit", "OUP Publishing", "oup_12_notes"] {
            let mut metadata = book(title);
            metadata.contributors.push(book_model::Contributor::new("Quentin Skinner", book_model::AUTHOR_MARC_RELATOR_CODE).unwrap());
            assert!(!unusable_title(title, &metadata.contributors), "{title}");
        }
        let mut metadata = book("true");
        normalize_with_filename(&mut metadata, "oup_6555.mobi").unwrap();
        assert_eq!(metadata.title, "Untitled");
        let narrator = book_model::Contributor::new("Quentin Skinner", book_model::CONTRIBUTOR_MARC_RELATOR_CODE).unwrap();
        assert!(!unusable_title("Quentin Skinner", &[narrator]));
    }


}
