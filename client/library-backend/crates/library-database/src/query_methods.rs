//! SQL methods shared by library operations and their callers.
//! SQL text and parameter declarations remain in feature files.
use rusqlite::{
    Error as SqlError, Row,
    types::{Type, ValueRef},
};
use std::io::{Error, ErrorKind};
use uuid::Uuid;

macro_rules! impl_sql {
    ($sql_name:ident = $({ $kind:tt $name:ident ($($variant:tt $param:ident $ptype:tt)*) $doc:literal $s:tt $($text:tt)+ }),+) => {
        pub trait $sql_name {
            $(include_sqlite_sql::decl_method! { $kind $name $doc () () $($param $variant $ptype)* })+
        }
        impl $sql_name for rusqlite::Connection {
            $(include_sqlite_sql::impl_method! { $kind $name () () ($($param $variant $ptype)*) => ($($variant $param)*) $($text)+ })+
        }
    };
}

/// Borrow a required BLOB while its query row remains alive.
pub(crate) fn blob_column<'a>(row: &'a Row<'_>, index: usize) -> rusqlite::Result<&'a [u8]> {
    blob_value(row, index, row.get_ref(index)?)
}

/// Borrow an optional BLOB, retaining SQL NULL as absence.
pub(crate) fn optional_blob_column<'a>(row: &'a Row<'_>, index: usize) -> rusqlite::Result<Option<&'a [u8]>> {
    match row.get_ref(index)? {
        ValueRef::Null => Ok(None),
        value => blob_value(row, index, value).map(Some),
    }
}

/// Validate a value already read from a row without copying its bytes.
fn blob_value<'a>(row: &Row<'_>, index: usize, value: ValueRef<'a>) -> rusqlite::Result<&'a [u8]> {
    match value {
        ValueRef::Blob(bytes) => Ok(bytes),
        value => Err(SqlError::InvalidColumnType(index, row.as_ref().column_name(index)?.to_owned(), value.data_type())),
    }
}

pub fn invalid_text_column(column: usize, message: impl Into<String>) -> SqlError {
    SqlError::FromSqlConversionFailure(column, Type::Text, Box::new(Error::new(ErrorKind::InvalidData, message.into())))
}

pub fn invalid_integer_column(column: usize, message: impl Into<String>) -> SqlError {
    SqlError::FromSqlConversionFailure(column, Type::Integer, Box::new(Error::new(ErrorKind::InvalidData, message.into())))
}

pub(crate) fn uuid_blob_column(row: &Row<'_>, column: usize) -> rusqlite::Result<Uuid> {
    Uuid::from_slice(blob_column(row, column)?).map_err(|error| SqlError::FromSqlConversionFailure(column, Type::Blob, Box::new(error)))
}

pub(crate) fn parse_uuid_column(value: String, column: usize) -> rusqlite::Result<Uuid> {
    Uuid::parse_str(&value).map_err(|error| invalid_text_column(column, error.to_string()))
}

pub(crate) fn first_column<T: rusqlite::types::FromSql>(query: impl FnOnce(&mut dyn FnMut(&Row<'_>) -> rusqlite::Result<()>) -> rusqlite::Result<()>) -> rusqlite::Result<Option<T>> {
    query_last(query, |row| row.get(0))
}

/// Map every row, retaining the last value and propagating all read errors.
pub(crate) fn query_last<T>(query: impl FnOnce(&mut dyn FnMut(&Row<'_>) -> rusqlite::Result<()>) -> rusqlite::Result<()>, mut map: impl FnMut(&Row<'_>) -> rusqlite::Result<T>) -> rusqlite::Result<Option<T>> {
    let mut value = None;
    query(&mut |row| {
        value = Some(map(row)?);
        Ok(())
    })?;
    Ok(value)
}

/// Collect mapped rows from an include_sql-generated query, returning any error before the caller processes them.
pub(crate) fn query_rows<T>(
    query: impl FnOnce(&mut dyn FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<()>) -> rusqlite::Result<()>, mut map: impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
) -> rusqlite::Result<Vec<T>> {
    let mut rows = Vec::new();
    query(&mut |row| {
        rows.push(map(row)?);
        Ok(())
    })?;
    Ok(rows)
}

/// Borrowed SQL values retain their caller's lifetime.
pub type SqlParam<'a> = dyn rusqlite::ToSql + 'a;

/// Inspect the active generated query during its row callback, so tests can
/// keep checking bounded scans without copying SQL or preparing it separately.
#[cfg(test)]
pub(crate) fn active_query_fullscan_steps(connection: &rusqlite::Connection) -> i32 {
    use rusqlite::ffi;
    // SAFETY: called synchronously inside a query callback. The connection and
    // all statements remain alive; these SQLite calls only inspect their state.
    unsafe {
        let handle = connection.handle();
        let mut statement = ffi::sqlite3_next_stmt(handle, std::ptr::null_mut());
        let mut steps = 0;
        while !statement.is_null() {
            if ffi::sqlite3_stmt_busy(statement) != 0 {
                steps = steps.max(ffi::sqlite3_stmt_status(statement, ffi::SQLITE_STMTSTATUS_FULLSCAN_STEP, 0));
            }
            statement = ffi::sqlite3_next_stmt(handle, statement);
        }
        steps
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{
        Connection,
        types::{FromSql, Type},
    };

    #[test]
    fn optional_column_reads_preserve_null_last_row_and_errors() {
        fn values<T: FromSql>(connection: &Connection, json: &str) -> rusqlite::Result<Option<T>> {
            first_column(|read| {
                let mut statement = connection.prepare("SELECT value FROM json_each(?1) ORDER BY key")?;
                let mut rows = statement.query([json])?;
                while let Some(row) = rows.next()? {
                    read(row)?;
                }
                Ok(())
            })
        }
        let connection = Connection::open_in_memory().unwrap();
        assert_eq!(values::<i64>(&connection, "[]").unwrap(), None);
        assert_eq!(values::<i64>(&connection, "[5,2,9]").unwrap(), Some(9));
        assert_eq!(values::<i64>(&connection, "[9,2,5]").unwrap(), Some(5));
        assert_eq!(values::<Option<i64>>(&connection, "[]").unwrap(), None);
        assert_eq!(values::<Option<i64>>(&connection, "[5,null]").unwrap(), Some(None));
        for invalid in [r#"[5,"broken"]"#, r#"["broken",5]"#] {
            assert!(matches!(values::<i64>(&connection, invalid), Err(SqlError::InvalidColumnType(0, _, Type::Text))));
        }
        assert!(matches!(first_column::<i64>(|_| Err(SqlError::InvalidQuery)), Err(SqlError::InvalidQuery)));
    }
}
