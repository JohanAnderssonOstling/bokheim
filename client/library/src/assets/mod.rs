mod local;
mod pending;
mod presence;
mod rejections;
mod requests;

pub use local::*;
pub use pending::*;
pub use presence::*;
pub use rejections::*;
pub use requests::*;

mod upload_intent;
mod work;
pub use upload_intent::*;
pub(crate) use upload_intent::{copy_publication_baseline, requeue_local_uploads};
pub use work::*;

pub(crate) mod queries;

/// Snapshot physical placements and their checksums before an upload.
pub fn local_book_versions(conn: &rusqlite::Connection, identity: &sync_common::ContentHash) -> rusqlite::Result<Vec<(String, String, String)>> {
    let mut query = conn.prepare(include_str!("sql/local_book_versions.sql"))?;
    query.query_map([identity.as_str()], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?.collect()
}

mod planning;
pub use planning::{AssetCandidate, asset_candidate_page, complete_asset_candidates};
