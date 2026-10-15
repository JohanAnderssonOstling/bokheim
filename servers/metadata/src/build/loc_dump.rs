//! ISBN/LCC evidence imported into the existing metadata snapshot from LC MARC.

use crate::Isbn13;
use crate::{
    error, sql_placeholders, Classification, ClassificationScheme, ClassificationSource, Connection, LookupStatus, MetadataError, RichIsbnMetadataResult, RichWorkMetadataMatch, SubjectHeading, WorkClassificationMatch, MAX_DESCRIPTION_BYTES,
};
use rusqlite::params_from_iter;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn lookup_lcc(connection: &Connection, isbn13: Isbn13) -> Result<BTreeSet<String>, MetadataError> {
    let mut statement = connection.prepare_cached("SELECT DISTINCT notation FROM isbn_lcc WHERE isbn13=?1 ORDER BY notation").map_err(error)?;
    let rows = statement.query_map([isbn13], |row| row.get::<_, String>(0)).map_err(error)?;
    rows.collect::<Result<BTreeSet<_>, _>>().map_err(error)
}

pub(crate) fn merge_lcc(matches: &mut Vec<WorkClassificationMatch>, codes: BTreeSet<String>) {
    // Do not attach one ISBN's evidence to conflicting Open Library works.
    if matches.len() > 1 {
        return;
    }
    let codes = codes.into_iter().filter_map(|code| subject_projection::canonical_lcc_notation(&code)).collect::<BTreeSet<_>>();
    if codes.is_empty() {
        return;
    }
    if matches.is_empty() {
        // An authority record is not an Open Library edition: never manufacture OL IDs.
        matches.push(WorkClassificationMatch { open_library_work_id: None, exact_edition_ids: Vec::new(), classifications: Vec::new() });
    }
    let classifications = &mut matches[0].classifications;
    for notation in codes {
        if let Some(existing) = classifications.iter_mut().find(|c| c.scheme == ClassificationScheme::LibraryOfCongress && c.notation == notation) {
            existing.source = ClassificationSource::ExactEdition;
        } else {
            classifications.push(Classification { evidence: Vec::new(), scheme: ClassificationScheme::LibraryOfCongress, notation, source: ClassificationSource::ExactEdition });
        }
    }
    classifications.sort();
}

/// Fill the rich fields already supported by the wire contract. Bibliographic
/// fields remain in lc_record for a future provider-neutral identity response.
#[cfg(test)]
pub(crate) fn enrich(connection: &Connection, results: &mut [RichIsbnMetadataResult]) -> Result<(), MetadataError> {
    let mut query = connection.prepare_cached("SELECT r.metadata FROM lc_record_isbn i JOIN lc_record r USING(lc_record_id) WHERE i.isbn13=?1 ORDER BY i.lc_record_id").map_err(error)?;
    #[derive(Deserialize)]
    struct Record {
        #[serde(default)]
        descriptions: Vec<String>,
        #[serde(default)]
        subjects: Vec<SubjectHeading>,
    }
    for result in results {
        if result.matches.len() > 1 {
            continue;
        }
        let Some(isbn) = Isbn13::parse(&result.requested_isbn) else { continue };
        let mut descriptions = BTreeSet::new();
        let mut subjects = BTreeSet::new();
        let rows = query.query_map([isbn], |r| r.get::<_, String>(0)).map_err(error)?;
        for row in rows {
            let record: Record = serde_json::from_str(&row.map_err(error)?).map_err(error)?;
            descriptions.extend(record.descriptions.into_iter().filter(|s| !s.trim().is_empty() && s.len() <= MAX_DESCRIPTION_BYTES));
            subjects.extend(record.subjects);
        }
        if descriptions.is_empty() && subjects.is_empty() {
            continue;
        }
        if result.matches.is_empty() {
            result.matches.push(RichWorkMetadataMatch { open_library_work_id: None, exact_edition_ids: Vec::new(), classifications: Vec::new(), authors: Vec::new(), description: None, popularity: None, subjects: Vec::new() });
            result.status = LookupStatus::Matched;
        }
        let matched = &mut result.matches[0];
        if matched.description.as_ref().is_none_or(|s| s.trim().is_empty()) && descriptions.len() == 1 {
            matched.description = descriptions.into_iter().next();
        }
        if matched.subjects.is_empty() {
            matched.subjects = subjects.into_iter().collect();
        }
    }
    Ok(())
}

