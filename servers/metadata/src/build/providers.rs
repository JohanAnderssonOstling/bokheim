//! Fold provider evidence into a batch while preserving LC-before-LibraryThing precedence.
use super::has_lcc;
use crate::{error, sql_placeholders, Classification, Connection, Isbn13, LookupStatus, MetadataError, RichIsbnMetadataResult, RichWorkMetadataMatch, SubjectHeading};
#[cfg(test)]
use rusqlite::{params, OptionalExtension};
use rusqlite::{params_from_iter, types::ToSql};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn apply_many(connection: &Connection, answers: &mut [RichIsbnMetadataResult]) -> Result<(), MetadataError> {
    let mut eligible = answers
        .iter()
        .enumerate()
        .filter(|(_, answer)| answer.status != LookupStatus::InvalidIsbn && answer.matches.len() <= 1 && !has_lcc(answer))
        .filter_map(|(index, answer)| answer.canonical_isbn13.as_deref().and_then(Isbn13::parse).map(|isbn| (index, isbn)))
        .collect::<Vec<_>>();
    for provider in ["loc", "librarything"] {
        if eligible.is_empty() {
            break;
        }
        let isbns = eligible.iter().map(|(_, isbn)| *isbn).collect::<BTreeSet<_>>();
        let mut evidence = BTreeMap::new();
        for chunk in isbns.into_iter().collect::<Vec<_>>().chunks(500) {
            let sql = format!("SELECT isbn13,classifications,subjects FROM provider_result WHERE provider=? AND status='matched' AND isbn13 IN ({}) ORDER BY isbn13", sql_placeholders(chunk.len()));
            let parameters = std::iter::once(&provider as &dyn ToSql).chain(chunk.iter().map(|isbn| isbn as &dyn ToSql));
            let mut query = connection.prepare_cached(&sql).map_err(error)?;
            for row in query.query_map(params_from_iter(parameters), |r| Ok((r.get::<_, Isbn13>(0)?, r.get::<_, Vec<u8>>(1)?, r.get::<_, Vec<u8>>(2)?))).map_err(error)? {
                let (isbn, codes, subjects) = row.map_err(error)?;
                evidence.insert(isbn, (codes, subjects));
            }
        }
        for &(index, isbn) in &eligible {
            if let Some((codes, subjects)) = evidence.get(&isbn) {
                merge(&mut answers[index], codes, subjects)?;
            }
        }
        if provider == "loc" {
            eligible.retain(|(index, isbn)| !evidence.contains_key(isbn) || !has_lcc(&answers[*index]));
        }
    }
    Ok(())
}

fn merge(answer: &mut RichIsbnMetadataResult, codes: &[u8], subjects: &[u8]) -> Result<(), MetadataError> {
    let codes: Vec<Classification> = serde_json::from_slice(codes).map_err(error)?;
    let subjects: Vec<SubjectHeading> = serde_json::from_slice(subjects).map_err(error)?;
    if codes.is_empty() && subjects.is_empty() {
        return Ok(());
    }
    if answer.matches.is_empty() {
        answer.matches.push(RichWorkMetadataMatch { open_library_work_id: None, exact_edition_ids: vec![], classifications: vec![], authors: vec![], description: None, popularity: None, subjects: vec![] });
    }
    // Preserve existing schemes while allowing crawled LCC to fill a record
    // that previously had only BISAC or DDC.
    let present = answer.matches[0].classifications.iter().map(|c| c.scheme).collect::<BTreeSet<_>>();
    answer.matches[0].classifications.extend(codes.into_iter().filter(|c| !present.contains(&c.scheme)));
    answer.matches[0].subjects.extend(subjects);
    answer.matches[0].subjects.sort();
    answer.matches[0].subjects.dedup();
    answer.status = LookupStatus::Matched;
    Ok(())
}

