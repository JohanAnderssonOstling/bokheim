//! All occurrences of the one phrase issued by book text search.
use device_sqlite::fts5::api;
pub use device_sqlite::fts5::tokens;
use rusqlite::{Connection, Error, ffi};
use std::{ffi::c_int, ptr};

pub fn register(connection: &Connection) -> rusqlite::Result<()> {
    unsafe {
        let api = api(connection)?;
        let code = (*api).xCreateFunction.ok_or(Error::InvalidQuery)?(api, c"book_matches".as_ptr(), ptr::null_mut(), Some(matches), None);
        if code == ffi::SQLITE_OK { Ok(()) } else { Err(Error::SqliteFailure(ffi::Error::new(code), None)) }
    }
}

unsafe extern "C" fn matches(api: *const ffi::Fts5ExtensionApi, context: *mut ffi::Fts5Context, output: *mut ffi::sqlite3_context, _: c_int, _: *mut *mut ffi::sqlite3_value) {
    let mut iterator: ffi::Fts5PhraseIter = std::mem::zeroed();
    let (mut column, mut offset) = (-1, -1);
    let (Some(first), Some(next)) = ((*api).xPhraseFirst, (*api).xPhraseNext) else {
        ffi::sqlite3_result_error_code(output, ffi::SQLITE_ERROR);
        return;
    };
    let code = first(context, 0, &mut iterator, &mut column, &mut offset);
    if code != ffi::SQLITE_OK {
        ffi::sqlite3_result_error_code(output, code);
        return;
    }
    // Compact, owned token positions; excerpts remain in the original books.
    let mut offsets = Vec::new();
    while column >= 0 {
        offsets.extend_from_slice(&(offset as u32).to_le_bytes());
        next(context, &mut iterator, &mut column, &mut offset);
    }
    ffi::sqlite3_result_blob64(output, offsets.as_ptr().cast(), offsets.len() as u64, ffi::SQLITE_TRANSIENT());
}
