//! FTS5 tokenization shared by CPU preparation and database queries.
use rusqlite::{Connection, Error, ffi};
use std::{
    ffi::{c_char, c_int, c_void},
    ops::Range,
    ptr, slice,
};

fn check(code: c_int) -> rusqlite::Result<()> {
    if code == ffi::SQLITE_OK { Ok(()) } else { Err(Error::SqliteFailure(ffi::Error::new(code), None)) }
}

/// # Safety
/// Use the returned API only on this connection's owning thread and while the
/// connection remains open. Tokenizer instances must be deleted before closing.
pub unsafe fn api(connection: &Connection) -> rusqlite::Result<*mut ffi::fts5_api> {
    let mut statement = ptr::null_mut();
    let mut api: *mut ffi::fts5_api = ptr::null_mut();
    check(ffi::sqlite3_prepare_v2(connection.handle(), c"SELECT fts5(?1)".as_ptr(), -1, &mut statement, ptr::null_mut()))?;
    let result = (|| {
        check(ffi::sqlite3_bind_pointer(statement, 1, (&mut api as *mut *mut ffi::fts5_api).cast(), c"fts5_api_ptr".as_ptr(), None))?;
        let code = ffi::sqlite3_step(statement);
        if code != ffi::SQLITE_ROW {
            check(code)?;
        }
        if api.is_null() {
            return Err(Error::InvalidQuery);
        }
        Ok(api)
    })();
    ffi::sqlite3_finalize(statement);
    result
}

pub fn tokens(connection: &Connection, text: &str) -> rusqlite::Result<Vec<String>> {
    let mut tokens = Vec::new();
    tokenize(connection, text, |term, _, _| {
        tokens.push(String::from_utf8_lossy(term).into_owned());
        true
    })?;
    Ok(tokens)
}

pub struct TokenSpan {
    pub bytes: Range<usize>,
    pub character_offset: usize,
}

/// Resolve sorted token starts in one pass, stopping after the final span.
pub fn token_spans(connection: &Connection, text: &str, starts: &[usize], length: usize) -> rusqlite::Result<Vec<Option<TokenSpan>>> {
    let mut spans: Vec<Option<TokenSpan>> = (0..starts.len()).map(|_| None).collect();
    if starts.is_empty() || length == 0 {
        return Ok(spans);
    }
    let (mut token, mut first, mut last) = (0, 0, 0);
    let (mut previous_byte, mut characters) = (0, 0);
    tokenize(connection, text, |_, start, end| {
        characters += text[previous_byte..start].chars().count();
        previous_byte = start;
        while first < starts.len() && starts[first] == token {
            spans[first] = Some(TokenSpan { bytes: start..start, character_offset: characters });
            first += 1;
        }
        while last < starts.len() && starts[last] + length == token + 1 {
            if let Some(span) = &mut spans[last] {
                span.bytes.end = end;
            }
            last += 1;
        }
        token += 1;
        last < starts.len()
    })?;
    // Incomplete spans (e.g. an unreadable continuation page) are unavailable.
    for span in &mut spans[last..] {
        *span = None;
    }
    Ok(spans)
}

/// Match the search phrase with an exact prefix on its final token.
pub fn prefix_phrase_matches(connection: &Connection, text: &str, phrase: &[String]) -> rusqlite::Result<Vec<Range<usize>>> {
    let mut matches: Vec<Range<usize>> = Vec::new();
    if phrase.is_empty() {
        return Ok(matches);
    }
    let mut terms = Vec::new();
    tokenize(connection, text, |term, start, end| {
        terms.push((String::from_utf8_lossy(term).into_owned(), start..end));
        true
    })?;
    for window in terms.windows(phrase.len()) {
        if window.iter().zip(phrase).enumerate().all(|(index, ((term, _), query))| if index + 1 == phrase.len() { term.starts_with(query) } else { term == query }) {
            let range = window[0].1.start..window[phrase.len() - 1].1.end;
            if let Some(previous) = matches.last_mut().filter(|previous| range.start <= previous.end) {
                previous.end = range.end;
            } else {
                matches.push(range);
            }
        }
    }
    Ok(matches)
}

fn tokenize<F: FnMut(&[u8], usize, usize) -> bool>(connection: &Connection, text: &str, mut visit: F) -> rusqlite::Result<()> {
    let length = c_int::try_from(text.len()).map_err(|_| Error::InvalidQuery)?;
    unsafe {
        let api = api(connection)?;
        let mut tokenizer: ffi::fts5_tokenizer = std::mem::zeroed();
        let mut context = ptr::null_mut();
        check((*api).xFindTokenizer.ok_or(Error::InvalidQuery)?(api, c"unicode61".as_ptr(), &mut context, &mut tokenizer))?;
        let (Some(create), Some(delete), Some(tokenize)) = (tokenizer.xCreate, tokenizer.xDelete, tokenizer.xTokenize) else { return Err(Error::InvalidQuery) };
        let mut arguments = [c"remove_diacritics".as_ptr(), c"2".as_ptr()];
        let mut instance = ptr::null_mut();
        check(create(context, arguments.as_mut_ptr(), 2, &mut instance))?;
        let code = tokenize(instance, (&mut visit as *mut F).cast(), 4, text.as_ptr().cast(), length, Some(collect::<F>));
        delete(instance);
        check(code)
    }
}

unsafe extern "C" fn collect<F: FnMut(&[u8], usize, usize) -> bool>(context: *mut c_void, _: c_int, term: *const c_char, length: c_int, start: c_int, end: c_int) -> c_int {
    // unicode61 emits valid UTF-8 and byte offsets into the supplied text.
    // SQLITE_DONE stops unicode61 early and its xTokenize returns SQLITE_OK.
    let visit = &mut *context.cast::<F>();
    if visit(slice::from_raw_parts(term.cast::<u8>(), length as usize), start as usize, end as usize) { ffi::SQLITE_OK } else { ffi::SQLITE_DONE }
}

/// Count document tokens without allocating a string for every word.
pub fn token_counts<'a>(texts: impl IntoIterator<Item = &'a str>) -> rusqlite::Result<Vec<usize>> {
    let connection = Connection::open_in_memory()?;
    texts
        .into_iter()
        .map(|text| {
            let mut count = 0;
            tokenize(&connection, text, |_, _, _| {
                count += 1;
                true
            })?;
            Ok(count)
        })
        .collect()
}
