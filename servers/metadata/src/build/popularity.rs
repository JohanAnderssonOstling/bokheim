//! Aggregate the optional ratings dump once and reuse it for answers and queue priority.
use super::{error, open_library_id, sql_placeholders, MetadataError};
use flate2::read::MultiGzDecoder;
use metadata_contract::{RichIsbnMetadataResult, WorkPopularity};
use rusqlite::{params_from_iter, Connection};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

pub(super) fn load(connection: &Connection, path: Option<&Path>) -> Result<(), MetadataError> {
    connection.execute_batch("CREATE TEMP TABLE build_rating(work_id INTEGER PRIMARY KEY,count INTEGER NOT NULL)").map_err(error)?;
    let Some(path) = path else { return Ok(()) };
    let reader = BufReader::new(MultiGzDecoder::new(File::open(path).map_err(error)?));
    let mut insert = connection.prepare_cached("INSERT INTO build_rating VALUES(?1,1) ON CONFLICT(work_id) DO UPDATE SET count=count+1").map_err(error)?;
    for line in reader.lines() {
        let line = line.map_err(error)?;
        let mut fields = line.split('\t');
        let work = fields.next().and_then(|id| open_library_id(id.trim_matches('"'), 'W'));
        let _edition = fields.next();
        let rating = fields.next().and_then(|rating| rating.trim_matches('"').parse::<u8>().ok());
        if let (Some(work), Some(1..=5)) = (work, rating) {
            insert.execute([work]).map_err(error)?;
        }
    }
    Ok(())
}

pub(super) fn attach(connection: &Connection, answers: &mut [RichIsbnMetadataResult], snapshot_date: &str) -> Result<(), MetadataError> {
    let ids = answers.iter().flat_map(|answer| &answer.matches).filter_map(|matched| matched.open_library_work_id.as_deref().and_then(|id| open_library_id(id, 'W'))).collect::<BTreeSet<_>>();
    let mut counts = BTreeMap::<i64, u64>::new();
    for chunk in ids.into_iter().collect::<Vec<_>>().chunks(500) {
        let sql = format!("SELECT work_id,count FROM build_rating WHERE work_id IN ({})", sql_placeholders(chunk.len()));
        let mut query = connection.prepare_cached(&sql).map_err(error)?;
        for row in query.query_map(params_from_iter(chunk), |row| Ok((row.get(0)?, row.get::<_, i64>(1)?))).map_err(error)? {
            let (work, count) = row.map_err(error)?;
            counts.insert(work, u64::try_from(count).map_err(error)?);
        }
    }
    for matched in answers.iter_mut().flat_map(|answer| &mut answer.matches) {
        matched.popularity = matched.open_library_work_id.as_deref().and_then(|id| {
            let rating_count = *counts.get(&open_library_id(id, 'W')?)?;
            Some(WorkPopularity { open_library_work_id: id.to_owned(), rating_count, snapshot_date: snapshot_date.to_owned() })
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::tests::snapshot_from;
    use crate::{build_answers_incremental, build_answers_with_ratings, MetadataService};
    use flate2::{write::GzEncoder, Compression};
    use std::io::Write;

    fn write_ratings(path: &Path, rows: &[u8]) {
        let mut gzip = GzEncoder::new(File::create(path).unwrap(), Compression::fast());
        gzip.write_all(rows).unwrap();
        gzip.finish().unwrap();
    }

    #[test]
    fn validated_ratings_enrich_answers_and_rank_the_queue_from_one_count() {
        let directory = tempfile::tempdir().unwrap();
        let snapshot = snapshot_from(
            directory.path(),
            &[serde_json::json!({"key":"/books/OL1M","works":[{"key":"/works/OL10W"}],"isbn_13":["9780306406157"]}), serde_json::json!({"key":"/books/OL2M","works":[{"key":"/works/OL20W"}],"isbn_13":["9780131103627"]})],
            &[],
            &[],
        );
        let ratings = directory.path().join("ratings.gz");
        write_ratings(
            &ratings,
            b"Work Key\tEdition Key\tRating\tDate\n\"/works/OL10W\"\t\"\"\t\"5\"\t2026-09-01\nOL10W\tOL1M\t3\t2026-09-02\nOL10W\t\t0\t2026-09-03\nOL10W\t\t6\t2026-09-03\nOL10W\t\tinvalid\t2026-09-03\nOL10W\t\nnot-a-work\t\t5\t2026-09-03\n",
        );
        let output = directory.path().join("answers.sqlite");
        build_answers_with_ratings(&snapshot, None, None, None, Some(&ratings), &output).unwrap();
        let service = MetadataService::open(&output, &directory.path().join("demand.sqlite")).unwrap();
        let response = service.enrich_rich(vec!["9780306406157".into(), "9780131103627".into()]).unwrap();
        let evidence = response[0].matches[0].popularity.as_ref().unwrap();
        assert_eq!(evidence.rating_count, 2);
        assert_eq!(evidence.open_library_work_id, "OL10W");
        assert_eq!(evidence.snapshot_date, service.state.snapshot.dump_date);
        assert!(response[1].matches[0].popularity.is_none());
        let connection = Connection::open(&output).unwrap();
        assert_eq!(connection.query_row("SELECT rank FROM wanted WHERE work_id=10", [], |row| row.get::<_, i64>(0)).unwrap(), -2);
        assert_eq!(connection.query_row("SELECT rank FROM wanted WHERE work_id=20", [], |row| row.get::<_, i64>(0)).unwrap(), 0);
        assert!(!connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='build_rating')", [], |row| row.get::<_, bool>(0)).unwrap());

        // Ratings belong to the immutable input identity: changing the dump
        // rebuilds every answer rather than retaining stale popularity.
        write_ratings(&ratings, b"OL10W\t\t5\t2026-09-01\n");
        let updated = directory.path().join("updated.sqlite");
        let report = build_answers_incremental(&snapshot, None, None, None, Some(&ratings), &output, &updated).unwrap();
        assert_eq!(report.mode, "full");
        let updated_service = MetadataService::open(&updated, &directory.path().join("updated-demand.sqlite")).unwrap();
        assert_eq!(updated_service.enrich_rich(vec!["9780306406157".into()]).unwrap()[0].matches[0].popularity.as_ref().unwrap().rating_count, 1);

        let omitted = directory.path().join("omitted.sqlite");
        build_answers_with_ratings(&snapshot, None, None, None, None, &omitted).unwrap();
        let omitted_service = MetadataService::open(&omitted, &directory.path().join("omitted-demand.sqlite")).unwrap();
        assert!(omitted_service.enrich_rich(vec!["9780306406157".into()]).unwrap()[0].matches[0].popularity.is_none());
    }
}
