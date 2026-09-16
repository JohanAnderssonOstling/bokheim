//! Durable local Audible work. Lookup results are proposals until their metadata
//! is committed to the library database.
#[cfg(test)]
use crate::audiobooks::queries::AudiobooksSql;
#[cfg(test)]
use crate::books::queries::BooksSql;
use crate::external_metadata;
use crate::schema::library_write_transaction;
use crate::storage_values::invalid_text_column;
use metadata_contract::audible::{LookupRequest, LookupResponse, LookupStatus};
use rusqlite::Connection;
use rusqlite::OptionalExtension;
use rusqlite::types::Type;
use sync_common::ContentHash;

fn json<T: serde::Serialize + ?Sized>(value: &T) -> rusqlite::Result<String> {
    serde_json::to_string(value).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
}
fn decode<T: serde::de::DeserializeOwned>(value: &str) -> rusqlite::Result<T> {
    serde_json::from_str(value).map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(e)))
}

/// Call after capturing current file evidence. Identical evidence leaves a
/// completed result and its retry policy intact; changed evidence invalidates
/// even a live claim so its late response cannot overwrite the new job.
pub fn discover(conn: &Connection, hash: &ContentHash, policy: &str, request: &LookupRequest, now: i64) -> rusqlite::Result<bool> {
    Ok(conn.execute(include_str!("sql/audible_jobs/discover_insert.sql"), rusqlite::params![hash.as_str(), policy, json(request)?, now])? > 0)
}

#[derive(Clone, Debug)]
pub struct Claim {
    pub content_hash: ContentHash,
    pub request: LookupRequest,
    token: String,
}

/// Claim just one bounded job. Expired leases retry the lookup.
/// No transaction or connection needs to survive the caller's network work.
pub fn claim(conn: &mut Connection, now: i64) -> rusqlite::Result<Option<Claim>> {
    claim_inner(conn, now, false, None)
}

pub fn claim_live(conn: &mut Connection, now: i64) -> rusqlite::Result<Option<Claim>> {
    claim_inner(conn, now, true, None)
}

pub fn batch_candidates(conn: &Connection, now: i64, hashes: &[ContentHash]) -> rusqlite::Result<Vec<ContentHash>> {
    conn.prepare_cached(include_str!("sql/audible_jobs/batch_candidates.sql"))?.query_map(rusqlite::params![json(hashes)?, now, metadata_contract::audible::POLICY_VERSION], |r| Ok(ContentHash::new(&r.get::<_, String>(0)?)))?.collect()
}

pub fn claim_batch(conn: &mut Connection, now: i64, hashes: &[ContentHash]) -> rusqlite::Result<Option<Claim>> {
    let hashes = json(hashes)?;
    claim_inner(conn, now, true, Some(&hashes))
}

fn claim_inner(conn: &mut Connection, now: i64, live: bool, hashes: Option<&str>) -> rusqlite::Result<Option<Claim>> {
    let tx = conn.transaction()?;
    let row =
        tx.query_row(include_str!("sql/audible_jobs/claim_inner_select.sql"), rusqlite::params![now, live, metadata_contract::audible::POLICY_VERSION, hashes], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))).optional()?;
    let Some((hash, request)) = row else { return Ok(None) };
    let token = uuid::Uuid::new_v4().to_string();
    let request = decode(&request)?;
    tx.execute(include_str!("sql/audible_jobs/claim_inner_update.sql"), rusqlite::params![hash, token, now.saturating_add(120), now])?;
    tx.commit()?;
    Ok(Some(Claim { content_hash: ContentHash::new(&hash), request, token }))
}

