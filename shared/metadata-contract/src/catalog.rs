//! Ranking values projected from server evidence for the compact desktop catalog.
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub struct CatalogEvidence {
    pub popularity: Option<CatalogPopularity>,
    pub sources: Vec<String>,
    pub external_audio: bool,
}

/// Raw crawling and merge records stay on the server. Only ranking and source
/// attribution cross into the published catalog, with audio format resolved.
pub fn source_evidence(source: &str, metadata: &Value) -> Result<CatalogEvidence, String> {
    let mut sources = Vec::new();
    let popularity = if source == "bokheim:combined-volumes" {
        let mut selected: Option<CatalogPopularity> = None;
        for entry in metadata["components"].as_array().into_iter().flatten().flat_map(|component| component["source_entries"].as_array().into_iter().flatten()) {
            let Some(original_source) = entry["source"].as_str() else { continue };
            if !sources.iter().any(|name| name == original_source) {
                sources.push(original_source.to_owned());
            }
            let Some(value) = observation(original_source, &entry["metadata"])? else { continue };
            if selected.as_ref().is_none_or(|previous| stronger(&value, previous)) {
                selected = Some(value);
            }
        }
        selected
    } else {
        observation(source, metadata)?
    };
    Ok(CatalogEvidence { popularity, sources, external_audio: metadata["audiobook_download_url"].as_str().is_some() || metadata["audiobook_delivery"] == "external_download" })
}

/// Source observations used by catalog browsing. Litteraturbanken retains its
/// equally weighted score; other sources have their own participation measure.
/// Missing observations stay unknown; zero is a recorded value.
#[derive(Debug, Clone, Default, serde::Serialize, Deserialize, PartialEq)]
pub struct CatalogPopularity {
    pub popularity: Option<u64>,
    pub epub_popularity: Option<u64>,
    pub combined_popularity: Option<CombinedPopularity>,
    #[serde(skip)]
    pub source: Option<String>,
    #[serde(skip)]
    pub source_score: Option<f64>,
}

impl CatalogPopularity {
    pub fn score(&self) -> Option<f64> {
        self.source_score.or_else(|| self.combined_popularity.as_ref()?.score)
    }
}

#[derive(Debug, Clone, serde::Serialize, Deserialize, PartialEq)]
pub struct CombinedPopularity {
    pub score: Option<f64>,
    pub reading_percentile: Option<f64>,
    pub epub_percentile: Option<f64>,
}

pub fn source_priority(source: &str) -> u8 {
    match source {
        "publicdomainlibrary.org" => 0,
        "standardebooks.org" => 1,
        "litteraturbanken.se" => 2,
        "unglue.it" => 3,
        "loyalbooks.com" => 4,
        "gutenberg.org" => 5,
        _ => 6,
    }
}

pub fn observation(source: &str, metadata: &Value) -> Result<Option<CatalogPopularity>, String> {
    let mut value = CatalogPopularity { source: Some(source.into()), ..Default::default() };
    match source {
        "litteraturbanken.se" => {
            value = serde_json::from_value(metadata.clone()).map_err(|error| error.to_string())?;
            value.source = Some(source.into());
            // Scores and observation groups are computed by the client.
            value.source_score = None;
            if let Some(combined) = &value.combined_popularity {
                for percentile in [combined.score, combined.reading_percentile, combined.epub_percentile].into_iter().flatten() {
                    if !percentile.is_finite() || !(0.0..=100.0).contains(&percentile) {
                        return Err("catalog popularity must be between 0 and 100".into());
                    }
                }
                if let Some(score) = combined.score {
                    let valid = value.popularity.zip(value.epub_popularity).is_some() && combined.reading_percentile.zip(combined.epub_percentile).is_some_and(|(reading, epub)| (score - (reading + epub) / 2.0).abs() < 1e-7);
                    if !valid {
                        return Err("catalog popularity does not match its two equally weighted percentiles".into());
                    }
                }
            }
        }
        "gutenberg.org" => value.source_score = metadata["downloads"].as_u64().map(|count| count as f64),
        "standardebooks.org" => {
            // This is an observed listing position, not an exposed download
            // count. Reciprocal rank sorts lower ordinals first while keeping
            // the score positive for existing author/subject aggregation.
            let rank = &metadata["popularity"];
            value.source_score = (rank["scope"] == "sitewide" && rank["kind"] == "ordinal")
                .then(|| rank["rank"].as_u64().filter(|position| *position > 0 && rank["total"].as_u64().is_some_and(|total| *position <= total)))
                .flatten()
                .map(|position| 1.0 / position as f64);
        }
        "unglue.it" => value.source_score = metadata["website_metadata"]["downloads"].as_u64().map(|count| count as f64),
        "loyalbooks.com" => {
            // Language/category ranks have different cohorts. Only a sitewide
            // rank orders the whole source. Ranked books lead unranked books;
            // rating participation orders the latter without treating stars
            // as a popularity count. All scores are positive for aggregation.
            let rank = metadata["popularity"].as_array().into_iter().flatten().filter(|rank| rank["scope"] == "sitewide").filter_map(|rank| rank["rank"].as_u64().filter(|rank| *rank > 0)).min();
            value.source_score = rank.map(|rank| 1.0 + 1.0 / rank as f64).or_else(|| metadata["rating_count"].as_u64().map(|count| count as f64 / (count as f64 + 1.0)));
        }
        _ => return Ok(None),
    }
    Ok(Some(value))
}

pub fn stronger(value: &CatalogPopularity, previous: &CatalogPopularity) -> bool {
    // Prefer known evidence, then the requested source order. Within a source
    // keep one strongest intact observation, independent of database row order.
    if value.score().is_some() != previous.score().is_some() {
        return value.score().is_some();
    }
    let source = |value: &CatalogPopularity| source_priority(value.source.as_deref().unwrap_or(""));
    if source(value) != source(previous) {
        return source(value) < source(previous);
    }
    let priority = |value: &CatalogPopularity| {
        let combined = value.combined_popularity.as_ref();
        (value.score(), value.popularity, value.epub_popularity, combined.and_then(|combined| combined.reading_percentile), combined.and_then(|combined| combined.epub_percentile))
    };
    priority(value) > priority(previous)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn standardebooks_ordinal_rank_orders_within_its_source_without_inventing_counts() {
        let rank = |position| observation("standardebooks.org", &json!({"popularity":{"scope":"sitewide","kind":"ordinal","rank":position,"total":1530}})).unwrap().unwrap();
        let first = rank(1);
        let later = rank(20);
        assert!(stronger(&first, &later));
        assert!(first.score().unwrap() > later.score().unwrap());
        assert!(first.popularity.is_none());
        assert!(first.epub_popularity.is_none());
        assert!(rank(0).score().is_none());
        assert!(rank(1531).score().is_none());
        let downloads = observation("gutenberg.org", &json!({"downloads":10000})).unwrap().unwrap();
        assert!(stronger(&later, &downloads));
    }
}
