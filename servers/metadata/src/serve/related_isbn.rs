//! Pure verification policy; callers perform ISBN requests without uploading book bytes.
use metadata_contract::{
    matching::{author_match_keys, matching_author_count, NormalizedTitle},
    Classification, ClassificationEvidence, ClassificationScheme, ClassificationSource, EditionIdentityQuery, EditionIdentityResult, EditionIdentityStatus, RichWorkMetadataMatch,
};

/// All checksum-valid ISBN references identified as a candidate edition of this
/// book at ingestion time (see `book_inspection::evidence::apply_evidence`).
/// These can identify another format of the book; they are not edition identities.
pub fn lookup_candidates(identifiers: &[book_model::Identifier]) -> Vec<String> {
    identifiers.iter().filter(|identifier| identifier.scheme() == &book_model::Scheme::Isbn && identifier.scope() == book_model::Scope::Edition).filter_map(book_model::Identifier::canonical_value).collect()
}

/// A printed ISBN disambiguates returned editions only when their full title and
/// at least one author agrees. A truncated search cannot establish that agreement.
pub fn verified_isbns(query: &EditionIdentityQuery, printed: &[String], result: &EditionIdentityResult) -> Vec<String> {
    if result.query_id != query.query_id || result.candidates_truncated || !matches!(result.status, EditionIdentityStatus::Matched | EditionIdentityStatus::Ambiguous) {
        return Vec::new();
    }
    let title = NormalizedTitle::new(&query.title);
    if title.full.len() < 5 || query.authors.is_empty() || query.authors.iter().any(|a| author_match_keys(a).iter().all(|k| k.split_whitespace().count() < 2)) {
        return Vec::new();
    }
    let records: Vec<_> = result.matched.iter().chain(&result.candidates).collect();
    let relevant = records.iter().filter(|e| printed.contains(&e.canonical_isbn13) && title.matches(&NormalizedTitle::new(&e.full_title()))).collect::<Vec<_>>();
    if relevant.iter().enumerate().any(|(index, e)| relevant[..index].iter().any(|previous| !authors_agree(&previous.authors, &e.authors))) {
        return Vec::new();
    }
    printed
        .iter()
        .filter(|isbn| {
            let editions: Vec<_> = records.iter().filter(|e| &e.canonical_isbn13 == *isbn).collect();
            !editions.is_empty() && editions.iter().all(|e| title.matches(&NormalizedTitle::new(&e.full_title())) && authors_agree(&query.authors, &e.authors))
        })
        .cloned()
        .collect()
}

/// Collect valid classifications from verified records. Transfer classifications only,
/// never a print edition's identifiers, work identity, description or entities.
pub fn classifications(verified: &[String], records: impl Fn(&str) -> Vec<RichWorkMetadataMatch>) -> Vec<Classification> {
    let mut accepted = Vec::new();
    for isbn in verified {
        for record in records(isbn) {
            let mut codes: Vec<_> = record.classifications.into_iter().filter(|c| c.scheme == ClassificationScheme::LibraryOfCongress && metadata_contract::classification_consensus_notation(c.scheme, &c.notation).is_some()).collect();
            for code in &mut codes {
                code.source = ClassificationSource::Work;
                code.evidence.push(ClassificationEvidence {
                    method: "local_bibliographic_print_isbn_verified_title_author".into(),
                    isbn13: isbn.clone(),
                    open_library_work_id: record.open_library_work_id.clone().unwrap_or_default(),
                    open_library_edition_id: record.exact_edition_ids.first().cloned().unwrap_or_default(),
                });
            }
            accepted.extend(codes);
        }
    }
    accepted.sort();
    accepted.dedup();
    accepted
}

pub fn classification_record(classifications: Vec<Classification>) -> RichWorkMetadataMatch {
    RichWorkMetadataMatch { open_library_work_id: None, exact_edition_ids: Vec::new(), classifications, authors: Vec::new(), description: None, popularity: None, subjects: Vec::new() }
}

/// Keep the supporting print ISBN in the persisted subject's provenance.
pub fn subject(code: &Classification) -> Option<book_model::BookSubject> {
    if let Some(evidence) = code.evidence.iter().find(|e| e.method == "local_bibliographic_isbn_reference") {
        return book_model::BookSubject::new(None, &code.notation, format!("inspection:isbn-reference:{}", evidence.isbn13), Some("lcc".into()), Some(code.notation.clone())).ok();
    }
    let evidence = code.evidence.iter().find(|e| e.method == "local_bibliographic_print_isbn_verified_title_author")?;
    book_model::BookSubject::new(None, &code.notation, format!("inspection:verified-print-isbn:{}", evidence.isbn13), Some("lcc".into()), Some(code.notation.clone())).ok()
}

fn authors_agree(local: &[String], remote: &[String]) -> bool {
    matching_author_count(local, remote) > 0
}
