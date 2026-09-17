//! Bounded snapshots for transfer planning, independent of physical file access.
use library_replica::RelativeBookPath;
use rusqlite::Connection;
use sync_common::ContentHash;

#[derive(Debug)]
pub struct AssetCandidate {
    pub work_id: i64,
    pub content_hash: ContentHash,
    pub live: bool,
    pub path: Option<RelativeBookPath>,
    pub book_upload_allowed: bool,
    pub thumbnail_upload_allowed: bool,
    pub thumbnail_remote: bool,
    pub thumbnail_pending: bool,
    pub requested: bool,
    pub rejected: bool,
    pub file_work_pending: bool,
}

/// One statement snapshots at most 32 work IDs and their relevant library state.
/// Missing/deleted books remain in the page so the caller can retire stale work.
pub fn asset_candidate_page(conn: &Connection, after: i64, through: i64) -> rusqlite::Result<Vec<AssetCandidate>> {
    conn.prepare_cached(include_str!("sql/planning/candidate_page.sql"))?
        .query_map(rusqlite::params![after, through], |row| {
            let live: bool = row.get(2)?;
            let path: Option<String> = row.get(3)?;
            let path = if live { path.map(|path| RelativeBookPath::parse(&path).map_err(|error| crate::storage_values::invalid_text_column(3, error.to_string()))).transpose()? } else { None };
            Ok(AssetCandidate {
                work_id: row.get(0)?,
                content_hash: ContentHash::new(&row.get::<_, String>(1)?),
                live,
                path,
                book_upload_allowed: row.get(4)?,
                thumbnail_upload_allowed: row.get(5)?,
                thumbnail_remote: row.get(6)?,
                thumbnail_pending: row.get(7)?,
                requested: row.get(8)?,
                rejected: row.get(9)?,
                file_work_pending: row.get(10)?,
            })
        })?
        .collect()
}

/// Retire exact snapshot IDs, preserving newer enqueues and newly pending file work.
/// Each pair is (work ID, recheck file work); dead/unreachable books skip that check.
/// The caller validates its account under the same write transaction.
pub fn complete_asset_candidates(conn: &Connection, candidates: &[(i64, bool)]) -> rusqlite::Result<()> {
    if candidates.is_empty() {
        return Ok(());
    }
    let ids = serde_json::to_string(candidates).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    conn.prepare_cached(include_str!("sql/planning/complete_candidates.sql"))?.execute([ids])?;
    Ok(())
}

#[cfg(test)]
mod tests;
