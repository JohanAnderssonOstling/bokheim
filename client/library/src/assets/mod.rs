mod local;
use include_sqlite_sql::include_sql;
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
pub(crate) use upload_intent::copy_book_baseline;
pub use work::*;

/// Snapshot physical placements and their checksums before an upload.
pub fn local_book_versions(conn: &rusqlite::Connection, identity: &sync_common::ContentHash) -> rusqlite::Result<Vec<(String, String, String)>> {
    let mut query = conn.prepare(include_str!("sql/local_book_versions.sql"))?;
    query.query_map([identity.as_str()], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?.collect()
}

mod planning;

include_sql!("src/assets/sql/assets.sql");
include_sql!("src/assets/sql/book_access.sql");
pub use planning::{asset_candidate_page, complete_asset_candidates, AssetCandidate};
