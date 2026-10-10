//! `/v3/books`: the whole enrichment pipeline for books described by their
//! file facts. The client sends what each file says and stores the record this
//! returns; every lookup order and merge rule lives here.
//!
//! Ported from the client workers (`library-backend/src/library/enrichment`)
//! and the `apply_*` rules in `library-database/src/enrichment`.
use crate::query::run_database;
use crate::{MetadataError, MetadataService};
use crate::serve::resolution::{authoritative_author_resolutions, classification_only_fallback, match_author_resolutions, merge_rich_matches, rich_subjects};
use crate::serve::{alternatives, related_isbn};
use book_model::{AgentId, BookRecord, BookSubject, BookTocEntry, Contributor, Identifier, Scheme, Scope, AUTHOR_MARC_RELATOR_CODE};
use metadata_contract::audible::{LookupRequest, LookupResponse, LookupStatus as AudibleStatus};
use metadata_contract::books::{BookEnrichment, BookEnrichmentRequest, BookEnrichmentResponse, BookEnrichmentStatus, BookFacts, TitleReading, MAX_BOOKS_PER_REQUEST};
use metadata_contract::{
    authority_subjects, subject_consensus, EditionClassificationConsensus, EditionIdentityQuery, EditionIdentityResult, EditionIdentityStatus, LookupStatus, RichIsbnMetadataResult, RichWorkMetadataMatch,
    MAX_ISBNS_PER_REQUEST,
};
use std::collections::{BTreeSet, HashMap};

/// Bump whenever a lookup or merge rule below changes; clients ask again.
pub const POLICY: &str = "books_v4";

// A work union larger than this is replaced by edition consensus when the
// consensus is smaller.
const MAX_DIRECT_CLASSIFICATIONS_WITHOUT_CONSENSUS: usize = 12;
// Subject sources an edition consensus may replace.
const REPLACEABLE_SUBJECT_SOURCES: [&str; 3] = ["dc:subject", "openlibrary:metadata:sibling-edition", "openlibrary:metadata:work"];

/// ISBN lookups for one book, keyed by requested and canonical ISBN.
type Lookups = HashMap<String, RichIsbnMetadataResult>;

struct AudibleRecording {
    asin: String,
    region: String,
}

struct AudibleEnrichment {
    toc: Option<Vec<BookTocEntry>>,
    recording: AudibleRecording,
}

impl MetadataService {
    pub(crate) async fn enrich_books(&self, request: BookEnrichmentRequest) -> Result<BookEnrichmentResponse, MetadataError> {
        if request.books.is_empty() || request.books.len() > MAX_BOOKS_PER_REQUEST {
            return Err(MetadataError(format!("between 1 and {MAX_BOOKS_PER_REQUEST} books are required")));
        }
        // Books wait on independent network fallbacks; database work is
        // already bounded by the query slots.
        let results = futures_util::future::join_all(request.books.into_iter().map(|facts| async move {
            let key = facts.key.clone();
            match self.enrich_book(facts).await {
                Ok(result) => result,
                Err(error) => BookEnrichment { key, status: BookEnrichmentStatus::Failed, record: None, toc: None, cover: None, detail: error.to_string() },
            }
        }))
        .await;
        Ok(BookEnrichmentResponse { policy: POLICY.to_owned(), results })
    }

