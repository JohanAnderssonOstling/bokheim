use std::fmt;

#[derive(Debug)]
pub struct CatalogStoreError(String);

impl CatalogStoreError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for CatalogStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for CatalogStoreError {}

impl From<rusqlite::Error> for CatalogStoreError {
    fn from(error: rusqlite::Error) -> Self {
        Self::new(error.to_string())
    }
}

impl From<wire::WireError> for CatalogStoreError {
    fn from(error: wire::WireError) -> Self {
        Self::new(error.to_string())
    }
}

impl From<serde_json::Error> for CatalogStoreError {
    fn from(error: serde_json::Error) -> Self {
        Self::new(error.to_string())
    }
}

pub fn decode_cached<T: serde::de::DeserializeOwned>(payload: &[u8]) -> Result<T, wire::WireError> {
    wire::decode(payload, wire::MAX_DECODED_RESPONSE_BYTES)
}

pub fn sql_decode_error(error: impl std::error::Error + Send + Sync + 'static) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Blob, Box::new(error))
}

pub fn storage_i64(value: u64, name: &str) -> Result<i64, CatalogStoreError> {
    i64::try_from(value).map_err(|_| CatalogStoreError::new(format!("{name} exceeds SQLite integer range")))
}

pub fn stored_u64(value: i64, name: &str) -> rusqlite::Result<u64> {
    u64::try_from(value).map_err(|_| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Integer, Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{name} is negative")))))
}

pub fn optional_stored_u64(value: Option<i64>, name: &str) -> rusqlite::Result<Option<u64>> {
    value.map(|value| stored_u64(value, name)).transpose()
}
