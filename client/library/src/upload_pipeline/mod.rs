//! Device-local admission after a fixed enrichment pass. An edit or account
//! rebind creates a new upload token and cannot inherit an earlier admission.
use rusqlite::Connection;
use std::collections::HashSet;
use sync_common::ContentHash;

#[derive(Clone, Debug)]
pub struct Batch {
    pub members: Vec<(ContentHash, Option<i64>)>,
}
impl Batch {
    pub fn hashes(&self) -> HashSet<ContentHash> {
        self.members.iter().map(|(hash, _)| *hash).collect()
    }
}

/// Capture once. New books and changed versions belong to the next pass.
pub fn snapshot(conn: &Connection) -> rusqlite::Result<Batch> {
    let members = conn.prepare_cached(include_str!("sql/snapshot.sql"))?.query_map([], |r| Ok((ContentHash::new(&r.get::<_, String>(0)?), r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
    Ok(Batch { members })
}

/// Called only after every enrichment phase has attempted this fixed set.
/// Cancellation leaves admission unchanged; retries survive in provider queues.
pub fn finish(conn: &Connection, batch: &Batch) -> rusqlite::Result<()> {
    let json = serde_json::to_string(&batch.members).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    conn.prepare_cached(include_str!("sql/finish.sql"))?.execute(rusqlite::params![json, metadata_contract::audible::POLICY_VERSION])?;
    Ok(())
}

pub fn ready(conn: &Connection) -> rusqlite::Result<HashSet<ContentHash>> {
    conn.prepare_cached(include_str!("sql/ready.sql"))?.query_map([], |r| Ok(ContentHash::new(&r.get::<_, String>(0)?)))?.collect()
}

pub fn pending(conn: &Connection) -> rusqlite::Result<bool> {
    conn.query_row(include_str!("sql/pending.sql"), [], |row| row.get(0))
}

pub fn ready_page(conn: &Connection, hashes: &[ContentHash]) -> rusqlite::Result<HashSet<ContentHash>> {
    let json = serde_json::to_string(hashes).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    conn.prepare_cached(include_str!("sql/ready_page.sql"))?.query_map([json], |r| Ok(ContentHash::new(&r.get::<_, String>(0)?)))?.collect()
}

fn scope_json(scope: &HashSet<ContentHash>) -> rusqlite::Result<String> {
    serde_json::to_string(scope).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
}

pub fn prepared(conn: &Connection, scope: &HashSet<ContentHash>) -> rusqlite::Result<Batch> {
    let members = conn.prepare_cached(include_str!("sql/prepared.sql"))?.query_map([scope_json(scope)?], |r| Ok((ContentHash::new(&r.get::<_, String>(0)?), r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
    Ok(Batch { members })
}

pub fn thumbnails(conn: &Connection, scope: &HashSet<ContentHash>, now: i64) -> rusqlite::Result<Vec<ContentHash>> {
    conn.prepare_cached(include_str!("sql/thumbnails.sql"))?.query_map(rusqlite::params![scope_json(scope)?, now], |r| Ok(ContentHash::new(&r.get::<_, String>(0)?)))?.collect()
}

/// Completed, successful libraries need no periodic enrichment pass.
pub fn retry_needed(conn: &Connection) -> rusqlite::Result<bool> {
    Ok(pending(conn)? || conn.query_row(include_str!("sql/retry.sql"), [metadata_contract::audible::POLICY_VERSION], |row| row.get::<_, bool>(0))?)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn add(conn: &Connection, digit: char) -> ContentHash {
        let hash = ContentHash::new(&digit.to_string().repeat(64));
        conn.execute(include_str!("sql/tests/book.sql"), [hash.as_str()]).unwrap();
        queue(conn, hash);
        hash
    }
    fn queue(conn: &Connection, hash: ContentHash) {
        conn.execute(include_str!("sql/tests/upload.sql"), [hash.as_str()]).unwrap();
    }
    #[test]
    fn frozen_batch_never_admits_new_members_or_replaced_versions() {
        let conn = Connection::open_in_memory().unwrap();
        crate::schema::init_db(&conn).unwrap();
        let first = add(&conn, '1');
        let changed = add(&conn, '2');
        let batch = snapshot(&conn).unwrap();
        let later = add(&conn, '3');
        queue(&conn, changed);
        assert!(ready(&conn).unwrap().is_empty());
        finish(&conn, &batch).unwrap();
        assert_eq!(ready_page(&conn, &[first, changed, later]).unwrap(), HashSet::from([first]));
        assert!(pending(&conn).unwrap());
        finish(&conn, &snapshot(&conn).unwrap()).unwrap();
        assert!(!pending(&conn).unwrap());
        assert_eq!(ready(&conn).unwrap().len(), 3);
        assert!(!retry_needed(&conn).unwrap(), "successful batches need no periodic retry");
        crate::external_metadata::record_external_metadata_attempt(&conn, &first, "test", "isbn", "failed", None).unwrap();
        assert!(retry_needed(&conn).unwrap(), "failed enrichment remains retryable after admission");
    }
    #[test]
    fn failed_file_preparation_does_not_hold_other_books_or_rewrite_readiness() {
        let conn = Connection::open_in_memory().unwrap();
        crate::schema::init_db(&conn).unwrap();
        let first = add(&conn, '6');
        let blocked = add(&conn, '7');
        conn.execute_batch(include_str!("sql/tests/clear_file_work.sql")).unwrap();
        crate::file_work::queue_book_work(&conn, &blocked).unwrap();
        let scope = snapshot(&conn).unwrap().hashes();
        let batch = prepared(&conn, &scope).unwrap();
        assert_eq!(batch.hashes(), HashSet::from([first]));
        finish(&conn, &batch).unwrap();
        let writes = conn.total_changes();
        finish(&conn, &batch).unwrap();
        assert_eq!(conn.total_changes(), writes, "unchanged admissions must not rewrite every book");
        assert_eq!(ready(&conn).unwrap(), HashSet::from([first]));
    }

    #[test]
    fn interrupted_audible_claim_waits_but_provider_failure_releases_the_book() {
        use crate::audiobooks::queries::AudiobooksSql;
        let mut conn = Connection::open_in_memory().unwrap();
        crate::schema::init_db(&conn).unwrap();
        let audio = add(&conn, 'a');
        let other = add(&conn, 'b');
        conn.upsert_audiobook_metadata(audio.as_str(), 1000, 0, "embedded_m4b", "[]").unwrap();
        let request = metadata_contract::audible::LookupRequest { title: "Book".into(), authors: vec![], duration_ms: 1000, recording: Default::default(), chapters: vec![] };
        crate::audible_jobs::discover(&conn, &audio, metadata_contract::audible::POLICY_VERSION, &request, 0).unwrap();
        let claim = crate::audible_jobs::claim_live(&mut conn, 0).unwrap().unwrap();
        let batch = snapshot(&conn).unwrap();
        finish(&conn, &batch).unwrap();
        assert_eq!(ready(&conn).unwrap(), HashSet::from([other]), "an interrupted request is not a completed attempt");
        let response = metadata_contract::audible::LookupResponse {
            status: metadata_contract::audible::LookupStatus::Unavailable,
            candidates: vec![],
            selected_asin: None,
            selected_region: None,
            chapter_plan: None,
            used_website_fallback: false,
            detail: "offline".into(),
        };
        crate::audible_jobs::resolved(&mut conn, &claim, &response, 300, 1).unwrap();
        finish(&conn, &batch).unwrap();
        assert_eq!(ready(&conn).unwrap(), HashSet::from([audio, other]));
        assert!(crate::audible_jobs::claim_live(&mut conn, 2).unwrap().is_none(), "retry backoff remains intact after upload admission");
    }

    #[test]
    fn readiness_survives_reopen_but_unfinished_batches_do_not_admit() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("library.db");
        let conn = Connection::open(&path).unwrap();
        crate::schema::init_db(&conn).unwrap();
        let done = add(&conn, '4');
        finish(&conn, &snapshot(&conn).unwrap()).unwrap();
        let waiting = add(&conn, '5');
        let _interrupted = snapshot(&conn).unwrap();
        drop(conn);
        let conn = Connection::open(&path).unwrap();
        crate::schema::init_db(&conn).unwrap();
        assert_eq!(ready(&conn).unwrap(), HashSet::from([done]));
        assert!(!ready(&conn).unwrap().contains(&waiting));
        queue(&conn, done);
        assert!(ready(&conn).unwrap().is_empty(), "reenabling/account rebinding rotates the upload token");
    }
}