    async fn enrich_book(&self, facts: BookFacts) -> Result<BookEnrichment, MetadataError> {
        let mut record = facts.record.clone();
        record.description.clear();
        record.book.infer_embedded_subject_isbns().map_err(|error| MetadataError(error.to_string()))?;
        let mut lookups = Lookups::new();
        let mut found = false;

        // Subjects: the book's ISBNs, then a title/author match, then any ISBN
        // that match found. A resolved LCC ends the search.
        let mut checked = BTreeSet::new();
        found |= self.classify_new_isbns(&mut record, &mut checked, &mut lookups).await?;
        if !has_resolved_lcc(&record.book) && !record.title.trim().is_empty() {
            found |= self.apply_edition_identity(&mut record, &facts.readings).await?;
            found |= self.classify_new_isbns(&mut record, &mut checked, &mut lookups).await?;
        }

        let isbns = canonical_isbns(&record);
        if !isbns.is_empty() {
            self.lookup_isbns(&isbns, &mut lookups).await?;
            found |= apply_authors(&mut record, &isbns, &lookups)?;
            if let Some(description) = unique_description(&isbns, &lookups) {
                record.description = description;
                found = true;
            }
        }

        let mut toc = None;
        let mut cover = None;
        if let Some(request) = facts.recording.filter(|_| facts.audiobook) {
            let response = audible_lookup(request).await;
            match response.status {
                AudibleStatus::Supported => {
                    let enrichment = apply_audible(&mut record, &response)?;
                    toc = enrichment.toc;
                    cover = self.resolve_audible_cover(&enrichment.recording.asin, &enrichment.recording.region).await?;
                    found = true;
                }
                AudibleStatus::Unavailable => return Err(MetadataError(format!("audiobook lookup unavailable: {}", response.detail))),
                AudibleStatus::NoMatch | AudibleStatus::Ambiguous => {}
            }
        }
        if cover.is_none() {
            let isbns = canonical_isbns(&record);
            if !isbns.is_empty() {
                cover = self.resolve_isbn_cover(&isbns).await?;
            }
        }

        found |= cover.is_some();
        let status = if found { BookEnrichmentStatus::Found } else { BookEnrichmentStatus::NoMatch };
        Ok(BookEnrichment { key: facts.key, status, record: found.then_some(record), toc, cover, detail: String::new() })
    }

    /// Looks up every ISBN not already in `lookups`. An ISBN the lookup
    /// omits fails the book, so the client asks again.
    async fn lookup_isbns(&self, isbns: &[String], lookups: &mut Lookups) -> Result<(), MetadataError> {
        let missing = isbns.iter().filter(|isbn| !lookups.contains_key(*isbn)).cloned().collect::<BTreeSet<_>>().into_iter().collect::<Vec<_>>();
        for chunk in missing.chunks(MAX_ISBNS_PER_REQUEST) {
            let request = chunk.to_vec();
            let response = run_database(self.clone(), move |service| service.enrich_rich(request)).await.map_err(|error| MetadataError(format!("lookup worker failed: {error}")))??;
            for result in response {
                let requested = book_model::canonical_isbn(&result.requested_isbn).unwrap_or_else(|| result.requested_isbn.clone());
                if let Some(isbn13) = result.canonical_isbn13.as_ref().filter(|isbn13| isbn13.as_str() != requested.as_str()) {
                    lookups.insert(isbn13.clone(), result.clone());
                }
                lookups.insert(requested, result);
            }
        }
        if let Some(omitted) = isbns.iter().find(|isbn| !lookups.contains_key(*isbn)) {
            return Err(MetadataError(format!("metadata lookup omitted ISBN {omitted}")));
        }
        Ok(())
    }

    /// Classifies the book by its ISBNs not yet in `checked`: printed reference
    /// ISBNs first, then its own. Does nothing once an LCC resolves.
    async fn classify_new_isbns(&self, record: &mut BookRecord, checked: &mut BTreeSet<String>, lookups: &mut Lookups) -> Result<bool, MetadataError> {
        if has_resolved_lcc(&record.book) {
            return Ok(false);
        }
        let mut isbns = related_isbn::lookup_candidates(&record.book.identifiers);
        isbns.extend(record.book.identifiers.iter().filter(|id| id.scheme() == &Scheme::Isbn).filter_map(|id| id.canonical_value().as_deref().and_then(book_model::from_metadata_value)));
        isbns.retain(|isbn| isbn_key(isbn).is_some_and(|key| checked.insert(key)));
        if isbns.is_empty() {
            return Ok(false);
        }
        self.apply_isbn_classifications(record, &isbns, lookups).await
    }

