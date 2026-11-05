//! Lossless SQLite description storage shared by catalogs and libraries.
//! Text is decoded only for the requested row; no decoded descriptions are cached.
use rusqlite::{
    Connection, Error, OptionalExtension, Result,
    functions::FunctionFlags,
    types::{Type, Value, ValueRef},
};
use std::io::{self, Read};
use zstd::{bulk::Compressor, stream::read::Decoder};

// Existing databases retain this version in each frame. Never replace its
// dictionary: future dictionaries must use another version and keep this reader.
const VERSION: u8 = 1;
const DICTIONARY_V1: &[u8] = include_bytes!("dictionary-v1.zdict");

fn invalid(kind: Type, message: impl Into<String>) -> Error {
    Error::FromSqlConversionFailure(0, kind, Box::new(io::Error::new(io::ErrorKind::InvalidData, message.into())))
}

/// Keep short text verbatim and store a versioned frame only when it is smaller.
/// Encoded values are already storage records, so copying them is idempotent.
pub fn encode(value: ValueRef<'_>) -> Result<Value> {
    let bytes = match value {
        ValueRef::Text(bytes) => bytes,
        ValueRef::Blob(bytes) => return Ok(Value::Blob(bytes.to_vec())),
        _ => return Err(invalid(value.data_type(), "description must be text or an encoded frame")),
    };
    let text = std::str::from_utf8(bytes).map_err(|error| Error::FromSqlConversionFailure(0, Type::Text, Box::new(error)))?;
    if !bytes.is_empty() {
        let mut compressor = Compressor::with_dictionary(9, DICTIONARY_V1).map_err(|error| Error::UserFunctionError(Box::new(error)))?;
        let compressed = compressor.compress(bytes).map_err(|error| Error::UserFunctionError(Box::new(error)))?;
        if compressed.len() + 1 < bytes.len() {
            let mut frame = Vec::with_capacity(compressed.len() + 1);
            frame.push(VERSION);
            frame.extend_from_slice(&compressed);
            return Ok(Value::Blob(frame));
        }
    }
    Ok(Value::Text(text.to_owned()))
}

pub fn decode(value: ValueRef<'_>) -> Result<String> {
    match value {
        ValueRef::Text(bytes) => String::from_utf8(bytes.to_vec()).map_err(|error| Error::FromSqlConversionFailure(0, Type::Text, Box::new(error))),
        ValueRef::Blob(frame) => {
            let Some((&VERSION, data)) = frame.split_first() else {
                return Err(invalid(Type::Blob, "unsupported description storage version"));
            };
            let mut decoder = Decoder::with_dictionary(data, DICTIONARY_V1).map_err(|error| Error::FromSqlConversionFailure(0, Type::Blob, Box::new(error)))?;
            let mut text = String::new();
            decoder.read_to_string(&mut text).map_err(|error| Error::FromSqlConversionFailure(0, Type::Blob, Box::new(error)))?;
            Ok(text)
        }
        _ => Err(invalid(value.data_type(), "description must be text or an encoded frame")),
    }
}

pub fn register(connection: &Connection) -> Result<()> {
    connection.create_scalar_function("pack_description", 1, FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC | FunctionFlags::SQLITE_INNOCUOUS, |context| encode(context.get_raw(0)))
}

#[derive(Clone, Copy)]
pub enum Source {
    Catalog,
    Library,
}

/// Both detail paths perform the same single-row lookup and decode operation.
pub fn read(connection: &Connection, source: Source, row_id: i64) -> Result<Option<String>> {
    let sql = match source {
        Source::Catalog => "SELECT text FROM description WHERE row_id=?1",
        Source::Library => "SELECT description FROM book_description WHERE book_row_id=?1",
    };
    connection.prepare_cached(sql)?.query_row([row_id], |row| decode(row.get_ref(0)?)).optional()
}