/// Resolve optional LC records with one indexed query per chunk, including misses.
pub(crate) fn enrich_many(connection: &Connection, results: &mut [RichIsbnMetadataResult]) -> Result<(), MetadataError> {
    if !connection.query_row("SELECT EXISTS(SELECT 1 FROM lc_record_isbn LIMIT 1)", [], |r| r.get::<_, bool>(0)).map_err(error)? {
        return Ok(());
    }
    #[derive(Deserialize)]
    struct Record {
        #[serde(default)]
        descriptions: Vec<String>,
        #[serde(default)]
        subjects: Vec<SubjectHeading>,
    }
    let isbns = results.iter().filter(|result| result.matches.len() <= 1).filter_map(|result| Isbn13::parse(&result.requested_isbn)).collect::<BTreeSet<_>>();
    let mut supplements = BTreeMap::<Isbn13, (BTreeSet<String>, BTreeSet<SubjectHeading>)>::new();
    for chunk in isbns.into_iter().collect::<Vec<_>>().chunks(500) {
        let sql = format!("SELECT i.isbn13,r.metadata FROM lc_record_isbn i JOIN lc_record r USING(lc_record_id) WHERE i.isbn13 IN ({}) ORDER BY i.isbn13,i.lc_record_id", sql_placeholders(chunk.len()));
        let mut query = connection.prepare_cached(&sql).map_err(error)?;
        for row in query.query_map(params_from_iter(chunk), |r| Ok((r.get::<_, Isbn13>(0)?, r.get::<_, String>(1)?))).map_err(error)? {
            let (isbn, json) = row.map_err(error)?;
            let record: Record = serde_json::from_str(&json).map_err(error)?;
            let (descriptions, subjects) = supplements.entry(isbn).or_default();
            descriptions.extend(record.descriptions.into_iter().filter(|s| !s.trim().is_empty() && s.len() <= MAX_DESCRIPTION_BYTES));
            subjects.extend(record.subjects);
        }
    }
    for result in results.iter_mut().filter(|result| result.matches.len() <= 1) {
        let Some((descriptions, subjects)) = Isbn13::parse(&result.requested_isbn).and_then(|isbn| supplements.get(&isbn)) else { continue };
        if descriptions.is_empty() && subjects.is_empty() {
            continue;
        }
        if result.matches.is_empty() {
            result.matches.push(RichWorkMetadataMatch { open_library_work_id: None, exact_edition_ids: vec![], classifications: vec![], authors: vec![], description: None, popularity: None, subjects: vec![] });
            result.status = LookupStatus::Matched;
        }
        let matched = &mut result.matches[0];
        if matched.description.as_ref().is_none_or(|s| s.trim().is_empty()) && descriptions.len() == 1 {
            matched.description = descriptions.first().cloned();
        }
        if matched.subjects.is_empty() {
            matched.subjects = subjects.iter().cloned().collect();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LookupStatus, RichWorkMetadataMatch};

    #[test]
    fn bulk_lc_metadata_matches_individual_lookups_with_duplicate_and_conflicting_records() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch("CREATE TABLE lc_record(lc_record_id TEXT PRIMARY KEY,metadata TEXT); CREATE TABLE lc_record_isbn(isbn13 INTEGER,lc_record_id TEXT,PRIMARY KEY(isbn13,lc_record_id)) WITHOUT ROWID;").unwrap();
        let mut results = Vec::new();
        for id in 0..650_i64 {
            let body = 978_000_000_000 + id;
            let sum = (0..12).map(|p| body / 10_i64.pow(11 - p) % 10 * if p % 2 == 0 { 1 } else { 3 }).sum::<i64>();
            let isbn = body * 10 + (10 - sum % 10) % 10;
            let mut result = RichIsbnMetadataResult { requested_isbn: format!("{isbn:013}"), canonical_isbn13: Some(format!("{isbn:013}")), status: LookupStatus::NoMatch, matches: vec![] };
            if id % 4 == 0 {
                let matched = RichWorkMetadataMatch { open_library_work_id: None, exact_edition_ids: vec![], classifications: vec![], authors: vec![], description: Some("Existing description".into()), popularity: None, subjects: vec![] };
                result.matches = vec![matched.clone(), matched];
                result.status = LookupStatus::Ambiguous;
            }
            let record_id = format!("lc{id}");
            let metadata = serde_json::json!({"descriptions":["Description", ""],"subjects":[{"source":"library_of_congress","authority":"lcsh","components":["Example"],"authority_uri":null}]}).to_string();
            connection.execute("INSERT INTO lc_record VALUES(?1,?2)", rusqlite::params![record_id, metadata]).unwrap();
            connection.execute("INSERT INTO lc_record_isbn VALUES(?1,?2)", rusqlite::params![isbn, record_id]).unwrap();
            if id % 4 == 1 {
                let extra_id = format!("extra{id}");
                connection.execute("INSERT INTO lc_record VALUES(?1,?2)", rusqlite::params![extra_id, serde_json::json!({"descriptions":["Conflicting description"]}).to_string()]).unwrap();
                connection.execute("INSERT INTO lc_record_isbn VALUES(?1,?2)", rusqlite::params![isbn, extra_id]).unwrap();
            }
            results.push(result);
        }
        results.extend(results[..10].to_vec());
        results.push(RichIsbnMetadataResult { requested_isbn: "invalid".into(), canonical_isbn13: None, status: LookupStatus::InvalidIsbn, matches: vec![] });
        let mut expected = results.clone();
        enrich(&connection, &mut expected).unwrap();
        enrich_many(&connection, &mut results).unwrap();
        assert_eq!(serde_json::to_value(&results).unwrap(), serde_json::to_value(expected).unwrap());
        assert_eq!(results[1].matches[0].description, None);
        assert_eq!(results[2].matches[0].description.as_deref(), Some("Description"));
    }
}