    /// Classifications from the book's ISBNs. Printed reference ISBNs
    /// contribute only their classifications; other alternatives count only
    /// where two editions agree.
    async fn apply_isbn_classifications(&self, record: &mut BookRecord, isbns: &[String], lookups: &mut Lookups) -> Result<bool, MetadataError> {
        self.lookup_isbns(isbns, lookups).await?;
        let exact_isbns = record.book.identifiers.iter().filter(|id| id.scheme() == &Scheme::Isbn).filter_map(|id| book_model::from_metadata_value(id.value())).collect::<BTreeSet<_>>();
        let reference_isbns = related_isbn::lookup_candidates(&record.book.identifiers).into_iter().collect::<BTreeSet<_>>();
        let mut consensus = EditionClassificationConsensus::default();
        let mut matched_records = Vec::new();
        for isbn in isbns {
            let Some(result) = lookups.get(isbn) else { continue };
            if result.matches.is_empty() || !matches!(result.status, LookupStatus::Matched | LookupStatus::Ambiguous) {
                continue;
            }
            let mut matches = result.matches.clone();
            if reference_isbns.contains(isbn) && !exact_isbns.contains(isbn) {
                matches = vec![related_isbn::classification_record({
                    let records = matches;
                    let mut codes = related_isbn::classifications(&[isbn.to_owned()], |_| records.clone());
                    for code in &mut codes {
                        for evidence in &mut code.evidence {
                            if evidence.method == "local_bibliographic_print_isbn_verified_title_author" {
                                evidence.method = "local_bibliographic_isbn_reference".into();
                            }
                        }
                    }
                    codes
                })];
            } else if !exact_isbns.contains(isbn) {
                consensus.record_alternative_matches(&mut matches, isbn);
            }
            matched_records.extend(matches);
        }
        let alternative = consensus.classifications();
        let original_has_classifications = !alternative.is_empty() || matched_records.iter().any(|matched| !matched.classifications.is_empty());
        if matched_records.is_empty() {
            return Ok(false);
        }
        let mut matched = merge_rich_matches(&matched_records);
        matched.classifications.extend(alternative);
        matched.classifications.sort();
        matched.classifications.dedup();
        let classification_count = record.book.subjects.len();
        let replace = original_has_classifications && classification_count > MAX_DIRECT_CLASSIFICATIONS_WITHOUT_CONSENSUS && matched.classifications.len() < classification_count;
        if replace {
            let replaceable = record
                .book
                .subjects
                .iter()
                .filter(|subject| subject.authority().is_some_and(|authority| authority.eq_ignore_ascii_case("lcc")) && REPLACEABLE_SUBJECT_SOURCES.contains(&subject.source()))
                .map(subject_key)
                .collect::<BTreeSet<_>>();
            record.book.subjects.retain(|subject| !replaceable.contains(&subject_key(subject)));
        }
        add_subjects(record, rich_subjects(&matched).map_err(MetadataError)?);
        Ok(true)
    }

