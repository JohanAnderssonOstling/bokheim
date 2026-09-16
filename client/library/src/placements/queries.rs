use include_sqlite_sql::include_sql;

include_sql!("src/placements/sql/placements.sql");

use crate::browse::queries::BrowsingSql;
use rusqlite::Connection;

pub fn directory_exists(conn: &Connection, directory_id: &str) -> rusqlite::Result<bool> {
    let mut exists = false;
    conn.live_directory_exists(directory_id, |row| {
        exists = row.get(0)?;
        Ok(())
    })?;
    Ok(exists)
}
