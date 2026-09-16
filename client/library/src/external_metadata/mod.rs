use crate::storage_values::invalid_integer_column;
use crate::storage_values::invalid_text_column;
use library_model::AgentId;
use library_model::ExternalAuthorId;
use library_model::{Identifier, PublicationMetadata, PublicationSubject, Scheme, Scope, canonical_isbn};
use metadata_contract::{EntitySource, PersonEntity, PlaceEntity, RichWorkMetadataMatch};
use rusqlite::Connection;
use rusqlite::OptionalExtension;
use std::collections::BTreeSet;
use std::collections::HashSet;
use sync_common::ContentHash;

// Imported CIP references remain separate from publication identity.

const ALTERNATIVE_EDITION_PROVENANCE: &str = "openlibrary-alternative-edition-match";
fn unresolved_entity_id(kind: &str, name: &str) -> String {
    let prefix = if kind == "place" { "OLP-" } else { "OLR-" };
    let mut id = String::with_capacity(prefix.len() + name.len() * 2);
    id.push_str(prefix);
    for byte in name.trim().as_bytes() {
        use std::fmt::Write as _;
        let _ = write!(id, "{byte:02x}");
    }
    id
}

#[derive(Clone, Debug)]
pub struct SubjectEnrichmentCandidate {
    pub content_hash: ContentHash,
    pub title: String,
    pub isbn: String,
}

#[derive(Clone, Debug)]
pub struct RichEnrichmentCandidate {
    pub content_hash: ContentHash,
    pub isbns: Vec<String>,
    /// ISBNs which identify the local publication itself. ISBNs attached from
    /// every ambiguous Open Library edition remain useful for locating work
    /// records, but their edition metadata must not be treated as exact
    /// evidence for the local file.
    pub exact_isbns: BTreeSet<String>,
    pub reference_isbns: BTreeSet<String>,
    pub classification_count: usize,
}

