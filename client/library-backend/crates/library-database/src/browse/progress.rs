//! Normalize legacy progress on writes without changing synchronized registers.
use rusqlite::{Connection, functions::FunctionFlags};

pub(crate) fn register_progress_function(connection: &Connection) -> rusqlite::Result<()> {
    connection.create_scalar_function("browse_reading_progress", 2, FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC, |context| {
        let progress = context.get::<Option<f64>>(0)?.unwrap_or_default();
        // SQL supplies CAST(read_pos AS REAL) to preserve SQLite's legacy
        // numeric-prefix rules, including positions from older libraries.
        let legacy = context.get::<Option<f64>>(1)?.unwrap_or_default();
        Ok(if progress > 0.0 {
            progress
        } else if legacy > 0.0 && legacy <= 1.0 {
            legacy * 100.0
        } else if legacy > 1.0 && legacy <= 100.0 {
            legacy
        } else {
            0.0
        })
    })
}
