//! Source popularity stays separate from bibliographic enrichment. The two
//! Litteraturbanken measures have different scales, so combine their percentiles
//! over cataloged editions, giving tied observations the same average rank.
use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;
use serde_json::{from_slice, from_str, json, to_string, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

const SOURCE: &str = "litteraturbanken.se";

#[derive(Deserialize)]
struct Index {
    data: Vec<Observation>,
}

#[derive(Deserialize)]
struct Observation {
    lbworkid: String,
    #[serde(default)]
    generated_from_part: bool,
    popularity: Option<u64>,
    epub_popularity: Option<u64>,
}

struct Entry {
    id: String,
    url: String,
    book_hash: Option<String>,
    external_key: Option<String>,
}

/// Zero and missing are distinct. Average tied ranks span 0–100; a singleton
/// or an entirely tied cohort gets the neutral percentile 50.
fn percentiles(values: impl Iterator<Item = (String, Option<u64>)>) -> HashMap<String, f64> {
    let mut ranked = values.filter_map(|(id, value)| value.map(|value| (value, id))).collect::<Vec<_>>();
    ranked.sort_unstable();
    let mut result = HashMap::new();
    let mut start = 0;
    while start < ranked.len() {
        let mut end = start + 1;
        while end < ranked.len() && ranked[end].0 == ranked[start].0 {
            end += 1;
        }
        let percentile = if ranked.len() == 1 { 50.0 } else { 100.0 * (start + end - 1) as f64 / (2.0 * (ranked.len() - 1) as f64) };
        for (_, id) in &ranked[start..end] {
            result.insert(id.clone(), percentile);
        }
        start = end;
    }
    result
}

pub fn import(db: &Connection, path: &Path, observed_at: &str) -> Result<(), String> {
    if observed_at.trim().is_empty() {
        return Err("popularity observations need a timestamp".into());
    }
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    let input_digest = content_address::hash_reader(&mut bytes.as_slice()).map_err(|error| error.to_string())?.to_hex();
    let Index { data } = from_slice(&bytes).map_err(|error| error.to_string())?;
    let mut observations = BTreeMap::new();
    for observation in data.into_iter().filter(|row| !row.generated_from_part) {
        let id = observation.lbworkid.clone();
        if observations.insert(id.clone(), observation).is_some() {
            return Err(format!("duplicate popularity observation for {id}"));
        }
    }
    let tx = db.unchecked_transaction().map_err(|error| error.to_string())?;
    let mut entries = Vec::new();
    {
        let mut statement =
            tx.prepare("SELECT entry_id,entry_url,hash,NULL FROM book_source WHERE source=?1 UNION ALL SELECT entry_id,entry_url,NULL,entry_key FROM external_book WHERE source=?1 ORDER BY 1").map_err(|error| error.to_string())?;
        let rows = statement.query_map([SOURCE], |row| Ok(Entry { id: row.get(0)?, url: row.get(1)?, book_hash: row.get(2)?, external_key: row.get(3)? })).map_err(|error| error.to_string())?;
        for row in rows {
            let entry = row.map_err(|error| error.to_string())?;
            if !observations.contains_key(&entry.id) {
                return Err(format!("index has no popularity observation for catalog edition {}", entry.id));
            }
            entries.push(entry);
        }
    }
    let reading = percentiles(entries.iter().map(|entry| (entry.id.clone(), observations[&entry.id].popularity)));
    let epub = percentiles(entries.iter().map(|entry| (entry.id.clone(), observations[&entry.id].epub_popularity)));
    let mut scored = 0;
    for entry in &entries {
        let row = &observations[&entry.id];
        let previous: Option<String> = tx.query_row("SELECT metadata FROM source_entry WHERE source=?1 AND entry_id=?2", params![SOURCE, entry.id], |row| row.get(0)).optional().map_err(|error| error.to_string())?;
        let mut metadata: Value = previous.map(|value| from_str(&value)).transpose().map_err(|error| error.to_string())?.unwrap_or_else(|| json!({}));
        if !metadata.is_object() {
            return Err(format!("source metadata for {} is not an object", entry.id));
        }
        let read_percentile = reading.get(&entry.id).copied();
        let epub_percentile = epub.get(&entry.id).copied();
        let score = read_percentile.zip(epub_percentile).map(|(read, download)| (read + download) / 2.0);
        scored += usize::from(score.is_some());
        metadata["popularity"] = json!(row.popularity);
        metadata["epub_popularity"] = json!(row.epub_popularity);
        metadata["combined_popularity"] = json!({
            "score": score, "reading_percentile": read_percentile, "epub_percentile": epub_percentile,
            "reading_weight": 0.5, "epub_weight": 0.5,
            "method": "average_tied_rank_percentile_v1", "cohort_source": SOURCE,
            "cohort_size": entries.len(), "reading_sample_size": reading.len(), "epub_sample_size": epub.len(),
            "observed_at": observed_at, "input_blake3": input_digest
        });
        tx.execute("INSERT INTO source_entry(source,entry_id,entry_url,book_hash,external_entry_key,metadata) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(source,entry_id) DO UPDATE SET entry_url=excluded.entry_url,book_hash=excluded.book_hash,external_entry_key=excluded.external_entry_key,metadata=excluded.metadata", params![SOURCE, entry.id, entry.url, entry.book_hash, entry.external_key, to_string(&metadata).map_err(|error| error.to_string())?]).map_err(|error| error.to_string())?;
    }
    tx.commit().map_err(|error| error.to_string())?;
    println!("Retained popularity for {} Litteraturbanken editions; {scored} combined scores", entries.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tied_values_share_rank_and_missing_is_not_zero() {
        let values = [("zero", Some(0)), ("a", Some(10)), ("b", Some(10)), ("high", Some(100)), ("unknown", None)];
        let ranks = percentiles(values.into_iter().map(|(id, value)| (id.into(), value)));
        assert_eq!(ranks["zero"], 0.0);
        assert_eq!(ranks["a"], 50.0);
        assert_eq!(ranks["b"], 50.0);
        assert_eq!(ranks["high"], 100.0);
        assert!(!ranks.contains_key("unknown"));
        assert_eq!(percentiles(std::iter::once(("single".into(), Some(12))))["single"], 50.0);
        assert!(percentiles([("a".into(), Some(12)), ("b".into(), Some(12))].into_iter()).values().all(|value| *value == 50.0));
        assert!(percentiles(std::iter::empty()).is_empty());
    }

    #[test]
    fn scores_use_catalog_cohort_and_preserve_existing_source_evidence() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::curation::open(dir.path()).unwrap();
        for (id, key) in [("a", "1"), ("b", "2"), ("missing", "3")] {
            db.execute("INSERT INTO external_book(entry_key,source,entry_id,entry_url,license,title,author,record) VALUES(?1,?2,?3,'https://litteraturbanken.se/book/info','test',?3,'Author','{}')", params![key, SOURCE, id]).unwrap();
        }
        db.execute("INSERT INTO source_entry VALUES(?1,'a','https://litteraturbanken.se/book/info',NULL,'1','{\"license\":\"retain\"}')", [SOURCE]).unwrap();
        let path = dir.path().join("index.json");
        std::fs::write(&path, r#"{"data":[{"lbworkid":"a","popularity":0,"epub_popularity":20},{"lbworkid":"b","popularity":10,"epub_popularity":0},{"lbworkid":"missing","popularity":null,"epub_popularity":null},{"lbworkid":"outside","popularity":1000,"epub_popularity":1000},{"lbworkid":"a","generated_from_part":true,"popularity":999,"epub_popularity":999}]}"#).unwrap();
        import(&db, &path, "2026-10-12T10:00:00Z").unwrap();
        let evidence = |id: &str| -> Value { from_str(&db.query_row("SELECT metadata FROM source_entry WHERE entry_id=?1", [id], |row| row.get::<_, String>(0)).unwrap()).unwrap() };
        let a = evidence("a");
        assert_eq!(a["license"], "retain");
        assert_eq!(a["popularity"], 0);
        assert_eq!(a["combined_popularity"]["score"], 50.0);
        assert_eq!(a["combined_popularity"]["cohort_size"], 3);
        assert_eq!(a["combined_popularity"]["reading_sample_size"], 2);
        assert_eq!(evidence("b")["combined_popularity"]["score"], 50.0);
        assert!(evidence("missing")["combined_popularity"]["score"].is_null());
        assert_eq!(db.query_row("SELECT record FROM external_book WHERE entry_id='a'", [], |row| row.get::<_, String>(0)).unwrap(), "{}");
        import(&db, &path, "2026-10-12T10:00:00Z").unwrap();
        assert_eq!(evidence("a"), a);
        std::fs::write(&path, r#"{"data":[{"lbworkid":"a","popularity":1,"epub_popularity":1}]}"#).unwrap();
        assert!(import(&db, &path, "2026-10-12").is_err());
        assert_eq!(evidence("a"), a);
    }
}
