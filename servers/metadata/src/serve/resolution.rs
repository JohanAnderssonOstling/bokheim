//! Pure subject and author resolution for enrichment results.
use book_model::BookSubject;
use metadata_contract::matching::author_match_keys;
use metadata_contract::{ClassificationScheme, ClassificationSource, EditionIdentityQuery, LookupStatus, RichWorkMetadataMatch};
use std::collections::{HashMap, HashSet};


pub fn merge_rich_matches(matches: &[RichWorkMetadataMatch]) -> RichWorkMetadataMatch {
    metadata_contract::merge_rich_work_matches(matches, classification_similarity_keys)
}

pub fn classification_similarity_keys(scheme: ClassificationScheme, notation: &str) -> Vec<String> {
    let system_id = match scheme {
        ClassificationScheme::LibraryOfCongress => subject_projection::LCC_SYSTEM_ID,
        ClassificationScheme::Bisac => subject_projection::BISAC_SYSTEM_ID,
        _ => return Vec::new(),
    };
    subject_projection::unified_subject_similarity_keys(system_id, notation)
}

pub fn rich_subjects(matched: &RichWorkMetadataMatch) -> Result<Vec<BookSubject>, String> {
    let mut subjects = Vec::new();
    for heading in &matched.subjects {
        let name = heading.components.join(" / ");
        subjects.push(BookSubject::new(heading.authority_uri.clone(), name, &heading.source, Some(heading.authority.clone()), None).map_err(|error| error.to_string())?);
    }
    for classification in &matched.classifications {
        if classification.evidence.iter().any(|e| e.method.starts_with("librarything_isbn:")) {
            let authority = match classification.scheme {
                ClassificationScheme::LibraryOfCongress => "lcc",
                ClassificationScheme::DeweyDecimal => "ddc",
                ClassificationScheme::Bisac => "bisac",
            };
            subjects.push(BookSubject::new(None, &classification.notation, "librarything:metadata:isbn", Some(authority.into()), Some(classification.notation.clone())).map_err(|error| error.to_string())?);
            continue;
        }
        if classification.evidence.iter().any(|e| e.method.starts_with(metadata_contract::authority_subjects::METHOD_PREFIX)) {
            subjects.push(BookSubject::new(None, &classification.notation, "library_of_congress:metadata:title-author", Some("lcc".into()), Some(classification.notation.clone())).map_err(|error| error.to_string())?);
            continue;
        }
        if classification.evidence.iter().any(|e| e.method == metadata_contract::subject_consensus::METHOD) {
            subjects.push(BookSubject::new(None, &classification.notation, "metadata:shared-edition-classification", Some("lcc".into()), Some(classification.notation.clone())).map_err(|error| error.to_string())?);
            continue;
        }
        if classification.evidence.iter().any(|e| e.method == metadata_contract::year_consensus::METHOD) {
            subjects.push(BookSubject::new(None, &classification.notation, "metadata:year-only-edition-consensus", Some("lcc".into()), Some(classification.notation.clone())).map_err(|error| error.to_string())?);
            continue;
        }
        if let Some(subject) = crate::serve::related_isbn::subject(classification) {
            subjects.push(subject);
            continue;
        }
        let authority = match classification.scheme {
            ClassificationScheme::LibraryOfCongress => "lcc",
            ClassificationScheme::Bisac => "bisac",
            _ => continue,
        };
        let source = match (matched.open_library_work_id.is_some(), classification.source) {
            (true, ClassificationSource::ExactEdition) => "openlibrary:metadata:exact-edition",
            (true, ClassificationSource::Work) => "openlibrary:metadata:work",
            (false, ClassificationSource::ExactEdition) => "metadata:exact-edition",
            (false, ClassificationSource::Work) => "metadata:work",
        };
        subjects.push(BookSubject::new(None, &classification.notation, source, Some(authority.to_owned()), Some(classification.notation.clone())).map_err(|error| error.to_string())?);
    }
    subjects.sort_by(|left, right| (left.authority(), left.name(), left.code()).cmp(&(right.authority(), right.name(), right.code())));
    subjects.dedup();
    Ok(subjects)
}