/// Commit the lookup result and any supported metadata together. A failed
/// transaction leaves the claim unfinished so the lookup can retry after expiry.
pub fn resolved(conn: &mut Connection, claim: &Claim, response: &LookupResponse, retry_at: i64, now: i64) -> rusqlite::Result<bool> {
    let tx = library_write_transaction(conn)?;
    let status = match response.status {
        LookupStatus::NoMatch => "no_match",
        LookupStatus::Unavailable => "unavailable",
        LookupStatus::Ambiguous => "no_match",
        LookupStatus::Supported => "applied",
    };
    let current = tx.execute(
        include_str!("sql/audible_jobs/resolved_update.sql"),
        rusqlite::params![claim.content_hash.as_str(), claim.token, status, json(response)?, if response.status == LookupStatus::Unavailable { retry_at.max(now.saturating_add(1)) } else { 0 }, now],
    )? > 0;
    if !current {
        return Ok(false);
    }
    if response.status == LookupStatus::Supported {
        let candidate = response
            .candidates
            .iter()
            .find(|candidate| Some(&candidate.asin) == response.selected_asin.as_ref() && Some(&candidate.region) == response.selected_region.as_ref())
            .ok_or_else(|| invalid_text_column(0, "Audible response has no selected recording"))?;
        if let Some(plan) = &response.chapter_plan {
            tx.execute(include_str!("sql/audible_jobs/resolved_update_2.sql"), rusqlite::params![claim.content_hash.as_str(), plan.chapters.len() as i64, json(&plan.chapters)?])?;
        }
        external_metadata::apply_audible_identifiers(&tx, claim.content_hash, &candidate.asin, &candidate.isbns)?;
    }
    tx.commit()?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setup() -> (Connection, ContentHash, LookupRequest) {
        let conn = Connection::open_in_memory().unwrap();
        crate::schema::init_db(&conn).unwrap();
        let hash = ContentHash::new(&"1".repeat(64));
        conn.upsert_audiobook_metadata(hash.as_str(), 1000, 0, "embedded_m4b", "[]").unwrap();
        let request = LookupRequest { title: "Book".into(), authors: vec![], duration_ms: 1000, recording: Default::default(), chapters: vec![] };
        (conn, hash, request)
    }
    fn response(status: LookupStatus) -> LookupResponse {
        LookupResponse { status, candidates: vec![], selected_asin: None, selected_region: None, chapter_plan: None, used_website_fallback: false, detail: "fixture".into() }
    }
    #[test]
    fn stale_evidence_and_expired_claims_cannot_commit() {
        let (mut conn, hash, mut request) = setup();
        assert!(discover(&conn, &hash, "v1", &request, 0).unwrap());
        let old = claim(&mut conn, 1).unwrap().unwrap();
        assert!(claim(&mut conn, 2).unwrap().is_none());
        request.title = "Corrected title".into();
        assert!(discover(&conn, &hash, "v1", &request, 3).unwrap());
        assert!(!resolved(&mut conn, &old, &response(LookupStatus::NoMatch), 0, 4).unwrap());
        let current = claim(&mut conn, 5).unwrap().unwrap();
        assert!(!resolved(&mut conn, &current, &response(LookupStatus::NoMatch), 0, 125).unwrap());
        let resumed = claim(&mut conn, 125).unwrap().unwrap();
        assert_eq!(resumed.request.title, "Corrected title");
        assert!(!resolved(&mut conn, &current, &response(LookupStatus::NoMatch), 0, 126).unwrap());
        assert!(resolved(&mut conn, &resumed, &response(LookupStatus::NoMatch), 0, 126).unwrap());
        assert!(!discover(&conn, &hash, "v1", &request, 127).unwrap());
        assert!(claim(&mut conn, 1000).unwrap().is_none());
        assert!(discover(&conn, &hash, "v2", &request, 1001).unwrap());
        assert!(claim(&mut conn, 1002).unwrap().is_some());
    }
    #[test]
    fn transient_failure_respects_backoff_and_deletion_cascades() {
        let (mut conn, hash, request) = setup();
        discover(&conn, &hash, "v1", &request, 0).unwrap();
        let job = claim(&mut conn, 1).unwrap().unwrap();
        assert!(resolved(&mut conn, &job, &response(LookupStatus::Unavailable), 50, 2).unwrap());
        assert!(claim(&mut conn, 49).unwrap().is_none());
        assert!(claim(&mut conn, 50).unwrap().is_some());
        conn.execute(include_str!("sql/audible_jobs/transient_failure_respects_backoff_and_deletion_cascades_delete.sql"), [hash.as_str()]).unwrap();
        assert!(claim(&mut conn, 1000).unwrap().is_none());
    }
    #[test]
    fn failed_metadata_update_rolls_back_completion_and_can_retry_after_restart() {
        let (_, hash, request) = setup();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        let mut conn = Connection::open(&path).unwrap();
        crate::schema::init_db(&conn).unwrap();
        conn.upsert_audiobook_metadata(hash.as_str(), 1000, 0, "embedded_m4b", "[]").unwrap();
        discover(&conn, &hash, "v1", &request, 0).unwrap();
        let interrupted = claim(&mut conn, 1).unwrap().unwrap();
        // Invalid supported responses cannot leave a completed job behind.
        assert!(resolved(&mut conn, &interrupted, &response(LookupStatus::Supported), 0, 2).is_err());
        assert_eq!(conn.query_row(include_str!("sql/audible_jobs/failed_metadata_update_rolls_back_completion_and_can_retry_after_restart_select.sql"), [], |row| row.get::<_, String>(0)).unwrap(), "resolving");
        assert!(conn.query_row(include_str!("sql/audible_jobs/failed_metadata_update_rolls_back_completion_and_can_retry_after_restart_select_2.sql"), [], |row| row.get::<_, Option<String>>(0)).unwrap().is_none());
        drop(conn);
        let mut conn = Connection::open(&path).unwrap();
        crate::schema::init_db(&conn).unwrap();
        assert!(claim(&mut conn, 120).unwrap().is_none());
        let resumed = claim(&mut conn, 121).unwrap().unwrap();
        assert_eq!(resumed.request.title, request.title);
        assert!(!resolved(&mut conn, &interrupted, &response(LookupStatus::NoMatch), 0, 122).unwrap());
        assert!(resolved(&mut conn, &resumed, &response(LookupStatus::NoMatch), 0, 122).unwrap());
        assert!(!discover(&conn, &hash, "v1", &request, 123).unwrap());
    }

    #[test]
    fn live_claims_exclude_hidden_and_old_policy_jobs() {
        let (mut conn, hash, request) = setup();
        conn.insert_book(hash.as_str(), "Book", 1, "m4b").unwrap();
        discover(&conn, &hash, metadata_contract::audible::POLICY_VERSION, &request, 0).unwrap();
        conn.execute(include_str!("sql/audible_jobs/live_claims_exclude_hidden_and_old_policy_jobs_update.sql"), [hash.as_str()]).unwrap();
        assert!(claim_live(&mut conn, 1).unwrap().is_none());
        assert!(next_due(&conn).unwrap().is_none());
        conn.execute(include_str!("sql/audible_jobs/live_claims_exclude_hidden_and_old_policy_jobs_update_2.sql"), [hash.as_str()]).unwrap();
        assert!(claim_live(&mut conn, 1).unwrap().is_some());
        assert!(claim_live(&mut conn, 120).unwrap().is_none());
        discover(&conn, &hash, "old-policy", &request, 123).unwrap();
        assert!(claim_live(&mut conn, 124).unwrap().is_none());
        assert!(next_due(&conn).unwrap().is_none());
    }
}

/// Deadline for the next live job, including a claim abandoned by cancellation.
pub fn next_due(conn: &Connection) -> rusqlite::Result<Option<i64>> {
    conn.query_row(include_str!("sql/audible_jobs/next_due_select.sql"), [metadata_contract::audible::POLICY_VERSION], |row| row.get(0))
}
