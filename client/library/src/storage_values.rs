use rusqlite::types::Type;
use uuid::Uuid;

pub fn invalid_text_column(column: usize, message: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(column, Type::Text, Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, message.into())))
}

pub(crate) fn invalid_integer_column(column: usize, message: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(column, Type::Integer, Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, message.into())))
}

pub(crate) fn nonnegative_u64(value: i64, column: usize, name: &str) -> rusqlite::Result<u64> {
    u64::try_from(value).map_err(|_| invalid_integer_column(column, format!("{name} must not be negative")))
}

pub(crate) fn bounded_u8(value: i64, column: usize, name: &str) -> rusqlite::Result<u8> {
    u8::try_from(value).map_err(|_| invalid_integer_column(column, format!("{name} is outside its storage range")))
}

pub(crate) fn storage_i64(value: u64, name: &str) -> rusqlite::Result<i64> {
    i64::try_from(value).map_err(|_| rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{name} exceeds the database integer range")))))
}

pub(crate) fn parse_uuid_column(value: String, column: usize) -> rusqlite::Result<Uuid> {
    Uuid::parse_str(&value).map_err(|error| invalid_text_column(column, error.to_string()))
}

pub(crate) fn unix_millis() -> Result<i64, rusqlite::Error> {
    let millis = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?.as_millis();
    i64::try_from(millis).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
}

pub(crate) fn unix_secs() -> Result<i64, rusqlite::Error> {
    let seconds = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?.as_secs();
    i64::try_from(seconds).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
}