/// The authority-qualified key every device hashes into the same author id:
/// the Open Library author, else the Wikidata item. The canonical forms are
/// part of that id, so they must never change.
pub fn external_author_identity(author: &metadata_contract::AuthorMetadata) -> Option<String> {
    canonical_external_id(&author.open_library_author_id, "/authors/", "OL", "A").map(|id| format!("open_library:{id}")).or_else(|| {
        let id = author.identifiers.iter().find(|id| id.authority == "wikidata")?;
        canonical_external_id(&id.value, "", "Q", "").map(|id| format!("wikidata:{id}"))
    })
}

/// `{prefix}{digits}{suffix}`, case-insensitive, without leading zeros.
fn canonical_external_id(value: &str, path: &str, prefix: &str, suffix: &str) -> Option<String> {
    let value = value.trim();
    if value.len() > 256 {
        return None;
    }
    let value = value.strip_prefix(path).unwrap_or(value).to_ascii_uppercase();
    let digits = value.strip_prefix(prefix)?.strip_suffix(suffix)?.trim_start_matches('0');
    (!digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())).then(|| format!("{prefix}{digits}{suffix}"))
}

#[derive(PartialEq, Eq)]
pub(crate) struct AuthorResolution {
    pub(crate) position: usize,
    pub(crate) name: String,
    pub(crate) external_id: String,
}

pub(crate) fn authoritative_author_resolutions(result: &metadata_contract::RichIsbnMetadataResult) -> Option<Vec<AuthorResolution>> {
    if result.status != LookupStatus::Matched || result.matches.len() != 1 {
        return None;
    }
    let remote = &result.matches[0].authors;
    if remote.is_empty() {
        return None;
    }
    let mut names = HashMap::new();
    let mut ids = HashSet::new();
    let mut authors = Vec::new();
    for author in remote {
        let name = author.name.as_deref()?;
        let keys = author_match_keys(name);
        if keys.is_empty() {
            return None;
        }
        let id = external_author_identity(author)?;
        for key in keys {
            if names.insert(key, author.identity_key()).is_some_and(|previous| previous != author.identity_key()) {
                return None;
            }
        }
        if ids.insert(author.identity_key()) {
            authors.push(AuthorResolution { position: authors.len(), name: name.to_owned(), external_id: id });
        }
    }
    Some(authors)
}

pub(crate) fn match_author_resolutions(credits: &[(usize, &str)], remote: &[metadata_contract::AuthorMetadata]) -> Vec<AuthorResolution> {
    let mut remote_by_name = HashMap::<String, Vec<&metadata_contract::AuthorMetadata>>::new();
    for author in remote {
        for key in author.name.as_deref().map(author_match_keys).unwrap_or_default() {
            remote_by_name.entry(key).or_default().push(author);
        }
    }
    let mut used = HashSet::new();
    let mut resolutions = Vec::new();
    for &(position, credit_name) in credits {
        let mut matches = Vec::<&metadata_contract::AuthorMetadata>::new();
        for author in author_match_keys(credit_name).iter().filter_map(|key| remote_by_name.get(key)).flatten().copied() {
            if !matches.iter().any(|known| known.identity_key() == author.identity_key()) {
                matches.push(author);
            }
        }
        let &[matched] = &matches[..] else { continue };
        if !used.insert(matched.identity_key()) {
            continue;
        }
        let Some(external_id) = external_author_identity(matched) else { continue };
        resolutions.push(AuthorResolution { position, name: matched.name.clone().unwrap_or_else(|| credit_name.to_owned()), external_id });
    }
    resolutions
}

pub fn classification_only_fallback(query: &EditionIdentityQuery, matched: metadata_contract::EditionIdentityMatch) -> Option<RichWorkMetadataMatch> {
    use metadata_contract::matching::matching_author_count;
    if matching_author_count(&query.authors, &matched.authors) == 0 {
        return None;
    }
    let mut classifications = matched.classifications;
    classifications.retain(|c| metadata_contract::classification_consensus_notation(c.scheme, &c.notation).is_some());
    if classifications.is_empty() {
        return None;
    }
    for classification in &mut classifications {
        classification.source = ClassificationSource::Work;
        classification.evidence.push(metadata_contract::ClassificationEvidence {
            method: "local_title_author_after_isbn_miss".into(),
            isbn13: matched.canonical_isbn13.clone(),
            open_library_work_id: matched.open_library_work_id.clone().unwrap_or_default(),
            open_library_edition_id: matched.open_library_edition_id.clone(),
        });
    }
    Some(RichWorkMetadataMatch { open_library_work_id: None, exact_edition_ids: Vec::new(), classifications, authors: Vec::new(), description: None, subjects: Vec::new() })
}