#[derive(Clone, Debug)]
pub struct AuthorEnrichmentCredit {
    pub position: usize,
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct AuthorEnrichmentCandidate {
    pub content_hash: ContentHash,
    pub isbn: String,
    pub isbns: Vec<String>,
    pub authors: Vec<AuthorEnrichmentCredit>,
}

#[derive(Clone, Debug)]
pub struct EditionIdentityCandidate {
    pub content_hash: ContentHash,
    pub title: String,
    pub authors: Vec<String>,
    pub publishers: Vec<String>,
    pub publication_year: Option<i32>,
    pub resource_isbns: Vec<String>,
}

/// Descriptions are independent of subject completion and apply to all formats.
pub fn description_candidates_after(conn: &Connection, provider: &str, limit: usize, after: &str) -> rusqlite::Result<Vec<(ContentHash, Vec<String>)>> {
    description_candidates_after_scoped(conn, provider, limit, after, None)
}

pub fn description_candidates_after_scoped(conn: &Connection, provider: &str, limit: usize, after: &str, scope: Option<&str>) -> rusqlite::Result<Vec<(ContentHash, Vec<String>)>> {
    let mut query = conn.prepare_cached(include_str!("sql/mod/description_candidates_after_query.sql"))?;
    query.query_map(rusqlite::params![after, provider, limit as i64, scope], |r| Ok((ContentHash::new(&r.get::<_, String>(0)?), r.get::<_, String>(1)?.split(',').map(str::to_owned).collect())))?.collect()
}

pub fn apply_description(conn: &rusqlite::Transaction<'_>, hash: ContentHash, provider: &str, isbns: &str, description: Option<&str>, status: &str) -> rusqlite::Result<usize> {
    let updated = description.map(|text| super::metadata::update_missing_description(conn, &hash, text)).transpose()?.unwrap_or(false);
    record_external_metadata_attempt(conn, &hash, provider, isbns, status, None)?;
    Ok(usize::from(updated))
}

/// Uses the persisted ISBNs produced by normal enrichment, with its existing
/// attempt ledger for negative results and retry backoff.
pub fn cover_enrichment_candidates(conn: &Connection, provider: &str, limit: usize) -> Result<Vec<(ContentHash, Vec<String>)>, rusqlite::Error> {
    cover_enrichment_candidates_after(conn, provider, limit, None)
}

pub fn cover_enrichment_candidates_after(conn: &Connection, provider: &str, limit: usize, after: Option<&str>) -> Result<Vec<(ContentHash, Vec<String>)>, rusqlite::Error> {
    cover_enrichment_candidates_after_scoped(conn, provider, limit, after, None)
}

pub fn cover_enrichment_candidates_after_scoped(conn: &Connection, provider: &str, limit: usize, after: Option<&str>, scope: Option<&str>) -> Result<Vec<(ContentHash, Vec<String>)>, rusqlite::Error> {
    let mut statement = conn.prepare(include_str!("sql/external_metadata/cover_enrichment_candidates_after_with.sql"))?;
    statement
        .query_map(rusqlite::params![provider, i64::try_from(limit).unwrap_or(i64::MAX), after, scope], |row| Ok((ContentHash::new(&row.get::<_, String>(0)?), row.get::<_, String>(1)?.split(',').map(str::to_owned).collect())))?
        .collect()
}

// Existing device-local review blobs predate the optional subtitle field.

/// Recover review/lookup evidence from original placements without opening files.
fn filename_review_identity(conn: &Connection, hash: &ContentHash, title: String, authors: Vec<String>, year: Option<i32>) -> rusqlite::Result<(String, Vec<String>, Option<i32>)> {
    use publication_enrichment::filename_identity::{unusable_identity, with_filename};
    if !unusable_identity(&title) && authors.iter().any(|a| !unusable_identity(a)) && year.is_some() {
        return Ok((title, authors, year));
    }
    let names = conn.prepare_cached(include_str!("sql/external_metadata/filename_review_identity_select.sql"))?.query_map([hash.as_str()], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let mut identity = (title, authors, year);
    for name in names {
        identity = with_filename(identity.0, identity.1, identity.2, &name);
    }
    Ok(identity)
}

fn edition_identity_input(conn: &Connection, content_hash: ContentHash, title: String, raw_publication: &[u8]) -> Result<EditionIdentityCandidate, rusqlite::Error> {
    let publication = decode_publication_metadata(raw_publication)?;
    let publishers = publication.publishers.iter().map(|publisher| publisher.name().as_str().to_owned()).collect();
    let publication_year = library_model::unique_publication_year(publication.dates.iter().map(|date| date.value()));
    let mut statement = conn.prepare_cached(include_str!("sql/external_metadata/edition_identity_input_select.sql"))?;
    let authors = statement.query_map([content_hash.as_str()], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
    let title = publication_enrichment::title_identity::full_title(&title, &publication);
    let (title, authors, publication_year) = filename_review_identity(conn, &content_hash, title, authors, publication_year)?;
    let resource_isbns = publication_enrichment::related_isbn::resource_candidates(&publication.properties);
    Ok(EditionIdentityCandidate { content_hash, title, authors, publishers, publication_year, resource_isbns })
}

/// Read local bibliographic evidence after an ISBN lookup misses, without
/// changing or inferring any publication identifiers.
pub fn isbn_miss_identity_input(conn: &Connection, content_hash: ContentHash) -> Result<Option<EditionIdentityCandidate>, rusqlite::Error> {
    let row = conn.query_row(include_str!("sql/external_metadata/related_isbn_input_select.sql"), [content_hash.as_str()], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))).optional()?;
    row.map(|(title, raw)| edition_identity_input(conn, content_hash, title, &raw)).transpose()
}

/// Persisted local inspection evidence, read without opening book files.
pub fn related_isbn_input(conn: &Connection, content_hash: ContentHash) -> rusqlite::Result<Option<(EditionIdentityCandidate, Vec<library_model::PublicationProperty>)>> {
    let row = conn.query_row(include_str!("sql/external_metadata/related_isbn_input_select.sql"), [content_hash.as_str()], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))).optional()?;
    row.map(|(title, raw)| {
        let properties = decode_publication_metadata(&raw)?.properties;
        Ok((edition_identity_input(conn, content_hash, title, &raw)?, properties))
    })
    .transpose()
}

pub fn record_external_metadata_attempt(conn: &Connection, content_hash: &ContentHash, provider_id: &str, identifier: &str, status: &str, detail: Option<&str>) -> Result<(), rusqlite::Error> {
    let bounded_detail = detail.map(|value| value.chars().take(1_024).collect::<String>());
    conn.execute(include_str!("sql/external_metadata/apply_external_subjects_insert.sql"), rusqlite::params![content_hash.as_str(), provider_id, identifier, status, bounded_detail])?;
    Ok(())
}

pub fn apply_filename_isbn(conn: &mut Connection, content_hash: ContentHash, provider_id: &str, isbn: &str) -> Result<bool, rusqlite::Error> {
    let canonical = canonical_isbn(isbn).ok_or_else(|| invalid_text_column(0, "filename ISBN is invalid"))?;
    apply_edition_identity_with_provenance(conn, content_hash, provider_id, &canonical, &[canonical.as_str()], "updated", Some("checksum-valid ISBN extracted from filename"), "filename-isbn")
}

pub fn edition_identity_candidates(conn: &Connection, provider_id: &str, limit: usize, retry_failed: bool) -> Result<Vec<EditionIdentityCandidate>, rusqlite::Error> {
    edition_identity_candidates_after(conn, provider_id, limit, retry_failed, None)
}

