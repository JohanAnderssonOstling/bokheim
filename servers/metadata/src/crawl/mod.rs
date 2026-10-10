//! One ranked queue; LC completes before LibraryThing is eligible.
use crate::crawl::library_of_congress::{LibraryOfCongressClient, LibraryOfCongressLookup};
use crate::schema::{self, now_ms};
use crate::*;

pub(crate) mod library_of_congress;
pub(crate) mod librarything;

pub(crate) fn open_provider(path: &Path) -> Result<Connection, MetadataError> {
    let c = Connection::open(path).map_err(error)?;
    c.busy_timeout(Duration::from_secs(5)).map_err(error)?;
    c.execute_batch(schema::PROVIDERS).map_err(error)?;
    Ok(c)
}
pub(crate) fn save_result(c: &Connection, provider: &str, isbn: Isbn13, status: &str, codes: &[Classification], subjects: &[SubjectHeading]) -> Result<(), MetadataError> {
    c.execute(
        "INSERT INTO provider_result VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(provider,isbn13) DO UPDATE SET fetched_at_ms=excluded.fetched_at_ms,status=excluded.status,classifications=excluded.classifications,subjects=excluded.subjects",
        params![provider, isbn, now_ms(), status, serde_json::to_vec(codes).map_err(error)?, serde_json::to_vec(subjects).map_err(error)?],
    )
    .map_err(error)?;
    c.execute("DELETE FROM provider_retry WHERE provider=?1 AND isbn13=?2", params![provider, isbn]).map_err(error)?;
    Ok(())
}
pub(crate) fn save_loc(c: &Connection, isbn: Isbn13, value: &LibraryOfCongressLookup) -> Result<(), MetadataError> {
    match value {
        LibraryOfCongressLookup::Matched { classifications, subjects } => save_result(c, "loc", isbn, "matched", classifications, subjects),
        LibraryOfCongressLookup::NoMatch => save_result(c, "loc", isbn, "no_match", &[], &[]),
        LibraryOfCongressLookup::Ambiguous => save_result(c, "loc", isbn, "ambiguous", &[], &[]),
    }
}
pub(crate) fn load_loc(c: &Connection, isbn: Isbn13) -> Result<Option<LibraryOfCongressLookup>, MetadataError> {
    let row =
        c.query_row("SELECT status,classifications,subjects FROM provider_result WHERE provider='loc' AND isbn13=?1", [isbn], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?, r.get::<_, Vec<u8>>(2)?))).optional().map_err(error)?;
    row.map(|(status, codes, subjects)| match status.as_str() {
        "matched" => Ok(LibraryOfCongressLookup::Matched { classifications: serde_json::from_slice(&codes).map_err(error)?, subjects: serde_json::from_slice(&subjects).map_err(error)? }),
        "no_match" => Ok(LibraryOfCongressLookup::NoMatch),
        "ambiguous" => Ok(LibraryOfCongressLookup::Ambiguous),
        _ => Err(MetadataError("invalid provider status".into())),
    })
    .transpose()
}
pub(crate) fn reserve_librarything(c: &Connection, now: i64) -> Result<(), MetadataError> {
    let tx = c.unchecked_transaction().map_err(error)?;
    // The write takes the SQLite writer lock before reading quota, across processes.
    tx.execute("INSERT OR IGNORE INTO provider_quota VALUES('librarything',?1,0)", [now / 86_400_000]).map_err(error)?;
    let used: i64 = tx.query_row("SELECT used FROM provider_quota WHERE provider='librarything' AND day=?1", [now / 86_400_000], |r| r.get(0)).map_err(error)?;
    if used >= 1000 {
        return Err(MetadataError("LibraryThing daily request limit reached".into()));
    }
    let next = tx.query_row("SELECT next_ms FROM provider_rate WHERE provider='librarything'", [], |r| r.get::<_, i64>(0)).optional().map_err(error)?.unwrap_or(0);
    if next > now {
        return Err(MetadataError("LibraryThing request spacing limit".into()));
    }
    tx.execute("UPDATE provider_quota SET used=used+1 WHERE provider='librarything' AND day=?1", [now / 86_400_000]).map_err(error)?;
    tx.execute("INSERT INTO provider_rate VALUES('librarything',?1) ON CONFLICT(provider) DO UPDATE SET next_ms=excluded.next_ms", [now + 2000]).map_err(error)?;
    tx.commit().map_err(error)
}