#[cfg(test)]
pub(super) fn apply_one(c: &Connection, isbn: i64, answer: &mut RichIsbnMetadataResult) -> Result<(), MetadataError> {
    if answer.status == LookupStatus::InvalidIsbn || answer.matches.len() > 1 {
        return Ok(());
    }
    let mut q = c.prepare_cached("SELECT status,classifications,subjects FROM provider_result WHERE provider=?1 AND isbn13=?2").map_err(error)?;
    for provider in ["loc", "librarything"] {
        if has_lcc(answer) {
            break;
        }
        let value = q.query_row(params![provider, isbn], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?, r.get::<_, Vec<u8>>(2)?))).optional().map_err(error)?;
        let Some((status, codes, subjects)) = value else {
            continue;
        };
        if status != "matched" {
            continue;
        }
        merge(answer, &codes, &subjects)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{schema, ClassificationScheme, ClassificationSource};

    #[test]
    fn bulk_provider_merge_matches_individual_lookups_across_query_boundaries() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(schema::PROVIDERS).unwrap();
        let classification = |scheme, notation: &str| Classification { scheme, notation: notation.to_owned(), source: ClassificationSource::ExactEdition, evidence: vec![] };
        let heading = SubjectHeading { source: "library_of_congress".into(), authority: "lcsh".into(), components: vec!["Example".into()], authority_uri: None };
        let mut answers = Vec::new();
        for id in 0..650_i64 {
            let body = 978_000_000_000 + id;
            let sum = (0..12).map(|p| body / 10_i64.pow(11 - p) % 10 * if p % 2 == 0 { 1 } else { 3 }).sum::<i64>();
            let isbn = body * 10 + (10 - sum % 10) % 10;
            let mut answer = RichIsbnMetadataResult { requested_isbn: format!("{isbn:013}"), canonical_isbn13: Some(format!("{isbn:013}")), status: LookupStatus::NoMatch, matches: vec![] };
            let mut loc_codes = vec![];
            let mut lt_codes = vec![];
            let mut loc_status = "matched";
            let mut malformed = false;
            match id % 8 {
                0 => {
                    loc_codes.push(classification(ClassificationScheme::LibraryOfCongress, "QA76"));
                    malformed = true;
                }
                1 => {
                    loc_codes.push(classification(ClassificationScheme::DeweyDecimal, "385"));
                    lt_codes.push(classification(ClassificationScheme::LibraryOfCongress, "HE2751"));
                }
                2 => {
                    loc_status = "no_match";
                    lt_codes.push(classification(ClassificationScheme::LibraryOfCongress, "E1"));
                }
                3 => {
                    loc_status = "no_match";
                }
                4 | 5 | 6 => {
                    answer.matches.push(RichWorkMetadataMatch { open_library_work_id: Some("OL1W".into()), exact_edition_ids: vec![], classifications: vec![], authors: vec![], description: None, popularity: None, subjects: vec![] });
                    answer.status = LookupStatus::Matched;
                    if id % 8 == 4 {
                        answer.matches[0].classifications.push(classification(ClassificationScheme::LibraryOfCongress, "D1"));
                    }
                    if id % 8 == 5 {
                        answer.matches.push(answer.matches[0].clone());
                        answer.status = LookupStatus::Ambiguous;
                    }
                    if id % 8 == 6 {
                        answer.requested_isbn = "invalid".into();
                        answer.canonical_isbn13 = None;
                        answer.status = LookupStatus::InvalidIsbn;
                    }
                    malformed = true;
                }
                _ => {}
            }
            let loc_json = if malformed && id % 8 != 0 { b"bad json".to_vec() } else { serde_json::to_vec(&loc_codes).unwrap() };
            let lt_json = if malformed { b"bad json".to_vec() } else { serde_json::to_vec(&lt_codes).unwrap() };
            let subjects = if id % 8 == 7 { vec![heading.clone(), heading.clone()] } else { vec![] };
            for (provider, status, codes) in [("loc", loc_status, loc_json), ("librarything", "matched", lt_json)] {
                connection.execute("INSERT INTO provider_result VALUES(?1,?2,1,?3,?4,?5)", params![provider, isbn, status, codes, serde_json::to_vec(&subjects).unwrap()]).unwrap();
            }
            answers.push(answer);
        }
        answers.extend(answers[..10].to_vec());
        let mut expected = answers.clone();
        for answer in &mut expected {
            let isbn = answer.canonical_isbn13.as_deref().unwrap_or("0").parse().unwrap();
            apply_one(&connection, isbn, answer).unwrap();
        }
        apply_many(&connection, &mut answers).unwrap();
        assert_eq!(serde_json::to_value(&answers).unwrap(), serde_json::to_value(expected).unwrap());
        assert_eq!(answers[0].matches[0].classifications[0].notation, "QA76");
        assert_eq!(answers[1].matches[0].classifications.len(), 2);
        assert_eq!(answers[7].matches[0].subjects, vec![heading]);
    }
}