    /// Title/author edition matching after the book's ISBNs found no subject.
    /// Adds an ISBN only when the evidence names one exact edition and the
    /// book has none.
    async fn apply_edition_identity(&self, record: &mut BookRecord, readings: &[TitleReading]) -> Result<bool, MetadataError> {
        let resource_isbns = canonical_isbns(record);
        let base = EditionIdentityQuery {
            query_id: "0".to_owned(),
            title: record.title.clone(),
            authors: record.authors().map(|author| author.name().to_owned()).collect(),
            author_surname: None,
            publishers: record.book.publishers.clone(),
            book_year: record.book.year,
        };
        let alternative_queries = alternatives::queries(&base, readings);
        let request = std::iter::once(base.clone()).chain(alternative_queries.iter().cloned()).collect();
        let response = run_database(self.clone(), move |service| service.resolve_editions(request)).await.map_err(|error| MetadataError(format!("lookup worker failed: {error}")))??;
        let results = response.into_iter().map(|result| (result.query_id.clone(), result)).collect::<HashMap<_, _>>();

        let supported = |query: &EditionIdentityQuery, result: &EditionIdentityResult| {
            !alternatives::classifications(query, &resource_isbns, result)
                .iter()
                .flat_map(|classification| match classification.scheme {
                    metadata_contract::ClassificationScheme::LibraryOfCongress => subject_projection::unified_subject_paths(subject_projection::LCC_SYSTEM_ID, &classification.notation),
                    metadata_contract::ClassificationScheme::Bisac => subject_projection::unified_subject_paths(subject_projection::BISAC_SYSTEM_ID, &classification.notation),
                    _ => Vec::new(),
                })
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
                .is_empty()
        };
        let mut result = results.get(&base.query_id);
        let mut query = base.clone();
        let mut used_alternative = false;
        if !result.is_some_and(|result| supported(&query, result)) {
            let missing_alternative = alternative_queries.iter().any(|alternative| !results.contains_key(&alternative.query_id));
            for alternative in &alternative_queries {
                if let Some(found) = results.get(&alternative.query_id).filter(|found| supported(alternative, found)) {
                    query = alternative.clone();
                    result = Some(found);
                    used_alternative = true;
                    break;
                }
            }
            if !used_alternative && missing_alternative {
                result = None;
            }
        }
        let Some(result) = result else { return Err(MetadataError("metadata response omitted the edition query".to_owned())) };

        let verified = related_isbn::verified_isbns(&query, &resource_isbns, result);
        let codes = related_isbn::classifications(&verified, |isbn| {
            result.matched.iter().chain(&result.candidates).filter(|record| record.canonical_isbn13 == isbn).map(|record| related_isbn::classification_record(record.classifications.clone())).collect()
        });
        if !codes.is_empty() {
            add_classification_subjects(record, &related_isbn::classification_record(codes))?;
            return Ok(true);
        }
        if let Some(matched) = result.authority_subjects.as_ref().and_then(|matched| authority_subjects::classification_record(&query, matched)) {
            add_classification_subjects(record, &matched)?;
            return Ok(true);
        }
        let agreed = subject_consensus::resolved_classifications(&query, result);
        if !agreed.is_empty() {
            add_classification_subjects(record, &related_isbn::classification_record(agreed))?;
            return Ok(true);
        }
        let mut changed = false;
        if let Some(fallback) = result.matched.clone().filter(|_| result.status == EditionIdentityStatus::Matched).and_then(|matched| classification_only_fallback(&query, matched)) {
            add_classification_subjects(record, &fallback)?;
            changed = true;
        }
        if result.status == EditionIdentityStatus::Matched && !used_alternative {
            if let Some(matched) = &result.matched {
                changed |= add_isbn(record, &matched.canonical_isbn13)?;
            }
        }
        Ok(changed)
    }
}

async fn audible_lookup(request: LookupRequest) -> LookupResponse {
    crate::serve::audible::client().lookup(request).await
}

/// Identifiers, publisher and chapters from the selected recording. The
/// file's own identifiers of a scheme and its publishers always win.
fn apply_audible(record: &mut BookRecord, response: &LookupResponse) -> Result<AudibleEnrichment, MetadataError> {
    let candidate = response
        .candidates
        .iter()
        .find(|candidate| Some(&candidate.asin) == response.selected_asin.as_ref() && Some(&candidate.region) == response.selected_region.as_ref())
        .ok_or_else(|| MetadataError("supported audiobook response has no selected recording".to_owned()))?;
    for (scheme, value) in std::iter::once((Scheme::Asin, &candidate.asin)).chain(candidate.isbns.iter().map(|isbn| (Scheme::Isbn, isbn))) {
        if record.book.identifiers.iter().any(|identifier| identifier.scheme() == &scheme) {
            continue;
        }
        if let Ok(identifier) = Identifier::new(value, scheme, Scope::Edition) {
            record.book.identifiers.push(identifier);
        }
    }
    if record.book.publishers.is_empty() {
        if let Some(name) = candidate.publisher.as_deref().filter(|name| !name.trim().is_empty()) {
            record.book.publishers.push(name.trim().to_owned());
        }
    }
    if record.description.is_empty() {
        if let Some(description) = candidate.description.as_deref().filter(|text| !text.trim().is_empty()) {
            record.description = description.to_owned();
        }
    }
    let toc = response.chapter_plan.as_ref().map(|plan| plan.chapters.iter().map(|chapter| BookTocEntry { title: chapter.title.clone(), target: book_model::audiobook_toc_target(chapter.start_ms), children: Vec::new() }).collect());
    let recording = AudibleRecording { asin: candidate.asin.clone(), region: candidate.region.clone() };
    Ok(AudibleEnrichment { toc, recording })
}

