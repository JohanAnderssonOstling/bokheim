use serde::de::DeserializeOwned;
use serde::Serialize;
use std::fmt;

pub const MEDIA_TYPE: &str = "application/vnd.bokheim+bincode; version=11";
pub const MEDIA_TYPE_BASE: &str = "application/vnd.bokheim+bincode";
pub const MAX_DECODED_REQUEST_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_DECODED_RESPONSE_BYTES: usize = 64 * 1024 * 1024;

const MAGIC: &[u8; 4] = b"BKHM";
const VERSION: u8 = 11;
const HEADER_BYTES: usize = MAGIC.len() + 1;

/// The magic+version header bytes prefixing every encoded value, exposed so
/// tests can assert against it without hardcoding a version byte that goes
/// stale on the next [`VERSION`] bump.
pub const fn header_bytes() -> [u8; HEADER_BYTES] {
    [MAGIC[0], MAGIC[1], MAGIC[2], MAGIC[3], VERSION]
}

pub fn is_current_media_type(value: &str) -> bool {
    let mut fields = value.split(';');
    if !fields.next().is_some_and(|media_type| media_type.trim().eq_ignore_ascii_case(MEDIA_TYPE_BASE)) {
        return false;
    }
    let mut version = None;
    for parameter in fields {
        let Some((name, value)) = parameter.trim().split_once('=') else { continue };
        if name.trim().eq_ignore_ascii_case("version") {
            if version.is_some() {
                return false;
            }
            version = Some(value.trim());
        }
    }
    version.and_then(|value| value.parse::<u8>().ok()) == Some(VERSION)
}

#[derive(Debug, PartialEq, Eq)]
pub enum WireError {
    Encode(String),
    Decode(String),
    InvalidHeader,
    UnsupportedVersion(u8),
    TooLarge { limit: usize },
}

impl fmt::Display for WireError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Encode(reason) => write!(formatter, "cannot encode Bokheim body: {reason}"),
            Self::Decode(reason) => write!(formatter, "cannot decode Bokheim body: {reason}"),
            Self::InvalidHeader => formatter.write_str("Bokheim body has an invalid wire header"),
            Self::UnsupportedVersion(version) => write!(formatter, "Bokheim wire version {version} is unsupported"),
            Self::TooLarge { limit } => write!(formatter, "decoded Bokheim body exceeds the {limit}-byte limit"),
        }
    }
}

impl std::error::Error for WireError {}

pub fn encode<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>, WireError> {
    let payload = bincode::serde::encode_to_vec(value, bincode::config::standard()).map_err(|error| WireError::Encode(error.to_string()))?;
    let mut body = Vec::with_capacity(HEADER_BYTES + payload.len());
    body.extend_from_slice(MAGIC);
    body.push(VERSION);
    body.extend_from_slice(&payload);
    Ok(body)
}

pub fn decode<T: DeserializeOwned>(body: &[u8], decoded_limit: usize) -> Result<T, WireError> {
    if body.len() > decoded_limit {
        return Err(WireError::TooLarge { limit: decoded_limit });
    }
    if body.len() < HEADER_BYTES || &body[..MAGIC.len()] != MAGIC {
        return Err(WireError::InvalidHeader);
    }
    if body[MAGIC.len()] != VERSION {
        return Err(WireError::UnsupportedVersion(body[MAGIC.len()]));
    }
    let (value, consumed) = bincode::serde::decode_from_slice(&body[HEADER_BYTES..], bincode::config::standard()).map_err(|error| WireError::Decode(error.to_string()))?;
    if consumed != body.len() - HEADER_BYTES {
        return Err(WireError::Decode("trailing bytes after Bokheim value".to_owned()));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Deserialize, PartialEq, Serialize)]
    struct Example {
        name: String,
        values: Vec<u64>,
    }

    #[derive(Debug, Deserialize, PartialEq, Serialize)]
    enum NumericVariant {
        First,
        Second(u64),
    }

    fn example(count: usize) -> Example {
        Example { name: "Bokheim".to_owned(), values: (0..count as u64).collect() }
    }

    #[test]
    fn bincode_round_trip_uses_the_versioned_bokheim_header() {
        let encoded = encode(&example(8)).unwrap();
        let golden = b"BKHM\x0b\x07Bokheim\x08\x00\x01\x02\x03\x04\x05\x06\x07";
        assert_eq!(encoded, golden);
        assert_eq!(&encoded[..MAGIC.len()], MAGIC);
        assert_eq!(encoded[MAGIC.len()], VERSION);
        assert_eq!(decode::<Example>(&encoded, MAX_DECODED_REQUEST_BYTES).unwrap(), example(8));

        assert_eq!(encode(&NumericVariant::Second(42)).unwrap(), b"BKHM\x0b\x01\x2a");
        assert_eq!(decode::<NumericVariant>(b"BKHM\x0b\x01\x2a", 64).unwrap(), NumericVariant::Second(42));
    }

    #[test]
    fn version_and_decoded_size_are_bounded() {
        let mut unsupported = encode(&example(1)).unwrap();
        unsupported[MAGIC.len()] = VERSION + 1;
        assert!(matches!(decode::<Example>(&unsupported, MAX_DECODED_REQUEST_BYTES), Err(WireError::UnsupportedVersion(version)) if version == VERSION + 1));
        assert_eq!(decode::<Example>(&encode(&example(1_000)).unwrap(), 32), Err(WireError::TooLarge { limit: 32 }));
    }

    #[test]
    fn decoder_rejects_trailing_and_malformed_data() {
        let mut trailing = encode(&NumericVariant::First).unwrap();
        trailing.push(0);
        assert!(matches!(decode::<NumericVariant>(&trailing, 64), Err(WireError::Decode(reason)) if reason.contains("trailing bytes")));
    }

    #[test]
    fn media_type_requires_the_current_version() {
        assert!(is_current_media_type(MEDIA_TYPE));
        assert!(is_current_media_type("Application/Vnd.Bokheim+Bincode; charset=binary; version=11"));
        assert!(!is_current_media_type(MEDIA_TYPE_BASE));
        assert!(!is_current_media_type("application/vnd.bokheim+bincode; version=1"));
        assert!(!is_current_media_type("application/vnd.bokheim+bincode; version=2"));
        assert!(!is_current_media_type("application/vnd.bokheim+bincode; version=6"));
        assert!(!is_current_media_type("application/vnd.bokheim+bincode; version=1; version=5"));
        assert!(!is_current_media_type("application/json; version=1"));
    }
}
