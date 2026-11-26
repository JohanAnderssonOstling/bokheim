//! Compact immutable indexes for work-scoped edition evidence.
use crate::*;

const VERSION: i64 = 2;

#[derive(Deserialize)]
pub(crate) struct EditionEvidence {
    pub edition_id: i64,
    pub work_id: i64,
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub book_year: Option<i32>,
    pub authors: Vec<String>,
    pub isbn13: String,
    pub codes: Vec<String>,
}

pub(crate) fn index_path(source: &Path) -> PathBuf {
    let mut name = source.as_os_str().to_os_string();
    name.push(".work-editions-v2.sqlite3");
    PathBuf::from(name)
}

pub(crate) fn identity(source: &Path) -> Result<(i64, String), MetadataError> {
    let metadata = fs::metadata(source).map_err(error)?;
    let modified = metadata.modified().map_err(error)?.duration_since(UNIX_EPOCH).map_err(error)?.as_nanos().to_string();
    Ok((i64::try_from(metadata.len()).map_err(error)?, modified))
}

pub(crate) fn open_index(source: &Path) -> Result<Option<Pool<SqliteConnectionManager>>, MetadataError> {
    let path = index_path(source);
    if !path.try_exists().map_err(error)? {
        return Ok(None);
    }
    let pool = super::openlibrary::open_description_pool(&path)?;
    let c = pool.get().map_err(error)?;
    let (version, bytes, modified): (i64, i64, String) = c.query_row("SELECT version,source_bytes,source_modified_ns FROM index_source WHERE singleton=1", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map_err(error)?;
    if version != VERSION || (bytes, modified) != identity(source)? {
        return Err(MetadataError("work edition index is stale; rebuild it from the configured immutable work source".into()));
    }
    for sql in ["SELECT work_id,edition_id,evidence FROM edition_lcc LIMIT 0", "SELECT work_id,book_year,edition_id FROM edition_year LIMIT 0"] {
        c.prepare(sql).map_err(error)?;
    }
    drop(c);
    Ok(Some(pool))
}

/// Build beside the source. Requests never construct indexes or mutate imports.
pub fn index_work_editions(source: &Path) -> Result<serde_json::Value, MetadataError> {
    let started = Instant::now();
    let source = source.canonicalize().map_err(error)?;
    let output = index_path(&source);
    if output.try_exists().map_err(error)? {
        return Err(MetadataError(format!("{} already exists; remove the obsolete index before rebuilding", output.display())));
    }
    let before = identity(&source)?;
    let temporary = super::openlibrary::building_path(&output);
    let reservation = fs::OpenOptions::new().write(true).create_new(true).open(&temporary).map_err(error)?;
    drop(reservation);
    let result = (|| {
        let c = Connection::open(&temporary).map_err(error)?;
        c.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL; PRAGMA temp_store=FILE; PRAGMA cache_size=-65536;").map_err(error)?;
        c.execute("ATTACH DATABASE ?1 AS source", [crate::snapshot::immutable_uri(&source)?]).map_err(error)?;
        c.execute_batch(
            "PRAGMA source.cache_size=-16384;
            CREATE TABLE index_source(singleton INTEGER PRIMARY KEY CHECK(singleton=1),version INTEGER NOT NULL,source_bytes INTEGER NOT NULL,source_modified_ns TEXT NOT NULL);
            CREATE TABLE edition_lcc(work_id INTEGER NOT NULL,edition_id INTEGER NOT NULL,evidence BLOB NOT NULL,PRIMARY KEY(work_id,edition_id)) WITHOUT ROWID;
            CREATE TABLE edition_year(work_id INTEGER NOT NULL,book_year INTEGER NOT NULL,edition_id INTEGER NOT NULL,PRIMARY KEY(work_id,book_year,edition_id)) WITHOUT ROWID;
            BEGIN;",
        )
        .map_err(error)?;
        tracing::info!("indexing classified editions by work");
        let classified = c
            .execute(
                "INSERT INTO edition_lcc
            SELECT e.work_id,e.edition_id,CAST(json_object(
                'edition_id',e.edition_id,'work_id',e.work_id,'title',b.title,
                'subtitle',COALESCE(s.subtitle,CASE WHEN b.title IS NULL OR b.normalized_title=w.normalized_title THEN ws.subtitle END),
                'book_year',b.book_year,
                'authors',json((SELECT json_group_array(name) FROM (
                    SELECT a.name FROM source.edition_author ea JOIN source.author a USING(author_id)
                    WHERE ea.edition_id=e.edition_id AND a.name IS NOT NULL ORDER BY ea.position))),
                'isbn13',COALESCE((SELECT CAST(isbn13 AS TEXT) FROM source.edition_isbn i WHERE i.edition_id=e.edition_id ORDER BY isbn13 LIMIT 1),''),
                'codes',json((SELECT json_group_array(notation) FROM (
                    SELECT notation FROM source.edition_classification ec WHERE ec.edition_id=e.edition_id AND ec.scheme=2 ORDER BY notation)))
            ) AS BLOB)
            FROM (SELECT DISTINCT edition_id FROM source.edition_classification WHERE scheme=2) code
            CROSS JOIN source.edition e
            LEFT JOIN source.edition_bibliography b USING(edition_id)
            LEFT JOIN source.edition_subtitle s USING(edition_id)
            LEFT JOIN source.work_bibliography w USING(work_id)
            LEFT JOIN source.work_subtitle ws USING(work_id)
            WHERE e.edition_id=code.edition_id AND e.work_id>0 AND e.edition_id>0
            ORDER BY e.work_id,e.edition_id",
                [],
            )
            .map_err(error)?;
        tracing::info!(classified, "indexing publication years by work");
        let publications = c.execute("INSERT INTO edition_year SELECT e.work_id,b.book_year,b.edition_id FROM source.edition_bibliography b CROSS JOIN source.edition e WHERE e.edition_id=b.edition_id AND b.book_year IS NOT NULL AND e.work_id>0 AND e.edition_id>0 ORDER BY e.work_id,b.book_year,b.edition_id", []).map_err(error)?;
        if identity(&source)? != before {
            return Err(MetadataError("work source changed while building its edition index".into()));
        }
        c.execute("INSERT INTO index_source VALUES(1,?1,?2,?3)", params![VERSION, before.0, before.1]).map_err(error)?;
        tracing::info!(publications, "finalizing work edition index");
        c.execute_batch("COMMIT; DETACH DATABASE source; ANALYZE main; PRAGMA main.optimize;").map_err(error)?;
        let check: String = c.query_row("PRAGMA main.quick_check", [], |r| r.get(0)).map_err(error)?;
        if check != "ok" {
            return Err(MetadataError(format!("work edition index integrity check failed: {check}")));
        }
        Ok((classified, publications))
    })();
    match result {
        Ok((classified, publications)) => {
            fs::rename(&temporary, &output).map_err(error)?;
            Ok(serde_json::json!({"output":output,"classified_editions":classified,"publication_editions":publications,"bytes":fs::metadata(output).map_err(error)?.len(),"elapsed_seconds":started.elapsed().as_secs_f64()}))
        }
        Err(error) => {
            let _ = fs::remove_file(&temporary);
            Err(error)
        }
    }
}