/// Authority identities for the book's author credits. When every ISBN
/// record agrees on one authoritative author list it replaces the author
/// credits; otherwise credits whose names match uniquely gain an identity.
fn apply_authors(record: &mut BookRecord, isbns: &[String], lookups: &Lookups) -> Result<bool, MetadataError> {
    let available = isbns.iter().filter_map(|isbn| lookups.get(isbn)).collect::<Vec<_>>();
    let authoritative = available.iter().filter_map(|result| authoritative_author_resolutions(result)).collect::<Vec<_>>();
    if let Some(authors) = authoritative.first() {
        if authoritative.iter().any(|other| other != authors) {
            return Ok(false);
        }
        let mut contributors = Vec::with_capacity(record.contributors.len());
        for author in authors {
            contributors.push(Contributor::with_id(author_id(&author.external_id), &author.name, AUTHOR_MARC_RELATOR_CODE).map_err(|error| MetadataError(error.to_string()))?);
        }
        contributors.extend(record.contributors.iter().filter(|contributor| contributor.role_code() != AUTHOR_MARC_RELATOR_CODE).cloned());
        let changed = contributors != record.contributors;
        record.contributors = contributors;
        return Ok(changed);
    }
    let Some(result) = available.iter().copied().find(|result| !result.matches.is_empty()) else { return Ok(false) };
    if matches!(result.status, LookupStatus::NoMatch | LookupStatus::InvalidIsbn) {
        return Ok(false);
    }
    let merged = merge_rich_matches(&result.matches);
    let credits = record.contributors.iter().enumerate().filter(|(_, contributor)| contributor.role_code() == AUTHOR_MARC_RELATOR_CODE).map(|(position, contributor)| (position, contributor.name())).collect::<Vec<_>>();
    let resolutions = match_author_resolutions(&credits, &merged.authors);
    let mut changed = false;
    for resolution in resolutions {
        let credit = &record.contributors[resolution.position];
        let resolved = Contributor::with_id(author_id(&resolution.external_id), credit.name(), credit.role_code()).map_err(|error| MetadataError(error.to_string()))?;
        changed |= resolved != *credit;
        record.contributors[resolution.position] = resolved;
    }
    Ok(changed)
}

/// Every device derives the same identity for an authority-resolved author,
/// from the key `external_author_identity` produced.
fn author_id(external_id: &str) -> AgentId {
    book_model::agent_id_from_migration_seed(format!("external-author:{external_id}").as_bytes())
}

/// A description only when every record for the book's ISBNs agrees.
fn unique_description(isbns: &[String], lookups: &Lookups) -> Option<String> {
    let descriptions = isbns.iter().filter_map(|isbn| lookups.get(isbn)).flat_map(|result| &result.matches).filter_map(|matched| matched.description.as_deref()).map(str::trim).filter(|text| !text.is_empty()).collect::<BTreeSet<_>>();
    (descriptions.len() == 1).then(|| descriptions.into_iter().next().expect("one description").to_owned())
}

/// A subject search is finished once an LCC code resolves in the taxonomy.
/// BISAC and unresolvable LCC codes never end it.
fn has_resolved_lcc(metadata: &book_model::BookMetadata) -> bool {
    metadata.subjects.iter().any(|s| s.authority().is_some_and(|a| a.eq_ignore_ascii_case("lcc")) && s.code().is_some_and(|c| !subject_projection::unified_subject_paths(subject_projection::LCC_SYSTEM_ID, c).is_empty()))
}

/// Every ISBN as its ISBN-13, so both forms of one edition compare equal.
fn isbn_key(value: &str) -> Option<String> {
    let isbn = book_model::from_metadata_value(value)?;
    if isbn.len() == 13 {
        return Some(isbn);
    }
    let mut isbn13 = format!("978{}", &isbn[..9]);
    let sum: u32 = isbn13.bytes().enumerate().map(|(i, c)| u32::from(c - b'0') * if i % 2 == 0 { 1 } else { 3 }).sum();
    isbn13.push(char::from(b'0' + ((10 - sum % 10) % 10) as u8));
    Some(isbn13)
}

