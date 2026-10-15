//! Keep explicitly declared work LCC separate from classifications of its editions.
use super::*;
use metadata_contract::{authority_subjects::OPEN_LIBRARY_WORK_PROVIDER, AuthoritySubjectMatch, ClassificationEvidence};

pub(crate) fn build_work_lookup(source: &Connection, out: &Connection) -> Result<u64, MetadataError> {
    out.execute("DELETE FROM authority_title WHERE source=?1", [OPEN_LIBRARY_WORK_PROVIDER]).map_err(error)?;
    let mut query = source.prepare("SELECT c.work_id,b.title,c.notation FROM work_classification c JOIN work_bibliography b USING(work_id) WHERE c.scheme=2 ORDER BY c.work_id,c.notation").map_err(error)?;
    let mut rows = query.query([]).map_err(error)?;
    let mut current: Option<(i64, String, BTreeSet<String>)> = None;
    let mut count = 0;
    while let Some(row) = rows.next().map_err(error)? {
        let id: i64 = row.get(0).map_err(error)?;
        if current.as_ref().is_some_and(|record| record.0 != id) {
            count += put_work(out, current.take().unwrap())?;
        }
        let record = current.get_or_insert((id, row.get(1).map_err(error)?, BTreeSet::new()));
        let notation: String = row.get(2).map_err(error)?;
        if let Some(notation) = metadata_contract::classification_consensus_notation(ClassificationScheme::LibraryOfCongress, &notation) {
            record.2.insert(notation);
        }
    }
    if let Some(record) = current {
        count += put_work(out, record)?;
    }
    Ok(count)
}

fn put_work(out: &Connection, (id, title, codes): (i64, String, BTreeSet<String>)) -> Result<u64, MetadataError> {
    if codes.is_empty() {
        return Ok(0);
    }
    let work = format!("OL{id}W");
    let classifications = codes
        .into_iter()
        .map(|notation| Classification {
            notation,
            scheme: ClassificationScheme::LibraryOfCongress,
            source: ClassificationSource::Work,
            evidence: vec![ClassificationEvidence { method: format!("{OPEN_LIBRARY_WORK_PROVIDER}_title_author:{work}"), isbn13: String::new(), open_library_work_id: work.clone(), open_library_edition_id: String::new() }],
        })
        .collect();
    let matched = AuthoritySubjectMatch { main_title: title.clone(), title, authors: Vec::new(), provider_id: OPEN_LIBRARY_WORK_PROVIDER.into(), record_ids: vec![work.clone()], classifications };
    put_authority(out, matched, vec![format!("openlibrary-work:{work}")])?;
    Ok(1)
}

/// Add the work lookup without rebuilding ISBN answers or replacing edition evidence.
/// A transaction keeps the previous index intact if indexing fails.
pub fn index_work_classifications(snapshot: &Path, answers: &Path) -> Result<serde_json::Value, MetadataError> {
    let resolver = Resolver::open(snapshot, None)?;
    let source = resolver.connection()?;
    let out = Connection::open(answers).map_err(error)?;
    crate::snapshot::require_separate(&out, snapshot, "work index input")?;
    if crate::snapshot::is_update(&out)? {
        return Err(MetadataError("work indexing requires a full answer database".into()));
    }
    let (version, date): (i64, String) = out.query_row("SELECT schema_version,dump_date FROM snapshot WHERE singleton=1", [], |r| Ok((r.get(0)?, r.get(1)?))).map_err(error)?;
    if version != schema::ANSWER_VERSION || date != resolver.state.snapshot.dump_date {
        return Err(MetadataError("work indexing requires the original imported dump date and answer schema".into()));
    }
    out.busy_timeout(Duration::from_secs(30)).map_err(error)?;
    out.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; BEGIN IMMEDIATE").map_err(error)?;
    let started = Instant::now();
    let previous: i64 = out.query_row("SELECT count(*) FROM authority_title WHERE source=?1", [OPEN_LIBRARY_WORK_PROVIDER], |r| r.get(0)).map_err(error)?;
    let works = build_work_lookup(&source, &out)?;
    out.execute_batch("COMMIT").map_err(error)?;
    Ok(serde_json::json!({"works": works, "previous_work_rows": previous, "dump_date": date, "elapsed_seconds": started.elapsed().as_secs_f64()}))
}
