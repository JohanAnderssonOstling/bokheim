//! Source-specific popularity projection over immutable catalog evidence.
use super::{CatalogPopularity, CombinedPopularity};
use crate::DatabaseError;
use rusqlite::{Connection, Error as SqlError, types::Type};
use std::collections::HashMap;

use metadata_contract::catalog::{source_priority as evidence_source_priority, stronger as stronger_evidence};

pub(super) fn source_priority(source: &str) -> u8 {
    // Retained for acquisition preferences and non-popularity browsing.
    // Popularity order never uses this source priority.
    evidence_source_priority(if source == "gutenberg.org" { "loyalbooks.com" } else { source })
}

fn stronger(value: &CatalogPopularity, previous: &CatalogPopularity) -> bool {
    // Every published source score uses the same scale. Select the strongest
    // normalized observation without preferring a provider over a higher score.
    if let (Some(score), Some(previous_score)) = (value.score(), previous.score()) {
        if score != previous_score {
            return score > previous_score;
        }
    }
    stronger_evidence(value, previous)
}

fn visit_popularity(db: &Connection, key: Option<&[u8; 32]>, mut receive: impl FnMut(Option<[u8; 32]>, Option<[u8; 32]>, CatalogPopularity)) -> Result<(), DatabaseError> {
    let sql = "SELECT hosted.key,external.key,
        ranking.name,entry.score,entry.popularity,entry.epub_popularity,entry.reading_percentile,entry.epub_percentile
        FROM source_entry entry JOIN source ranking ON ranking.id=entry.ranking_source_id
        LEFT JOIN book hosted ON hosted.row_id=entry.hosted_row_id LEFT JOIN book external ON external.row_id=entry.external_row_id
        WHERE (?1 IS NULL OR hosted.key=?1 OR external.key=?1)";
    let mut statement = db.prepare(sql)?;
    let rows = statement.query_map([key.map(|key| key.as_slice())], |row| {
        let source: String = row.get(2)?;
        let score = row.get::<_, Option<f64>>(3)?;
        let reading_percentile = row.get::<_, Option<f64>>(6)?;
        let epub_percentile = row.get::<_, Option<f64>>(7)?;
        let count = |column| row.get::<_, Option<i64>>(column)?.map(u64::try_from).transpose().map_err(|error| SqlError::FromSqlConversionFailure(column, Type::Integer, Box::new(error)));
        let combined_popularity = (source == "litteraturbanken.se" && (score.is_some() || reading_percentile.is_some() || epub_percentile.is_some())).then_some(CombinedPopularity { score, reading_percentile, epub_percentile });
        Ok((
            row.get::<_, Option<[u8; 32]>>(0)?,
            row.get::<_, Option<[u8; 32]>>(1)?,
            CatalogPopularity { popularity: count(4)?, epub_popularity: count(5)?, combined_popularity, source_score: (source != "litteraturbanken.se").then_some(score).flatten(), source: Some(source) },
        ))
    })?;
    for row in rows {
        let (hosted, external, value) = row?;
        receive(hosted, external, value);
    }
    Ok(())
}

pub(super) fn read_popularity_for(db: &Connection, key: &[u8; 32]) -> Result<Option<CatalogPopularity>, DatabaseError> {
    let mut selected = None;
    visit_popularity(db, Some(key), |_, _, value| {
        if selected.as_ref().is_none_or(|previous| stronger(&value, previous)) {
            selected = Some(value);
        }
    })?;
    Ok(selected)
}

pub(super) fn read_popularity(db: &Connection) -> Result<HashMap<[u8; 32], CatalogPopularity>, DatabaseError> {
    let mut popularity = HashMap::new();
    visit_popularity(db, None, |hosted, external, value| {
        for identity in hosted.into_iter().chain(external) {
            match popularity.get_mut(&identity) {
                Some(previous) if stronger(&value, previous) => *previous = value.clone(),
                Some(_) => {}
                None => {
                    popularity.insert(identity, value.clone());
                }
            }
        }
    })?;
    Ok(popularity)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_sources_select_the_higher_normalized_score_independent_of_source_order() {
        let value = |source: &str, score| CatalogPopularity { source: Some(source.into()), source_score: score, ..Default::default() };
        let gutenberg = value("gutenberg.org", Some(90.0));
        let loyalbooks = value("loyalbooks.com", Some(25.0));
        assert_eq!(source_priority("gutenberg.org"), source_priority("loyalbooks.com"));
        assert!(stronger(&gutenberg, &loyalbooks));
        assert!(!stronger(&loyalbooks, &gutenberg));
        assert!(stronger(&value("loyalbooks.com", Some(95.0)), &gutenberg));
        assert!(stronger(&value("gutenberg.org", Some(0.0)), &value("loyalbooks.com", None)));
        assert!(stronger(&value("bilinguator.com", Some(50.0)), &value("standardebooks.org", Some(25.0))));
        assert!(stronger(&value("unglue.it", Some(90.0)), &value("litteraturbanken.se", Some(40.0))));
        assert!(stronger(&value("gutenberg.org", Some(1.0)), &value("publicdomainlibrary.org", None)));
        assert!(source_priority("standardebooks.org") < source_priority("gutenberg.org"));
    }
}
