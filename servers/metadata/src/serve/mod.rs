//! Read-only point lookups, per-book merging and Audible. No provider clients or keys.

#[cfg(test)]
use crate::build::tests::snapshot_from;
use crate::schema::{self, now_ms};
use crate::*;
use std::sync::Mutex;

pub(crate) mod alternatives;
pub(crate) mod audible;
pub(crate) mod books;
pub(crate) mod covers;
pub(crate) mod http;
pub(crate) mod identity;
pub(crate) mod related_isbn;
pub(crate) mod resolution;

#[derive(Clone)]
pub struct MetadataService {
    pub(crate) state: Arc<ServiceState>,
}
pub(crate) struct ServiceState {
    pub(crate) database: PathBuf,
    pub(crate) pool: Pool<SqliteConnectionManager>,
    pub(crate) demand: Mutex<Connection>,
    pub(crate) snapshot: SnapshotDescription,
    pub(crate) schema_version: i64,
    pub(crate) descriptions_available: bool,
    pub(crate) started_at: Instant,
    pub(crate) books: EndpointMetrics,
    pub(crate) processed_items: AtomicU64,
    pub(crate) item_processing_duration_ms: AtomicU64,
}
impl MetadataService {
    pub fn open(answers: &Path, demand: &Path) -> Result<Self, MetadataError> {
        let answers = answers.canonicalize().map_err(error)?;
        if demand.exists() && demand.canonicalize().map_err(error)? == answers {
            return Err(MetadataError("demand database must be separate from the answer file".into()));
        }
        let pool = open_read_pool(&answers)?;
        let c = pool.get().map_err(error)?;
        let (version, date, built): (i64, String, i64) = c.query_row("SELECT schema_version,dump_date,built_at_ms FROM snapshot WHERE singleton=1", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map_err(error)?;
        if version != schema::ANSWER_VERSION {
            return Err(MetadataError(format!("unsupported answer schema {version}; expected {}", schema::ANSWER_VERSION)));
        }
        // Require a complete schema at startup instead of probing during requests.
        for sql in [
            "SELECT isbn13,answer FROM isbn_answer LIMIT 0",
            "SELECT work_id,text FROM work_description LIMIT 0",
            "SELECT key,edition_id,edition_match,work_match FROM edition_title_key LIMIT 0",
            "SELECT key,author_keys,source,control_number,codes FROM authority_title LIMIT 0",
            "SELECT edition_id,isbn13,work_id,title,subtitle,work_title,authors,publishers,book_year,classifications FROM edition_candidate LIMIT 0",
        ] {
            c.prepare(sql).map_err(error)?;
        }
        let descriptions_available = true;
        drop(c);
        let d = Connection::open(demand).map_err(error)?;
        d.busy_timeout(Duration::from_secs(5)).map_err(error)?;
        d.execute_batch(schema::DEMAND).map_err(error)?;
        d.execute_batch("CREATE TABLE IF NOT EXISTS resolved_cover(id TEXT PRIMARY KEY NOT NULL, bytes BLOB NOT NULL)").map_err(error)?;
        Ok(Self {
            state: Arc::new(ServiceState {
                database: answers,
                pool,
                demand: Mutex::new(d),
                snapshot: SnapshotDescription { dump_date: date, imported_at_ms: u64::try_from(built).map_err(error)? },
                schema_version: version,
                descriptions_available,
                started_at: Instant::now(),
                books: EndpointMetrics::default(),
                processed_items: AtomicU64::new(0),
                item_processing_duration_ms: AtomicU64::new(0),
            }),
        })
    }
    pub(crate) fn enrich_rich(&self, isbns: Vec<String>) -> Result<Vec<RichIsbnMetadataResult>, MetadataError> {
        crate::serve::http::validate_isbn_count(isbns.len())?;
        let c = self.state.pool.get().map_err(error)?;
        let mut q = c.prepare_cached("SELECT answer FROM isbn_answer WHERE isbn13=?1").map_err(error)?;
        let mut descriptions = c.prepare_cached("SELECT text FROM work_description WHERE work_id=?1").map_err(error)?;
        let mut results = Vec::with_capacity(isbns.len());
        for requested in isbns {
            let isbn = Isbn13::parse(&requested);
            let raw = isbn.map(|id| q.query_row([id], |r| r.get::<_, Vec<u8>>(0)).optional().map_err(error)).transpose()?.flatten();
            let mut answer = if let Some(raw) = raw {
                let stored: schema::StoredAnswer = serde_json::from_slice(&raw).map_err(error)?;
                let mut answer = stored.result;
                if answer.matches.len() != stored.description_works.len() {
                    return Err(MetadataError("answer has invalid description references".into()));
                }
                for (m, work) in answer.matches.iter_mut().zip(stored.description_works) {
                    if let Some(work) = work {
                        m.description = Some(descriptions.query_row([work], |r| r.get(0)).map_err(error)?);
                    }
                }
                answer
            } else {
                RichIsbnMetadataResult { requested_isbn: requested.clone(), canonical_isbn13: isbn.map(|isbn| isbn.to_string()), status: if isbn.is_some() { LookupStatus::NoMatch } else { LookupStatus::InvalidIsbn }, matches: vec![] }
            };
            answer.requested_isbn = requested;
            if let Some(isbn) =
                isbn.filter(|_| !answer.matches.iter().flat_map(|m| &m.classifications).any(|c| c.scheme == ClassificationScheme::LibraryOfCongress && metadata_contract::classification_consensus_notation(c.scheme, &c.notation).is_some()))
            {
                let now = now_ms();
                self.state.demand.lock().map_err(error)?.execute("INSERT INTO demand VALUES(?1,?2,?2,1) ON CONFLICT(isbn13) DO UPDATE SET last_seen_ms=excluded.last_seen_ms,requests=requests+1", params![isbn, now]).map_err(error)?;
            }
            results.push(answer);
        }
        Ok(results)
    }
    pub(crate) fn resolve_editions(&self, queries: Vec<EditionIdentityQuery>) -> Result<Vec<EditionIdentityResult>, MetadataError> {
        if queries.is_empty() || queries.len() > MAX_EDITION_QUERIES_PER_REQUEST {
            return Err(MetadataError(format!("between 1 and {MAX_EDITION_QUERIES_PER_REQUEST} edition queries are required")));
        }
        identity::resolve(&*self.state.pool.get().map_err(error)?, queries)
    }
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;
    #[test]
    fn descriptions_are_shared_but_exact_answers_and_requested_isbns_survive() {
        let dir = tempfile::tempdir().unwrap();
        let source = snapshot_from(
            dir.path(),
            &[serde_json::json!({"key":"/books/OL1M","works":[{"key":"/works/OL10W"}],"isbn_13":["9780306406157"]}), serde_json::json!({"key":"/books/OL2M","works":[{"key":"/works/OL10W"}],"isbn_13":["9780131103627"]})],
            &[serde_json::json!({"key":"/works/OL10W","description":"One shared description."})],
            &[],
        );
        let answers = dir.path().join("answers.sqlite");
        build_answers(&source, None, None, None, &answers).unwrap();
        let c = Connection::open(&answers).unwrap();
        assert_eq!(c.query_row("SELECT count(*) FROM work_description", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
        let raw: Vec<u8> = c.query_row("SELECT answer FROM isbn_answer WHERE isbn13=9780306406157", [], |r| r.get(0)).unwrap();
        let stored: schema::StoredAnswer = serde_json::from_slice(&raw).unwrap();
        assert!(stored.result.matches[0].description.is_none());
        assert_eq!(stored.description_works, vec![Some(10)]);
        let server = MetadataService::open(&answers, &dir.path().join("demand.sqlite")).unwrap();
        std::fs::remove_file(source).unwrap();
        let results = server.enrich_rich(vec!["0-306-40615-2".into(), "9780131103627".into()]).unwrap();
        assert_eq!(results[0].requested_isbn, "0-306-40615-2");
        assert_eq!(results[0].canonical_isbn13.as_deref(), Some("9780306406157"));
        assert!(results.iter().all(|r| r.matches[0].description.as_deref() == Some("One shared description.")));
        assert!(server.state.pool.get().unwrap().execute("DELETE FROM isbn_answer", []).is_err());
    }
}