pub fn edition_identity_candidates_after(conn: &Connection, provider_id: &str, limit: usize, retry_failed: bool, after: Option<&str>) -> Result<Vec<EditionIdentityCandidate>, rusqlite::Error> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let mut statement = conn.prepare(include_str!("sql/mod/edition_identity_candidates_after_query.sql"))?;
    let rows = statement.query_map(rusqlite::params![provider_id, i32::from(retry_failed), after, i64::try_from(limit).unwrap_or(i64::MAX)], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, Vec<u8>>(2)?)))?;
    let mut candidates = Vec::new();
    for row in rows {
        let (raw_hash, title, raw_publication) = row?;
        let content_hash = ContentHash::new(&raw_hash);
        let candidate = edition_identity_input(conn, content_hash, title, &raw_publication)?;
        if !publication_enrichment::filename_identity::unusable_identity(&candidate.title) {
            candidates.push(candidate);
        }
    }
    Ok(candidates)
}

pub fn edition_identity_candidate(conn: &Connection, provider_id: &str, content_hash: &ContentHash) -> Result<Option<EditionIdentityCandidate>, rusqlite::Error> {
    let row = conn.query_row(include_str!("sql/external_metadata/edition_identity_candidate_select.sql"), [content_hash.as_str()], |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))).optional()?;
    let Some((title, raw_publication)) = row else { return Ok(None) };
    let has_attempt: bool = conn.query_row(include_str!("sql/external_metadata/edition_identity_candidate_select_2.sql"), rusqlite::params![content_hash.as_str(), provider_id], |row| row.get(0))?;
    if !has_attempt {
        return Ok(None);
    }
    Ok(Some(edition_identity_input(conn, *content_hash, title, &raw_publication)?))
}

pub fn apply_edition_identity(conn: &mut Connection, content_hash: ContentHash, provider_id: &str, identifier: &str, isbn: Option<&str>, status: &str, detail: Option<&str>) -> Result<bool, rusqlite::Error> {
    apply_edition_identity_with_provenance(conn, content_hash, provider_id, identifier, &isbn.into_iter().collect::<Vec<_>>(), status, detail, "openlibrary-edition-match")
}

pub fn apply_alternative_edition_isbns(conn: &mut Connection, content_hash: ContentHash, provider_id: &str, identifier: &str, isbns: &[&str], detail: Option<&str>) -> Result<bool, rusqlite::Error> {
    apply_edition_identity_with_provenance(conn, content_hash, provider_id, identifier, isbns, "updated", detail, "openlibrary-alternative-edition-match")
}

fn apply_edition_identity_with_provenance(conn: &mut Connection, content_hash: ContentHash, provider_id: &str, identifier: &str, isbns: &[&str], status: &str, detail: Option<&str>, provenance_value: &str) -> Result<bool, rusqlite::Error> {
    let transaction = crate::schema::library_write_transaction(conn)?;
    let result = apply_edition_identity_with_provenance_in_transaction(&transaction, content_hash, provider_id, identifier, isbns, status, detail, provenance_value)?;
    transaction.commit()?;
    Ok(result)
}

/// Apply inside an existing library write transaction.
fn apply_edition_identity_with_provenance_in_transaction(
    conn: &rusqlite::Transaction<'_>, content_hash: ContentHash, provider_id: &str, identifier: &str, isbns: &[&str], status: &str, detail: Option<&str>, provenance_value: &str,
) -> Result<bool, rusqlite::Error> {
    let transaction = conn;
    let raw_publication = transaction.query_row(include_str!("sql/external_metadata/apply_external_subjects_select.sql"), [content_hash.as_str()], |row| row.get::<_, Vec<u8>>(0)).optional()?;
    let Some(raw_publication) = raw_publication else { return Ok(false) };
    let mut updated = false;
    if !isbns.is_empty() {
        let already_has_isbn: bool = transaction.query_row(include_str!("sql/external_metadata/apply_edition_identity_with_provenance_in_transaction_select_2.sql"), [content_hash.as_str()], |row| row.get(0))?;
        if !already_has_isbn {
            let mut publication = decode_publication_metadata(&raw_publication)?;
            for isbn in isbns.iter().copied().collect::<BTreeSet<_>>() {
                let inferred = Identifier::new(isbn, Scheme::Isbn, Scope::Edition, provenance_value).map_err(|error| invalid_text_column(0, error.to_string()))?;
                publication.identifiers.push(inferred);
            }
            super::metadata::update_book_publication_metadata_preserving_ids(&transaction, content_hash.clone(), &publication)?;
            updated = true;
        }
    }
    let bounded_detail = detail.map(|value| value.chars().take(1_024).collect::<String>());
    transaction.execute(include_str!("sql/external_metadata/apply_external_subjects_insert.sql"), rusqlite::params![content_hash.as_str(), provider_id, identifier, status, bounded_detail])?;
    Ok(updated)
}

pub fn rich_enrichment_candidates(conn: &Connection, provider_id: &str, limit: usize, retry_failed: bool) -> Result<Vec<RichEnrichmentCandidate>, rusqlite::Error> {
    rich_enrichment_candidates_after(conn, provider_id, limit, retry_failed, None)
}

