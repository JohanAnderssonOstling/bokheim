use serde::de::DeserializeOwned;
use serde::Serialize;
use std::fmt;
use std::io::Read;

pub const MEDIA_TYPE: &str = "application/vnd.bokheim+bincode; version=7";
pub const MEDIA_TYPE_BASE: &str = "application/vnd.bokheim+bincode";
pub const CONTENT_ENCODING_ZSTD: &str = "zstd";
pub const COMPRESSION_THRESHOLD_BYTES: usize = 1024;
pub const MAX_DECODED_REQUEST_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_DECODED_RESPONSE_BYTES: usize = 64 * 1024 * 1024;

const MAGIC: &[u8; 4] = b"BKHM";
const VERSION: u8 = 7;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentEncoding {
    Identity,
    Zstd,
}

#[derive(Debug)]
pub struct EncodedBody {
    pub bytes: Vec<u8>,
    pub encoding: ContentEncoding,
}

#[derive(Debug, PartialEq, Eq)]
pub enum WireError {
    Encode(String),
    Decode(String),
    InvalidHeader,
    UnsupportedVersion(u8),
    TooLarge { limit: usize },
    Compression(String),
}

impl fmt::Display for WireError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Encode(reason) => write!(formatter, "cannot encode Bokheim body: {reason}"),
            Self::Decode(reason) => write!(formatter, "cannot decode Bokheim body: {reason}"),
            Self::InvalidHeader => formatter.write_str("Bokheim body has an invalid wire header"),
            Self::UnsupportedVersion(version) => write!(formatter, "Bokheim wire version {version} is unsupported"),
            Self::TooLarge { limit } => write!(formatter, "decoded Bokheim body exceeds the {limit}-byte limit"),
            Self::Compression(reason) => write!(formatter, "cannot process Bokheim compression: {reason}"),
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

pub fn zstd(body: &[u8]) -> Result<Vec<u8>, WireError> {
    zstd::bulk::compress(body, 1).map_err(|error| WireError::Compression(error.to_string()))
}

pub fn unzstd(body: &[u8], decoded_limit: usize) -> Result<Vec<u8>, WireError> {
    let decoder = zstd::stream::read::Decoder::new(body).map_err(|error| WireError::Compression(error.to_string()))?;
    let mut decoded = Vec::new();
    decoder.take(decoded_limit.saturating_add(1) as u64).read_to_end(&mut decoded).map_err(|error| WireError::Compression(error.to_string()))?;
    if decoded.len() > decoded_limit {
        return Err(WireError::TooLarge { limit: decoded_limit });
    }
    Ok(decoded)
}

pub fn encode_transport<T: Serialize + ?Sized>(value: &T, accept_zstd: bool) -> Result<EncodedBody, WireError> {
    let identity = encode(value)?;
    if !accept_zstd || identity.len() < COMPRESSION_THRESHOLD_BYTES {
        return Ok(EncodedBody { bytes: identity, encoding: ContentEncoding::Identity });
    }
    let compressed = zstd(&identity)?;
    if compressed.len() >= identity.len() {
        return Ok(EncodedBody { bytes: identity, encoding: ContentEncoding::Identity });
    }
    Ok(EncodedBody { bytes: compressed, encoding: ContentEncoding::Zstd })
}