fn canonical_isbns(record: &BookRecord) -> Vec<String> {
    record.book.identifiers.iter().filter(|id| id.scheme() == &Scheme::Isbn).filter_map(|id| id.canonical_value()).collect::<BTreeSet<_>>().into_iter().collect()
}

/// Records an edition ISBN unless the book already has one.
fn add_isbn(record: &mut BookRecord, isbn: &str) -> Result<bool, MetadataError> {
    if record.book.identifiers.iter().any(|id| id.scheme() == &Scheme::Isbn && id.canonical_value().is_some()) {
        return Ok(false);
    }
    record.book.identifiers.push(Identifier::new(isbn, Scheme::Isbn, Scope::Edition).map_err(|error| MetadataError(error.to_string()))?);
    Ok(true)
}

fn add_classification_subjects(record: &mut BookRecord, matched: &RichWorkMetadataMatch) -> Result<(), MetadataError> {
    add_subjects(record, rich_subjects(matched).map_err(MetadataError)?);
    Ok(())
}

fn add_subjects(record: &mut BookRecord, subjects: Vec<BookSubject>) {
    for subject in subjects {
        if !record.book.subjects.contains(&subject) {
            record.book.subjects.push(subject);
        }
    }
}

fn subject_key(subject: &BookSubject) -> (String, String) {
    (subject.authority().unwrap_or_default().to_ascii_lowercase(), subject.code().unwrap_or_else(|| subject.name()).to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::tests::snapshot_from;

    fn facts(title: &str, isbn: Option<&str>) -> BookFacts {
        let author = Contributor::with_id(book_model::agent_id_from_migration_seed(b"local author"), "Alex Smith", AUTHOR_MARC_RELATOR_CODE).unwrap();
        let mut book = book_model::BookMetadata::default();
        if let Some(isbn) = isbn {
            book.identifiers.push(Identifier::new(isbn, Scheme::Isbn, Scope::Book).unwrap());
        }
        let record = BookRecord { metadata: book_model::BookDetails { title: title.to_owned(), subtitle: None, contributors: vec![author], book: book }, description: "File description.".to_owned() };
        BookFacts { key: "0".to_owned(), record, audiobook: false, readings: Vec::new(), recording: None }
    }

    fn service(directory: &std::path::Path) -> MetadataService {
        let database = snapshot_from(
            directory,
            &[serde_json::json!({
                "key": "/books/OL1M", "title": "Example LCC Book", "works": [{"key": "/works/OL10W"}],
                "authors": [{"key": "/authors/OL20A"}], "isbn_13": ["9780306406157"], "lc_classifications": ["E757 .A37 1985"]
            })],
            &[serde_json::json!({
                "key": "/works/OL10W", "title": "Example LCC Book", "authors": [{"author": {"key": "/authors/OL20A"}}],
                "description": {"type": "/type/text", "value": "Open Library summary."}
            })],
            &[serde_json::json!({"key": "/authors/OL20A", "name": "Alex Smith"})],
        );
        {
            let answers = directory.join("answers.sqlite");
            crate::build_answers(&database, None, None, None, &answers).unwrap();
            let service = MetadataService::open(&answers, &directory.join("demand.sqlite")).unwrap();
            service.state.demand.lock().unwrap().execute("INSERT INTO resolved_cover VALUES('isbn-9780306406157',?1)", [b"cached art".as_slice()]).unwrap();
            service
        }
    }

    async fn enrich(service: &MetadataService, facts: BookFacts) -> BookEnrichment {
        let mut response = service.enrich_books(BookEnrichmentRequest { books: vec![facts] }).await.unwrap();
        assert_eq!(response.policy, POLICY);
        response.results.pop().unwrap()
    }

    #[tokio::test]
    async fn resolved_cover_skips_missing_isbn_and_survives_restart() {
        let temporary = tempfile::tempdir().unwrap();
        let service = service(temporary.path());
        service.state.demand.lock().unwrap().execute("DELETE FROM resolved_cover", []).unwrap();
        let provider = axum::Router::new().route(
            "/b/isbn/:file",
            axum::routing::get(|axum::extract::Path(file): axum::extract::Path<String>| async move {
                if file == "9780306406157-L.jpg" {
                    (axum::http::StatusCode::OK, b"resolved artwork".to_vec())
                } else {
                    (axum::http::StatusCode::NOT_FOUND, Vec::new())
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, provider).await.unwrap();
        });
        let isbns = vec!["9780131103627".to_owned(), "9780306406157".to_owned()];
        assert!(service.resolve_isbn_cover_at(&isbns[..1], &origin).await.unwrap().is_none());
        let cover = service.resolve_isbn_cover_at(&isbns, &origin).await.unwrap().unwrap();
        assert_eq!(cover.id, "isbn-9780306406157");
        task.abort();
        let reopened = MetadataService::open(&temporary.path().join("answers.sqlite"), &temporary.path().join("demand.sqlite")).unwrap();
        assert_eq!(reopened.resolve_isbn_cover_at(&isbns[1..], &origin).await.unwrap(), Some(cover.clone()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, crate::app(reopened)).await.unwrap();
        });
        let http = reqwest::Client::new();
        let response = http.get(format!("{origin}/v3/covers/{}", cover.id)).send().await.unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert_eq!(response.bytes().await.unwrap().as_ref(), b"resolved artwork");
        assert_eq!(http.get(format!("{origin}/v3/covers/missing")).send().await.unwrap().status(), reqwest::StatusCode::NOT_FOUND);
        task.abort();
    }

    #[tokio::test]
    async fn isbn_book_gains_subjects_author_identity_and_description() {
        let temporary = tempfile::tempdir().unwrap();
        let service = service(temporary.path());
        let result = enrich(&service, facts("Example LCC Book", Some("9780306406157"))).await;

        assert_eq!(result.status, BookEnrichmentStatus::Found, "{}", result.detail);
        let record = result.record.unwrap();
        assert!(record.book.subjects.iter().any(|subject| subject.authority() == Some("lcc")), "{:?}", record.book.subjects);
        let expected = author_id("open_library:OL20A");
        assert_eq!(record.contributors[0].agent_id(), expected, "an authority-resolved author gets the same id on every device");
        assert_eq!(record.description, "Open Library summary.", "the server reports its own description, never the file's");
        assert_eq!(result.cover.unwrap().id, "isbn-9780306406157");
    }

    #[tokio::test]
    async fn title_only_book_is_classified_through_edition_identity() {
        let temporary = tempfile::tempdir().unwrap();
        let service = service(temporary.path());
        let result = enrich(&service, facts("Example LCC Book", None)).await;

        assert_eq!(result.status, BookEnrichmentStatus::Found, "{}", result.detail);
        let record = result.record.unwrap();
        assert!(record.book.subjects.iter().any(|subject| subject.authority() == Some("lcc")), "{:?}", record.book.subjects);
    }

    #[tokio::test]
    async fn unknown_book_is_no_match_and_repeats_identically() {
        let temporary = tempfile::tempdir().unwrap();
        let service = service(temporary.path());
        let first = enrich(&service, facts("A Book Nobody Catalogued", None)).await;
        let second = enrich(&service, facts("A Book Nobody Catalogued", None)).await;

        assert_eq!(first.status, BookEnrichmentStatus::NoMatch, "{}", first.detail);
        assert!(first.record.is_none());
        assert_eq!(serde_json::to_string(&first).unwrap(), serde_json::to_string(&second).unwrap(), "the same facts get the same answer");
    }

    #[tokio::test]
    async fn empty_and_oversized_requests_are_rejected() {
        let temporary = tempfile::tempdir().unwrap();
        let service = service(temporary.path());
        assert!(service.enrich_books(BookEnrichmentRequest { books: Vec::new() }).await.is_err());
        let books = (0..=MAX_BOOKS_PER_REQUEST).map(|_| facts("Example LCC Book", None)).collect();
        assert!(service.enrich_books(BookEnrichmentRequest { books }).await.is_err());
    }
}
