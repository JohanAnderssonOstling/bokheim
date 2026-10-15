//! Extend an immutable answer snapshot with classified editions lacking ISBNs.
use super::{identity, *};
use crate::snapshot as answer_snapshot;
use reqwest::Url;
use rusqlite::OptionalExtension;

#[derive(Debug, Serialize)]
pub struct EditionIndexReport {
    pub eligible_editions: u64,
    pub visited_editions: u64,
    pub indexed_editions: u64,
    pub selection_seconds: f64,
    pub indexing_seconds: f64,
    pub elapsed_seconds: f64,
    pub file_bytes: u64,
    pub complete: bool,
}

/// Publish only index additions in a new cumulative overlay. ISBN answers and
/// the immutable base remain untouched. A limit supports measuring a real batch.
pub fn extend_edition_index(snapshot: &Path, previous: &Path, output: &Path, limit: Option<usize>) -> Result<EditionIndexReport, MetadataError> {
    if output.exists() || limit == Some(0) {
        return Err(MetadataError("index output must be new and any limit must be positive".into()));
    }
    let started = Instant::now();
    let inputs = incremental::InputIdentity::read(snapshot, None, None)?;
    let resolver = Resolver::open(snapshot, None)?;
    let source = resolver.connection()?;
    validate_source(&source)?;
    let old = answer_snapshot::open(previous)?;
    answer_snapshot::require_separate(&old, snapshot, "index source")?;
    let (version, date): (i64, String) = old.query_row("SELECT schema_version,dump_date FROM snapshot WHERE singleton=1", [], |r| Ok((r.get(0)?, r.get(1)?))).map_err(error)?;
    if version != schema::ANSWER_VERSION || date != resolver.state.snapshot.dump_date {
        return Err(MetadataError("edition indexing requires the same answer schema and imported dump date".into()));
    }
    if old.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='answer_build')", [], |r| r.get::<_, bool>(0)).map_err(error)? {
        let prior: Option<String> = old.query_row("SELECT inputs FROM answer_build WHERE singleton=1", [], |r| r.get(0)).optional().map_err(error)?;
        if let Some(prior) = prior {
            serde_json::from_str::<incremental::InputIdentity>(&prior).map_err(error)?.verify_source(snapshot)?;
        }
    }
    let temporary = output.with_extension(format!("indexing-{}", std::process::id()));
    let _reservation = fs::OpenOptions::new().write(true).create_new(true).open(&temporary).map_err(error)?;
    let result = (|| {
        let out = answer_snapshot::create_update(previous, &temporary)?;
        out.execute_batch("BEGIN IMMEDIATE;").map_err(error)?;
        let mut uri = Url::from_file_path(snapshot.canonicalize().map_err(error)?).map_err(|_| MetadataError("invalid index source path".into()))?;
        uri.set_query(Some("mode=ro"));
        out.execute("ATTACH DATABASE ?1 AS index_source", [uri.as_str()]).map_err(error)?;
        tracing::info!("selecting classified editions without ISBNs");
        let selecting = Instant::now();
        out.execute_batch(
            "CREATE TEMP TABLE pending_edition(edition_id INTEGER PRIMARY KEY);
             INSERT OR IGNORE INTO pending_edition SELECT c.edition_id FROM index_source.edition_classification c
             WHERE c.scheme=2 AND NOT EXISTS(SELECT 1 FROM index_source.edition_isbn i WHERE i.edition_id=c.edition_id)
             AND NOT EXISTS(SELECT 1 FROM edition_candidate e WHERE e.edition_id=c.edition_id);
             INSERT OR IGNORE INTO pending_edition SELECT e.edition_id FROM
                 (SELECT work_id FROM index_source.work_classification WHERE scheme=2 UNION SELECT work_id FROM index_source.edition_work_classification WHERE scheme=2) c
                 JOIN index_source.edition e USING(work_id)
             WHERE NOT EXISTS(SELECT 1 FROM index_source.edition_isbn i WHERE i.edition_id=e.edition_id)
             AND NOT EXISTS(SELECT 1 FROM edition_candidate present WHERE present.edition_id=e.edition_id);",
        )
        .map_err(error)?;
        let eligible_editions = out.query_row("SELECT count(*) FROM pending_edition", [], |r| r.get::<_, i64>(0)).map_err(error)? as u64;
        let selection_seconds = selecting.elapsed().as_secs_f64();
        tracing::info!(eligible_editions, selection_seconds, "selected additional edition evidence owners");
        let indexing = Instant::now();
        let before = out.query_row("SELECT count(*) FROM main.delta_edition_candidate", [], |r| r.get::<_, i64>(0)).map_err(error)? as u64;
        let mut visited_editions = 0_u64;
        let mut after = i64::MIN;
        loop {
            let remaining = limit.map_or(4096, |limit| limit.saturating_sub(visited_editions as usize).min(4096));
            if remaining == 0 {
                break;
            }
            let selected = {
                let mut stmt = out.prepare("SELECT edition_id FROM pending_edition WHERE edition_id>?1 ORDER BY edition_id LIMIT ?2").map_err(error)?;
                let rows = stmt.query_map(params![after, remaining as i64], |r| r.get::<_, i64>(0)).map_err(error)?.collect::<Result<BTreeSet<_>, _>>().map_err(error)?;
                rows
            };
            if selected.is_empty() {
                break;
            }
            after = *selected.last().unwrap();
            visited_editions += selected.len() as u64;
            identity::build_candidates_for(&source, &out, Some(&selected))?;
            tracing::info!(visited_editions, elapsed_seconds = indexing.elapsed().as_secs_f64(), "extending edition search index");
        }
        let indexed_editions = out.query_row("SELECT count(*) FROM main.delta_edition_candidate", [], |r| r.get::<_, i64>(0)).map_err(error)? as u64 - before;
        let indexing_seconds = indexing.elapsed().as_secs_f64();
        inputs.verify_source(snapshot)?;
        out.execute("UPDATE main.snapshot SET built_at_ms=?1 WHERE singleton=1", [now_ms()]).map_err(error)?;
        out.execute_batch("COMMIT; PRAGMA main.optimize;").map_err(error)?;
        let integrity: String = out.query_row("PRAGMA main.quick_check", [], |r| r.get(0)).map_err(error)?;
        if integrity != "ok" {
            return Err(MetadataError(format!("index integrity check failed: {integrity}")));
        }
        drop(out);
        // Refuse publication if the retained base changed during indexing.
        drop(answer_snapshot::open(&temporary)?);
        fs::hard_link(&temporary, output).map_err(error)?;
        Ok(EditionIndexReport {
            eligible_editions,
            visited_editions,
            indexed_editions,
            selection_seconds,
            indexing_seconds,
            elapsed_seconds: started.elapsed().as_secs_f64(),
            file_bytes: fs::metadata(output).map_err(error)?.len(),
            complete: visited_editions == eligible_editions,
        })
    })();
    let _ = fs::remove_file(&temporary);
    result
}