pub fn decode_transport<T: DeserializeOwned>(body: &[u8], encoding: ContentEncoding, decoded_limit: usize) -> Result<T, WireError> {
    match encoding {
        ContentEncoding::Identity => decode(body, decoded_limit),
        ContentEncoding::Zstd => {
            let decoded = unzstd(body, decoded_limit)?;
            decode(&decoded, decoded_limit)
        }
    }
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

    #[derive(Debug, Deserialize, PartialEq, Serialize)]
    struct BytePayload {
        bytes: Vec<u8>,
    }

    fn example(count: usize) -> Example {
        Example { name: "Bokheim".to_owned(), values: (0..count as u64).collect() }
    }

    #[test]
    fn bincode_round_trip_uses_the_versioned_bokheim_header() {
        let encoded = encode(&example(8)).unwrap();
        let golden = b"BKHM\x07\x07Bokheim\x08\x00\x01\x02\x03\x04\x05\x06\x07";
        assert_eq!(encoded, golden);
        assert_eq!(&encoded[..MAGIC.len()], MAGIC);
        assert_eq!(encoded[MAGIC.len()], VERSION);
        assert_eq!(decode::<Example>(&encoded, MAX_DECODED_REQUEST_BYTES).unwrap(), example(8));

        assert_eq!(encode(&NumericVariant::Second(42)).unwrap(), b"BKHM\x07\x01\x2a");
        assert_eq!(decode::<NumericVariant>(b"BKHM\x07\x01\x2a", 64).unwrap(), NumericVariant::Second(42));
    }

    #[test]
    fn compression_version_and_decoded_size_are_bounded() {
        let encoded = encode_transport(&example(1_000), true).unwrap();
        assert_eq!(encoded.encoding, ContentEncoding::Zstd);
        assert_eq!(decode_transport::<Example>(&encoded.bytes, encoded.encoding, MAX_DECODED_REQUEST_BYTES).unwrap(), example(1_000));

        let mut unsupported = encode(&example(1)).unwrap();
        unsupported[MAGIC.len()] = VERSION + 1;
        assert!(matches!(decode::<Example>(&unsupported, MAX_DECODED_REQUEST_BYTES), Err(WireError::UnsupportedVersion(version)) if version == VERSION + 1));

        let compressed = zstd(&vec![0_u8; 4096]).unwrap();
        assert_eq!(unzstd(&compressed, 1024), Err(WireError::TooLarge { limit: 1024 }));
    }

    #[test]
    fn transport_compresses_only_eligible_payloads_when_it_saves_bytes() {
        let small = encode_transport(&example(2), true).unwrap();
        assert_eq!(small.encoding, ContentEncoding::Identity);

        let refused = encode_transport(&example(1_000), false).unwrap();
        assert_eq!(refused.encoding, ContentEncoding::Identity);

        let mut state = 0x9e37_79b9_7f4a_7c15_u64;
        let random_like = (0..4096)
            .map(|_| {
                state ^= state >> 12;
                state ^= state << 25;
                state ^= state >> 27;
                (state.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 56) as u8
            })
            .collect();
        let incompressible = encode_transport(&BytePayload { bytes: random_like }, true).unwrap();
        assert_eq!(incompressible.encoding, ContentEncoding::Identity);
    }

    #[test]
    fn decoder_rejects_trailing_and_malformed_data() {
        let mut trailing = encode(&NumericVariant::First).unwrap();
        trailing.push(0);
        assert!(matches!(decode::<NumericVariant>(&trailing, 64), Err(WireError::Decode(reason)) if reason.contains("trailing bytes")));
        assert!(matches!(decode_transport::<NumericVariant>(b"not zstd", ContentEncoding::Zstd, 64), Err(WireError::Compression(_))));
    }

    #[test]
    fn media_type_requires_the_current_version() {
        assert!(is_current_media_type(MEDIA_TYPE));
        assert!(is_current_media_type("Application/Vnd.Bokheim+Bincode; charset=binary; version=7"));
        assert!(!is_current_media_type(MEDIA_TYPE_BASE));
        assert!(!is_current_media_type("application/vnd.bokheim+bincode; version=1"));
        assert!(!is_current_media_type("application/vnd.bokheim+bincode; version=2"));
        assert!(!is_current_media_type("application/vnd.bokheim+bincode; version=6"));
        assert!(!is_current_media_type("application/vnd.bokheim+bincode; version=1; version=5"));
        assert!(!is_current_media_type("application/json; version=1"));
    }
}