pub fn rich_enrichment_candidates_after(conn: &Connection, provider_id: &str, limit: usize, retry_failed: bool, after: Option<&str>) -> Result<Vec<RichEnrichmentCandidate>, rusqlite::Error> {
    let sql = include_str!("sql/mod/rich_enrichment_candidates_after_query.sql");
    // A keyset page must sort only by its cursor. Keeping the classification
    // priority here would require scanning/sorting the entire remaining set.
    let sql = if after.is_some() {
        sql.replace("ORDER BY classification_count DESC,book.content_hash", "ORDER BY book.content_hash").replace("ORDER BY candidates.classification_count DESC,candidates.content_hash", "ORDER BY candidates.content_hash")
    } else {
        sql.to_owned()
    };
    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map(rusqlite::params![provider_id, i32::from(retry_failed), i64::try_from(limit).unwrap_or(i64::MAX), after], |row| {
        let classification_count = row.get::<_, i64>(3)?;
        let classification_count = usize::try_from(classification_count).map_err(|error| rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Integer, Box::new(error)))?;
        Ok((ContentHash::new(&row.get::<_, String>(0)?), row.get::<_, Option<String>>(1)?, row.get::<_, Option<String>>(2)?, classification_count))
    })?;
    let mut candidates = Vec::<RichEnrichmentCandidate>::new();
    for row in rows {
        let (content_hash, raw_isbn, source, classification_count) = row?;
        if candidates.last().is_none_or(|candidate| candidate.content_hash != content_hash) {
            candidates.push(RichEnrichmentCandidate { content_hash, isbns: Vec::new(), exact_isbns: BTreeSet::new(), reference_isbns: BTreeSet::new(), classification_count });
        }
        let Some(isbn) = raw_isbn.as_deref().and_then(canonical_isbn) else { continue };
        let candidate = candidates.last_mut().expect("candidate was inserted above");
        if source.as_deref() == Some("inspection-reference") {
            candidate.reference_isbns.insert(isbn.clone());
        }
        let is_alternative_edition = source.as_deref() == Some(ALTERNATIVE_EDITION_PROVENANCE) || source.as_deref() == Some("inspection-reference");
        if !is_alternative_edition {
            candidate.exact_isbns.insert(isbn.clone());
        }
        if candidate.isbns.last() != Some(&isbn) {
            candidate.isbns.push(isbn);
        }
    }
    Ok(candidates)
}

pub fn apply_rich_enrichment(
    conn: &mut Connection, content_hash: ContentHash, provider_id: &str, isbn: &str, matched: Option<&RichWorkMetadataMatch>, places: &[PlaceEntity], people: &[PersonEntity], subjects: Vec<PublicationSubject>, status: &str,
    detail: Option<&str>, replace_previous_work_classifications: bool,
) -> Result<usize, rusqlite::Error> {
    let transaction = crate::schema::library_write_transaction(conn)?;
    let result = apply_rich_enrichment_in_transaction(&transaction, content_hash, provider_id, isbn, matched, places, people, subjects, status, detail, replace_previous_work_classifications)?;
    transaction.commit()?;
    Ok(result)
}

