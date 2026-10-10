//! SQL methods shared by library operations and their callers.
//! SQL text and parameter declarations remain in feature files.
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

pub(crate) fn first_column<T: rusqlite::types::FromSql>(
    query: impl FnOnce(&mut dyn FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<()>) -> rusqlite::Result<()>,
) -> rusqlite::Result<Option<T>> {
    let mut value = None;
    query(&mut |row| {
        value = Some(row.get(0)?);
        Ok(())
    })?;
    Ok(value)
}

/// Map the rows returned by an include_sql-generated query.
pub(crate) fn query_rows<T>(
    query: impl FnOnce(&mut dyn FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<()>) -> rusqlite::Result<()>,
    mut map: impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
) -> rusqlite::Result<std::vec::IntoIter<rusqlite::Result<T>>> {
    let mut rows = Vec::new();
    query(&mut |row| {
        rows.push(Ok(map(row)?));
        Ok(())
    })?;
    Ok(rows.into_iter())
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