/// Reloads the ranked wanted table each pass, so swapping the snapshot feeds a running crawler.
/// It never queries ISBN answers or owns a dump database.
pub async fn crawl(answers: &Path, providers: &Path, key: Option<&Path>, once: bool) -> Result<(), MetadataError> {
    let mut db = open_provider(providers)?;
    db.execute_batch("PRAGMA temp_store=FILE").map_err(error)?;
    let mut loaded = None;
    let lc = LibraryOfCongressClient::new()?.with_cache(providers)?;
    let lt = key.map(|k| crate::crawl::librarything::LibraryThingClient::open(k, providers)).transpose()?;
    loop {
        let wanted = crate::build::read_connection(answers)?;
        let version: i64 = wanted.query_row("SELECT schema_version FROM snapshot WHERE singleton=1", [], |r| r.get(0)).map_err(error)?;
        if version != schema::ANSWER_VERSION {
            return Err(MetadataError("unsupported wanted schema".into()));
        }
        let signature: (i64, String, i64) = wanted.query_row("SELECT built_at_ms,dump_date,provider_results_through_ms FROM snapshot WHERE singleton=1", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map_err(error)?;
        if loaded.as_ref() != Some(&signature) {
            // Copy only the queue into TEMP; provider joins remain local and indexed.
            db.execute_batch("DROP TABLE IF EXISTS temp.wanted; CREATE TEMP TABLE wanted(isbn13 INTEGER PRIMARY KEY,rank INTEGER,work_id INTEGER); CREATE INDEX temp.wanted_by_work ON wanted(work_id);").map_err(error)?;
            {
                let tx = db.transaction().map_err(error)?;
                let mut q = wanted.prepare("SELECT isbn13,rank,work_id FROM wanted ORDER BY rank,isbn13").map_err(error)?;
                for row in q.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, Option<i64>>(2)?))).map_err(error)? {
                    let (isbn, rank, work) = row.map_err(error)?;
                    tx.execute("INSERT INTO temp.wanted VALUES(?1,?2,?3)", params![isbn, rank, work]).map_err(error)?;
                }
                tx.commit().map_err(error)?;
            }
            loaded = Some(signature);
        }
        drop(wanted);
        let pending = select_pending(&db, "loc", schema::now_ms(), crate::crawl::library_of_congress::MAX_BATCH_ISBNS)?;
        if !pending.is_empty() {
            if let Err(e) = lc.lookup_batch(&pending).await {
                tracing::warn!(%e,"LC crawl batch failed");
            }
        }
        if let Some(lt) = &lt {
            // Recheck after every result: a code on one ISBN closes its whole work.
            for _ in 0..50 {
                let used = db.query_row("SELECT used FROM provider_quota WHERE provider='librarything' AND day=?1", [now_ms() / 86_400_000], |r| r.get::<_, i64>(0)).optional().map_err(error)?.unwrap_or(0);
                if used >= 1000 {
                    break;
                }
                let Some(isbn) = select_pending(&db, "librarything", now_ms(), 1)?.first().copied() else {
                    break;
                };
                match lt.lookup_isbn(isbn).await {
                    Ok(()) => {}
                    Err(e) => {
                        db.execute("INSERT OR REPLACE INTO provider_retry VALUES('librarything',?1,?2,?3)", params![isbn, now_ms() + 3_600_000, e.to_string()]).map_err(error)?;
                        tracing::warn!(%e,"LibraryThing crawl failed");
                    }
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
        if once {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_secs(30)).await;
    }
}
fn select_pending(c: &Connection, provider: &str, now: i64, limit: usize) -> Result<Vec<Isbn13>, MetadataError> {
    let mut q=c.prepare("SELECT w.isbn13 FROM temp.wanted w
WHERE NOT EXISTS(SELECT 1 FROM provider_result r WHERE r.provider=?1 AND r.isbn13=w.isbn13)
AND NOT EXISTS(SELECT 1 FROM provider_retry r WHERE r.provider=?1 AND r.isbn13=w.isbn13 AND r.retry_at_ms>?2)
AND (?1='loc' OR EXISTS(SELECT 1 FROM provider_result r WHERE r.provider='loc' AND r.isbn13=w.isbn13))
AND NOT EXISTS(SELECT 1 FROM provider_result r WHERE r.isbn13=w.isbn13 AND EXISTS(SELECT 1 FROM json_each(CAST(r.classifications AS TEXT)) code WHERE json_extract(code.value,'$.scheme')='library_of_congress'))
AND NOT EXISTS(SELECT 1 FROM temp.wanted sibling JOIN provider_result r ON r.isbn13=sibling.isbn13 WHERE w.work_id IS NOT NULL AND sibling.work_id=w.work_id AND EXISTS(SELECT 1 FROM json_each(CAST(r.classifications AS TEXT)) code WHERE json_extract(code.value,'$.scheme')='library_of_congress'))
ORDER BY w.rank,w.isbn13 LIMIT ?3").map_err(error)?;
    let rows = q.query_map(params![provider, now, limit as i64], |r| r.get(0)).map_err(error)?.collect::<Result<Vec<_>, _>>().map_err(error)?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn db() -> Connection {
        let c = open_provider(Path::new(":memory:")).unwrap();
        c.execute_batch("CREATE TEMP TABLE wanted(isbn13 INTEGER PRIMARY KEY,rank INTEGER,work_id INTEGER); CREATE INDEX temp.wanted_by_work ON wanted(work_id); INSERT INTO wanted VALUES(9780306406157,-10,10),(9780131103627,0,10),(9781691706631,-20,NULL);").unwrap();
        c
    }
    fn isbn(value: &str) -> Isbn13 {
        Isbn13::parse(value).unwrap()
    }
    fn lcc() -> Vec<Classification> {
        vec![Classification { scheme: ClassificationScheme::LibraryOfCongress, notation: "QA76".into(), source: ClassificationSource::ExactEdition, evidence: vec![] }]
    }
    #[test]
    fn lc_completion_retry_and_work_codes_control_queue() {
        let c = db();
        assert_eq!(select_pending(&c, "loc", 0, 50).unwrap(), vec![isbn("9781691706631"), isbn("9780306406157"), isbn("9780131103627")]);
        assert!(select_pending(&c, "librarything", 0, 50).unwrap().is_empty());
        save_loc(&c, isbn("9780306406157"), &LibraryOfCongressLookup::NoMatch).unwrap();
        save_loc(&c, isbn("9781691706631"), &LibraryOfCongressLookup::Ambiguous).unwrap();
        assert_eq!(select_pending(&c, "librarything", 0, 50).unwrap(), vec![isbn("9781691706631"), isbn("9780306406157")]);
        c.execute("INSERT INTO provider_retry VALUES('librarything',9781691706631,1000,'temporary failure')", []).unwrap();
        assert_eq!(select_pending(&c, "librarything", 999, 50).unwrap(), vec![isbn("9780306406157")]);
        assert_eq!(select_pending(&c, "librarything", 1000, 50).unwrap(), vec![isbn("9781691706631"), isbn("9780306406157")]);
        save_result(&c, "librarything", isbn("9780306406157"), "matched", &lcc(), &[]).unwrap();
        assert!(select_pending(&c, "loc", 1000, 50).unwrap().is_empty());
        assert_eq!(select_pending(&c, "librarything", 1000, 50).unwrap(), vec![isbn("9781691706631")]);
        save_loc(&c, isbn("9781691706631"), &LibraryOfCongressLookup::Matched { classifications: lcc(), subjects: vec![] }).unwrap();
        assert!(select_pending(&c, "librarything", 1000, 50).unwrap().is_empty());
    }
    #[test]
    fn lt_reserves_failed_requests_and_quota_survives_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("provider.sqlite");
        let c = open_provider(&path).unwrap();
        reserve_librarything(&c, 10000).unwrap();
        assert!(reserve_librarything(&c, 11999).unwrap_err().to_string().contains("spacing"));
        c.execute("UPDATE provider_quota SET used=999", []).unwrap();
        drop(c);
        let c = open_provider(&path).unwrap();
        reserve_librarything(&c, 12000).unwrap();
        assert!(reserve_librarything(&c, 14000).unwrap_err().to_string().contains("daily"));
        assert_eq!(c.query_row("SELECT used FROM provider_quota", [], |r| r.get::<_, i64>(0)).unwrap(), 1000);
        reserve_librarything(&c, 86_400_000).unwrap();
        assert_eq!(c.query_row("SELECT used FROM provider_quota WHERE day=1", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    }
    #[test]
    fn normalized_completed_results_do_not_expire() {
        let c = db();
        save_loc(&c, isbn("9780306406157"), &LibraryOfCongressLookup::NoMatch).unwrap();
        assert_eq!(load_loc(&c, isbn("9780306406157")).unwrap(), Some(LibraryOfCongressLookup::NoMatch));
        assert_eq!(load_loc(&c, isbn("9780131103627")).unwrap(), None);
        assert!(!select_pending(&c, "loc", i64::MAX, 50).unwrap().contains(&isbn("9780306406157")));
    }
}
