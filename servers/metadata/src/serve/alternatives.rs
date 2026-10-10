//! Alternative readings of a book's title, verified like the stored one.

use metadata_contract::books::TitleReading;
use metadata_contract::EditionIdentityQuery;

pub const MAX_ALTERNATIVE_QUERIES: usize = 4;

/// One query per distinct reading. A reading without authors is also tried
/// with the book's own credits.
pub fn queries(base: &EditionIdentityQuery, readings: &[TitleReading]) -> Vec<EditionIdentityQuery> {
    let mut queries = Vec::new();
    for reading in readings {
        let read_authors = (!reading.authors.is_empty() || reading.surname.is_some()).then(|| reading.authors.clone());
        for authors in read_authors.into_iter().chain(std::iter::once(base.authors.clone())) {
            let mut query = base.clone();
            query.title = reading.title.clone();
            query.authors = authors;
            query.author_surname = reading.surname.clone().filter(|_| query.authors.is_empty());
            query.book_year = query.book_year.or(reading.year);
            let equivalent = |other: &EditionIdentityQuery| {
                metadata_contract::matching::NormalizedTitle::new(&query.title).full == metadata_contract::matching::NormalizedTitle::new(&other.title).full
                    && query.authors == other.authors
                    && query.author_surname == other.author_surname
            };
            if equivalent(base) || queries.iter().any(equivalent) {
                continue;
            }
            query.query_id = format!("{}:alternative:{}", base.query_id, queries.len());
            queries.push(query);
            if queries.len() == MAX_ALTERNATIVE_QUERIES {
                return queries;
            }
        }
    }
    queries
}

/// Apply the same subject-verification paths used by normal title lookup.
pub fn classifications(query: &EditionIdentityQuery, resource_isbns: &[String], result: &metadata_contract::EditionIdentityResult) -> Vec<metadata_contract::Classification> {
    if result.query_id != query.query_id || result.candidates_truncated {
        return vec![];
    }
    let verified = crate::serve::related_isbn::verified_isbns(query, resource_isbns, result);
    let codes = crate::serve::related_isbn::classifications(&verified, |isbn| {
        result.matched.iter().chain(&result.candidates).filter(|m| m.canonical_isbn13 == isbn).map(|m| crate::serve::related_isbn::classification_record(m.classifications.clone())).collect()
    });
    if !codes.is_empty() {
        return codes;
    }
    if let Some(record) = result.authority_subjects.as_ref().and_then(|m| metadata_contract::authority_subjects::classification_record(query, m)) {
        return record.classifications;
    }
    let codes = metadata_contract::subject_consensus::resolved_classifications(query, result);
    if !codes.is_empty() {
        return codes;
    }
    if result.status == metadata_contract::EditionIdentityStatus::Matched {
        if let Some(record) = result.matched.clone().and_then(|m| crate::serve::resolution::classification_only_fallback(query, m)) {
            return record.classifications;
        }
    }
    vec![]
}