/// Apply inside an existing library write transaction.
pub fn apply_rich_enrichment_in_transaction(
    conn: &rusqlite::Transaction<'_>, content_hash: ContentHash, provider_id: &str, isbn: &str, matched: Option<&RichWorkMetadataMatch>, places: &[PlaceEntity], people: &[PersonEntity], subjects: Vec<PublicationSubject>, status: &str,
    detail: Option<&str>, replace_previous_work_classifications: bool,
) -> Result<usize, rusqlite::Error> {
    let transaction = conn;
    let raw_publication = transaction.query_row(include_str!("sql/external_metadata/apply_external_subjects_select.sql"), [content_hash.as_str()], |row| row.get::<_, Vec<u8>>(0)).optional()?;
    let Some(raw_publication) = raw_publication else { return Ok(0) };

    for place in places {
        transaction.execute(include_str!("sql/external_metadata/apply_rich_enrichment_in_transaction_insert.sql"), rusqlite::params![place.wikidata_id, place.label, place.latitude_e7, place.longitude_e7])?;
    }
    for person in people {
        transaction.execute(include_str!("sql/external_metadata/apply_rich_enrichment_in_transaction_insert_2.sql"), rusqlite::params![person.wikidata_id, person.label, person.birth_year, person.death_year, i32::from(person.fictional)])?;
    }
    for (wikidata_id, aliases) in places.iter().map(|entity| (&entity.wikidata_id, &entity.aliases)).chain(people.iter().map(|entity| (&entity.wikidata_id, &entity.aliases))) {
        transaction.execute(include_str!("sql/external_metadata/apply_rich_enrichment_in_transaction_delete.sql"), [wikidata_id])?;
        for alias in aliases {
            transaction.execute(include_str!("sql/external_metadata/apply_rich_enrichment_in_transaction_insert_3.sql"), rusqlite::params![wikidata_id, alias])?;
        }
    }
    for place in places {
        transaction.execute(include_str!("sql/external_metadata/apply_rich_enrichment_in_transaction_delete_2.sql"), [&place.wikidata_id])?;
        for (relation, parent_ids) in [("parent", &place.parent_ids), ("country", &place.country_ids)] {
            for parent_id in parent_ids {
                transaction.execute(include_str!("sql/external_metadata/apply_rich_enrichment_in_transaction_insert_4.sql"), rusqlite::params![place.wikidata_id, parent_id, relation])?;
            }
        }
    }

    // Classification-only recovery must not erase an existing entity graph.
    let classifications_only = matched.is_some_and(|m| {
        m.open_library_work_id.is_none()
            && m.exact_edition_ids.is_empty()
            && m.places.is_empty()
            && m.people.is_empty()
            && m.classifications.iter().any(|c| {
                c.evidence.iter().any(|e| {
                    e.method == "local_bibliographic_isbn_reference"
                        || e.method == "local_bibliographic_print_isbn_verified_title_author"
                        || e.method == metadata_contract::year_consensus::METHOD
                        || e.method == metadata_contract::subject_consensus::METHOD
                })
            })
    });
    let retired_entities = if classifications_only {
        Vec::new()
    } else {
        transaction.prepare(include_str!("sql/external_metadata/apply_rich_enrichment_in_transaction_select_2.sql"))?.query_map([content_hash.as_str()], |row| row.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?
    };
    if !classifications_only {
        transaction.execute(include_str!("sql/external_metadata/apply_rich_enrichment_in_transaction_delete_3.sql"), [content_hash.as_str()])?;
        if let Some(matched) = matched {
            for (kind, references) in [("place", &matched.places), ("person", &matched.people)] {
                for reference in references {
                    let entity_id = reference.wikidata_id.clone().unwrap_or_else(|| unresolved_entity_id(kind, &reference.name));
                    if reference.wikidata_id.is_none() {
                        transaction.execute(include_str!("sql/external_metadata/apply_rich_enrichment_in_transaction_insert_5.sql"), rusqlite::params![entity_id, kind, reference.name])?;
                    }
                    let source = match reference.source {
                        EntitySource::ExactEdition => "exact_edition",
                        EntitySource::Work => "work",
                    };
                    transaction.execute(include_str!("sql/external_metadata/apply_rich_enrichment_in_transaction_insert_6.sql"), rusqlite::params![content_hash.as_str(), entity_id, kind, reference.name, source])?;
                }
            }
        }
    }
    // Only this book's retired references can have become orphaned. Avoid
    // scanning the library-wide entity table for every book under the writer.
    for entity in retired_entities {
        transaction.execute(include_str!("sql/external_metadata/apply_rich_enrichment_in_transaction_delete_4.sql"), [entity])?;
    }

    let mut imported = 0;
    if let Some(description) = matched.and_then(|value| value.description.as_deref()) {
        if super::metadata::update_missing_description(transaction, &content_hash, description)? {
            imported += 1;
        }
    }
    if !subjects.is_empty() || replace_previous_work_classifications {
        let mut publication = decode_publication_metadata(&raw_publication)?;
        let before = publication.subjects.len();
        if replace_previous_work_classifications {
            let fallback_codes = {
                let mut statement = transaction.prepare(include_str!("sql/external_metadata/apply_rich_enrichment_in_transaction_select_3.sql"))?;
                let codes = statement.query_map([content_hash.as_str()], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?.collect::<Result<BTreeSet<_>, _>>()?;
                codes
            };
            publication.subjects.retain(|subject| {
                let authority = subject.authority().unwrap_or_default().to_ascii_lowercase();
                let notation = subject.code().unwrap_or_else(|| subject.name());
                !fallback_codes.contains(&(authority, notation.to_owned()))
            });
        }
        let removed = publication.subjects.len() != before;
        for subject in subjects {
            if !publication.subjects.contains(&subject) {
                publication.subjects.push(subject);
                imported += 1;
            }
        }
        if imported > 0 || removed {
            super::metadata::update_book_publication_metadata_preserving_ids(&transaction, content_hash.clone(), &publication)?;
        }
    }
    let bounded_detail = detail.map(|value| value.chars().take(1_024).collect::<String>());
    transaction.execute(include_str!("sql/external_metadata/apply_external_subjects_insert.sql"), rusqlite::params![content_hash.as_str(), provider_id, isbn, status, bounded_detail])?;
    Ok(imported)
}

pub fn author_enrichment_candidates(conn: &Connection, provider_id: &str, limit: usize, retry_failed: bool) -> Result<Vec<AuthorEnrichmentCandidate>, rusqlite::Error> {
    author_enrichment_candidates_after(conn, provider_id, limit, retry_failed, None)
}

pub fn author_enrichment_candidates_after(conn: &Connection, provider_id: &str, limit: usize, retry_failed: bool, after: Option<&str>) -> Result<Vec<AuthorEnrichmentCandidate>, rusqlite::Error> {
    author_enrichment_candidates_after_scoped(conn, provider_id, limit, retry_failed, after, None)
}

pub fn author_enrichment_candidates_after_scoped(conn: &Connection, provider_id: &str, limit: usize, retry_failed: bool, after: Option<&str>, scope: Option<&str>) -> Result<Vec<AuthorEnrichmentCandidate>, rusqlite::Error> {
    let mut statement = conn.prepare(include_str!("sql/mod/author_enrichment_candidates_after_query.sql"))?;
    let rows = statement.query_map(rusqlite::params![provider_id, i32::from(retry_failed), i64::try_from(limit).unwrap_or(i64::MAX), after, scope], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
    let mut candidates = Vec::new();
    for row in rows {
        let (raw_hash, raw_isbns) = row?;
        let isbns = raw_isbns.split(',').filter_map(canonical_isbn).collect::<BTreeSet<_>>().into_iter().collect::<Vec<_>>();
        let Some(isbn) = isbns.first().cloned() else { continue };
        let content_hash = ContentHash::new(&raw_hash);
        let mut authors = conn.prepare_cached(include_str!("sql/external_metadata/author_enrichment_candidates_after_select.sql"))?;
        let authors = authors
            .query_map([content_hash.as_str()], |row| {
                let position = usize::try_from(row.get::<_, i64>(0)?).map_err(|_| invalid_integer_column(0, "author position must not be negative"))?;
                Ok(AuthorEnrichmentCredit { position, name: row.get(1)? })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        candidates.push(AuthorEnrichmentCandidate { content_hash, isbn, isbns, authors });
    }
    Ok(candidates)
}

/// Replace author-role credits from an unambiguous ISBN record, preserving other roles.
pub fn apply_authoritative_authors_in_transaction(conn: &rusqlite::Transaction<'_>, content_hash: ContentHash, provider_id: &str, isbn: &str, authors: &[(usize, String, ExternalAuthorId)]) -> Result<usize, rusqlite::Error> {
    if authors.is_empty() {
        return Ok(0);
    }
    let mut credits = authors.iter().map(|(_, name, _)| library_model::Contributor::new(name, None, library_model::MarcRelatorCode(*b"aut")).map_err(|e| invalid_text_column(0, e.to_string()))).collect::<Result<Vec<_>, _>>()?;
    let mut statement = conn.prepare(include_str!("sql/external_metadata/apply_authoritative_authors_in_transaction_select.sql"))?;
    for row in statement.query_map([content_hash.as_str()], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, Vec<u8>>(3)?)))? {
        let (id, name, sort, role) = row?;
        let id = AgentId::parse_str(&id).map_err(|e| invalid_text_column(0, e.to_string()))?;
        let role = library_model::MarcRelatorCode(role.try_into().map_err(|_| invalid_text_column(3, "invalid contributor role"))?);
        credits.push(library_model::Contributor::with_id(id, &name, sort, role).map_err(|e| invalid_text_column(0, e.to_string()))?);
    }
    drop(statement);
    let prepared = super::authors::prepare_contributors(conn, &credits, super::authors::AuthorIdPolicy::PreserveIncoming, None)?;
    super::authors::replace_book_contributors(conn, content_hash, &prepared)?;
    apply_author_enrichment_in_transaction(conn, content_hash, provider_id, isbn, authors, "updated", Some("authoritative Open Library author credits from unambiguous ISBN match"))
}

pub fn apply_author_enrichment(conn: &mut Connection, content_hash: ContentHash, provider_id: &str, isbn: &str, resolutions: &[(usize, String, ExternalAuthorId)], status: &str, detail: Option<&str>) -> Result<usize, rusqlite::Error> {
    let transaction = crate::schema::library_write_transaction(conn)?;
    let result = apply_author_enrichment_in_transaction(&transaction, content_hash, provider_id, isbn, resolutions, status, detail)?;
    transaction.commit()?;
    Ok(result)
}

/// Apply inside an existing library write transaction.
pub fn apply_author_enrichment_in_transaction(
    conn: &rusqlite::Transaction<'_>, content_hash: ContentHash, provider_id: &str, isbn: &str, resolutions: &[(usize, String, ExternalAuthorId)], status: &str, detail: Option<&str>,
) -> Result<usize, rusqlite::Error> {
    let transaction = conn;
    let mut updated = 0_usize;
    for (position, preferred_name, external_id) in resolutions {
        super::metadata::resolve_book_author_identity(&transaction, content_hash.clone(), *position, preferred_name, external_id)?;
        updated += 1;
    }
    let bounded_detail = detail.map(|value| value.chars().take(1_024).collect::<String>());
    transaction.execute(include_str!("sql/external_metadata/apply_external_subjects_insert.sql"), rusqlite::params![content_hash.as_str(), provider_id, isbn, status, bounded_detail])?;
    Ok(updated)
}

pub fn subject_enrichment_candidates(conn: &Connection, provider_id: &str, limit: usize, retry_failed: bool) -> Result<Vec<SubjectEnrichmentCandidate>, rusqlite::Error> {
    let limit = i64::try_from(limit).unwrap_or(i64::MAX);
    let retry_failed = i32::from(retry_failed);
    let mut statement = conn.prepare(include_str!("sql/external_metadata/subject_enrichment_candidates_select.sql"))?;
    let rows = statement.query_map(rusqlite::params![provider_id, retry_failed, limit], |row| {
        let raw_hash: String = row.get(0)?;
        Ok(SubjectEnrichmentCandidate { content_hash: ContentHash::new(&raw_hash), title: row.get(1)?, isbn: row.get(2)? })
    })?;
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();
    for candidate in rows {
        let candidate = candidate?;
        if canonical_isbn(&candidate.isbn).is_some() && seen.insert(candidate.content_hash.clone()) {
            candidates.push(candidate);
        }
    }
    Ok(candidates)
}

pub fn apply_external_subjects(conn: &mut Connection, content_hash: ContentHash, provider_id: &str, isbn: &str, subjects: Vec<PublicationSubject>, status: &str, detail: Option<&str>) -> Result<usize, rusqlite::Error> {
    let transaction = crate::schema::library_write_transaction(conn)?;
    let raw_publication = transaction.query_row(include_str!("sql/external_metadata/apply_external_subjects_select.sql"), [content_hash.as_str()], |row| row.get::<_, Vec<u8>>(0)).optional()?;
    let Some(raw_publication) = raw_publication else {
        return Ok(0);
    };
    let mut imported = 0;
    if !subjects.is_empty() {
        let has_subjects: bool = transaction.query_row(include_str!("sql/external_metadata/apply_external_subjects_select_2.sql"), [content_hash.as_str()], |row| row.get(0))?;
        if !has_subjects {
            let mut publication = decode_publication_metadata(&raw_publication)?;
            imported = subjects.len();
            publication.subjects.extend(subjects);
            super::metadata::update_book_publication_metadata_preserving_ids(&transaction, content_hash.clone(), &publication)?;
        }
    }
    let bounded_detail = detail.map(|value| value.chars().take(1_024).collect::<String>());
    transaction.execute(include_str!("sql/external_metadata/apply_external_subjects_insert.sql"), rusqlite::params![content_hash.as_str(), provider_id, isbn, status, bounded_detail])?;
    transaction.commit()?;
    Ok(imported)
}

pub(super) fn decode_publication_metadata(raw: &[u8]) -> Result<PublicationMetadata, rusqlite::Error> {
    if raw.is_empty() {
        return Ok(PublicationMetadata::default());
    }
    match sync_common::wire::decode(raw, sync_common::wire::MAX_DECODED_REQUEST_BYTES) {
        Ok(metadata) => Ok(metadata),
        Err(sync_common::wire::WireError::UnsupportedVersion(version @ (10 | 11))) if raw.starts_with(b"BKHM") && raw.get(4) == Some(&version) => {
            let mut compatible = raw.to_vec();
            compatible[4] = sync_common::wire::header_bytes()[4];
            let metadata = sync_common::wire::decode::<PublicationMetadata>(&compatible, sync_common::wire::MAX_DECODED_REQUEST_BYTES).map_err(|error| invalid_text_column(0, error.to_string()))?;
            let reencoded = sync_common::wire::encode(&metadata).map_err(|error| invalid_text_column(0, error.to_string()))?;
            if reencoded.get(5..) != raw.get(5..) {
                return Err(invalid_text_column(0, format!("wire version {version} publication payload is not byte-compatible with the current wire version")));
            }
            log::info!("upgrading a byte-compatible publication metadata value from wire version {version}");
            Ok(metadata)
        }
        Err(error) => Err(invalid_text_column(0, error.to_string())),
    }
}

/// Commit a prepared enrichment batch atomically. Callers bound batch size and
/// release the connection between batches; this callback cannot await network IO.
pub fn apply_enrichment_batch<T>(conn: &mut Connection, apply: impl FnOnce(&rusqlite::Transaction<'_>) -> rusqlite::Result<T>) -> rusqlite::Result<T> {
    let transaction = crate::schema::library_write_transaction(conn)?;
    let result = apply(&transaction)?;
    transaction.commit()?;
    Ok(result)
}

pub fn apply_edition_identity_in_transaction(conn: &rusqlite::Transaction<'_>, content_hash: ContentHash, provider_id: &str, identifier: &str, isbn: Option<&str>, status: &str, detail: Option<&str>) -> rusqlite::Result<bool> {
    apply_edition_identity_with_provenance_in_transaction(conn, content_hash, provider_id, identifier, &isbn.into_iter().collect::<Vec<_>>(), status, detail, "openlibrary-edition-match")
}

pub fn apply_filename_isbn_in_transaction(conn: &rusqlite::Transaction<'_>, content_hash: ContentHash, provider_id: &str, isbn: &str) -> rusqlite::Result<bool> {
    let canonical = canonical_isbn(isbn).ok_or_else(|| invalid_text_column(0, "filename ISBN is invalid"))?;
    apply_edition_identity_with_provenance_in_transaction(conn, content_hash, provider_id, &canonical, &[canonical.as_str()], "updated", Some("checksum-valid ISBN extracted from filename"), "filename-isbn")
}

/// Merge verified recording identifiers while preserving library metadata and IDs.
pub fn apply_audible_identifiers(conn: &rusqlite::Transaction<'_>, hash: ContentHash, asin: &str, isbns: &[String]) -> rusqlite::Result<()> {
    let raw: Option<Vec<u8>> = conn.query_row(include_str!("sql/external_metadata/apply_external_subjects_select.sql"), [hash.as_str()], |row| row.get(0)).optional()?;
    let Some(raw) = raw else { return Ok(()) };
    let mut publication = decode_publication_metadata(&raw)?;
    let source = format!("audible:{asin}");
    let mut new_isbn = false;
    for (value, scheme) in std::iter::once((asin, Scheme::Asin)).chain(isbns.iter().map(|isbn| (isbn.as_str(), Scheme::Isbn))) {
        if !publication.identifiers.iter().any(|id| id.scheme == scheme && id.value == value) {
            new_isbn |= scheme == Scheme::Isbn;
            publication.identifiers.push(Identifier::new(value, scheme, Scope::Edition, &source).map_err(|e| invalid_text_column(0, e.to_string()))?);
        }
    }
    super::metadata::update_book_publication_metadata_preserving_ids(conn, hash, &publication)?;
    if new_isbn {
        // A previous title/ISBN miss must not hide newly verified recording IDs.
        conn.execute(include_str!("sql/external_metadata/apply_audible_identifiers_delete.sql"), [hash.as_str()])?;
    }
    Ok(())
}

#[cfg(test)]
mod filename_review_tests {
    use super::*;
    #[test]
    fn hash_title_uses_filename_for_review_and_keeps_real_titles() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("sql/external_metadata/hash_title_uses_filename_for_review_and_keeps_real_titles_create.sql")).unwrap();
        let hash = ContentHash::new(&"a".repeat(64));
        conn.execute(include_str!("sql/external_metadata/hash_title_uses_filename_for_review_and_keeps_real_titles_insert.sql"), [hash.as_str()]).unwrap();
        for extension in ["pdf", "epub"] {
            conn.execute(include_str!("sql/external_metadata/hash_title_uses_filename_for_review_and_keeps_real_titles_delete.sql"), []).unwrap();
            conn.execute(include_str!("sql/external_metadata/hash_title_uses_filename_for_review_and_keeps_real_titles_insert_2.sql"), [format!("Refactoring UI -- Steve Schoger, Adam Wathan -- 2018 -- archive.{extension}")]).unwrap();
            let actual = filename_review_identity(&conn, &hash, "387b47f0673441e9cb40e35363b49627".into(), vec!["Unknown".into()], None).unwrap();
            assert_eq!(actual, ("Refactoring UI".into(), vec!["Steve Schoger".into(), "Adam Wathan".into()], Some(2018)));
        }
        assert_eq!(filename_review_identity(&conn, &hash, "Actual title".into(), vec!["Actual Author".into()], Some(2020)).unwrap(), ("Actual title".into(), vec!["Actual Author".into()], Some(2020)));
    }
}

#[cfg(test)]
mod subtitle_storage_tests {
    use super::*;
    #[test]
    fn ingested_title_evidence_reaches_lookup_and_survives_wire_storage() {
        let conn = Connection::open_in_memory().unwrap();
        crate::schema::init_db(&conn).unwrap();
        let hash = test_support::fixture_content_hash(91);
        let tx = conn.unchecked_transaction().unwrap();
        crate::metadata::insert_scanned_book(&tx, hash, "Algorithms: A Guide (Jeff Erickson) (2019)", "").unwrap();
        tx.commit().unwrap();
        let raw: Vec<u8> = conn.query_row(include_str!("../metadata/sql/metadata/normalize_stored_title_select.sql"), [hash.as_str()], |row| row.get(1)).unwrap();
        let publication = decode_publication_metadata(&raw).unwrap();
        assert_eq!(publication.subtitle(), Some("A Guide"));
        assert_eq!(publication.dates[0].value(), "2019");
        let candidate = edition_identity_input(&conn, hash, "Algorithms".into(), &raw).unwrap();
        assert_eq!(candidate.title, "Algorithms: A Guide");
        assert_eq!(candidate.authors, ["Jeff Erickson"]);
        assert_eq!(candidate.publication_year, Some(2019));
    }
}
