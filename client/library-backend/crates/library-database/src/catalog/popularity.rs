//! Source-specific popularity projection over immutable catalog evidence.
use super::{CatalogPopularity, CombinedPopularity, key_bytes};
use crate::DatabaseError;
use rusqlite::{Connection, Error as SqlError, types::Type};
use std::collections::HashMap;

pub(super) use metadata_contract::catalog::source_priority;
use metadata_contract::catalog::stronger;

pub(super) fn read_popularity_for(db: &Connection, key: Option<&str>) -> Result<HashMap<String, CatalogPopularity>, DatabaseError> {
    let mut popularity = HashMap::new();
    let sql = "SELECT CASE WHEN hosted.row_id IS NOT NULL THEN lower(hex(hosted.key)) END,CASE WHEN external.row_id IS NOT NULL THEN lower(hex(external.key)) END,
        ranking.name,entry.score,entry.popularity,entry.epub_popularity,entry.reading_percentile,entry.epub_percentile
        FROM source_entry entry JOIN source ranking ON ranking.id=entry.ranking_source_id
        LEFT JOIN book hosted ON hosted.row_id=entry.hosted_row_id LEFT JOIN book external ON external.row_id=entry.external_row_id
        WHERE (?1 IS NULL OR hosted.key=?1 OR external.key=?1)";
    let mut statement = db.prepare(sql)?;
    let rows = statement.query_map([key_bytes(key)?], |row| {
        let source: String = row.get(2)?;
        let score = row.get::<_, Option<f64>>(3)?;
        let reading_percentile = row.get::<_, Option<f64>>(6)?;
        let epub_percentile = row.get::<_, Option<f64>>(7)?;
        let count = |column| row.get::<_, Option<i64>>(column)?.map(u64::try_from).transpose().map_err(|error| SqlError::FromSqlConversionFailure(column, Type::Integer, Box::new(error)));
        let combined_popularity = (source == "litteraturbanken.se" && (score.is_some() || reading_percentile.is_some() || epub_percentile.is_some())).then_some(CombinedPopularity { score, reading_percentile, epub_percentile });
        Ok((
            row.get::<_, Option<String>>(0)?,
            row.get::<_, Option<String>>(1)?,
            CatalogPopularity { popularity: count(4)?, epub_popularity: count(5)?, combined_popularity, source_score: (source != "litteraturbanken.se").then_some(score).flatten(), source: Some(source) },
        ))
    })?;
    for row in rows {
        let (hosted, external, value) = row?;
        for identity in hosted.into_iter().chain(external) {
            match popularity.get_mut(&identity) {
                Some(previous) if stronger(&value, previous) => *previous = value.clone(),
                Some(_) => {}
                None => {
                    popularity.insert(identity, value.clone());
                }
            }
        }
    }
    Ok(popularity)
}

pub(super) fn read_popularity(db: &Connection) -> Result<HashMap<String, CatalogPopularity>, DatabaseError> {
    read_popularity_for(db, None)
}
