//! Stable identifiers shared by domain, storage, transport, and application layers.


use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// A non-negative Unix timestamp measured in milliseconds, bounded to the
/// signed database storage range.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct UnixMillis(u64);

impl UnixMillis {
    pub const MAX: u64 = i64::MAX as u64;
    pub const fn new(value: u64) -> Result<Self, TimestampError> {
        if value <= Self::MAX { Ok(Self(value)) } else { Err(TimestampError) }
    }
    pub const fn get(self) -> u64 { self.0 }
    pub const fn storage_i64(self) -> i64 { self.0 as i64 }
    pub const fn saturating_add(self, milliseconds: u64) -> Self {
        let value = self.0.saturating_add(milliseconds);
        if value > Self::MAX { Self(Self::MAX) } else { Self(value) }
    }
}

impl fmt::Display for UnixMillis {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result { self.0.fmt(formatter) }
}

impl<'de> Deserialize<'de> for UnixMillis {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(u64::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimestampError;

impl fmt::Display for TimestampError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result { formatter.write_str("Unix-millisecond timestamps must fit signed 64-bit storage") }
}

impl std::error::Error for TimestampError {}

/// File formats supported by the library's book workflows.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum BookFormat { Epub, Pdf, Mobi, Fb2, Cbz, Html, WebPub, M4b }

impl BookFormat {
    pub fn from_extension(extension: &str) -> Option<Self> {
        match extension.to_ascii_lowercase().as_str() {
            "epub" => Some(Self::Epub), "pdf" => Some(Self::Pdf),
            "mobi" | "azw" | "azw3" => Some(Self::Mobi), "fb2" => Some(Self::Fb2),
            "cbz" => Some(Self::Cbz), "html" | "htm" | "xhtml" => Some(Self::Html),
            "webpub" => Some(Self::WebPub), "m4b" => Some(Self::M4b), _ => None,
        }
    }
    pub const fn canonical_extension(self) -> &'static str {
        match self { Self::Epub => "epub", Self::Pdf => "pdf", Self::Mobi => "mobi", Self::Fb2 => "fb2", Self::Cbz => "cbz", Self::Html => "html", Self::WebPub => "webpub", Self::M4b => "m4b" }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BookId(u64);

impl BookId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
    pub fn canonical(self) -> String {
        self.0.to_string()
    }
    pub fn parse(value: &str) -> Result<Self, std::num::ParseIntError> {
        value.parse::<u64>().map(Self)
    }
    pub fn from_content_hash(content_hash: &ContentHash) -> Self {
        let bytes = content_hash.as_str().as_bytes();
        let mut id = [0_u8; 8];
        for (index, byte) in id.iter_mut().enumerate() {
            *byte = (decode_hex_nibble(bytes[index * 2]) << 4) | decode_hex_nibble(bytes[index * 2 + 1]);
        }
        Self(u64::from_le_bytes(id))
    }
}
impl fmt::Display for BookId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl FromStr for BookId {
    type Err = std::num::ParseIntError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}
impl Serialize for BookId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            serializer.serialize_str(&self.canonical())
        } else {
            serializer.serialize_u64(self.0)
        }
    }
}
impl<'de> Deserialize<'de> for BookId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if !deserializer.is_human_readable() {
            return u64::deserialize(deserializer).map(Self::new);
        }
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Compatible {
            String(String),
            Number(u64),
        }
        match Compatible::deserialize(deserializer)? {
            Compatible::String(value) => Self::parse(&value).map_err(serde::de::Error::custom),
            Compatible::Number(value) => Ok(Self::new(value)),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ContentHash(String);
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentHashParseError;
impl fmt::Display for ContentHashParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("content hash must be exactly 64 lowercase hexadecimal characters")
    }
}
impl std::error::Error for ContentHashParseError {}
impl ContentHash {
    pub fn parse(value: &str) -> Result<Self, ContentHashParseError> {
        if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)) {
            Ok(Self(value.to_owned()))
        } else {
            Err(ContentHashParseError)
        }
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn into_string(self) -> String {
        self.0
    }
}
impl fmt::Display for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl FromStr for ContentHash {
    type Err = ContentHashParseError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}
impl Serialize for ContentHash {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            return serializer.serialize_str(self.as_str());
        }
        let encoded = self.as_str().as_bytes();
        let mut bytes = [0_u8; 32];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = (decode_hex_nibble(encoded[index * 2]) << 4) | decode_hex_nibble(encoded[index * 2 + 1]);
        }
        bytes.serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for ContentHash {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if !deserializer.is_human_readable() {
            let bytes = <[u8; 32]>::deserialize(deserializer)?;
            const HEX: &[u8; 16] = b"0123456789abcdef";
            let mut encoded = String::with_capacity(64);
            for byte in bytes {
                encoded.push(HEX[(byte >> 4) as usize] as char);
                encoded.push(HEX[(byte & 15) as usize] as char);
            }
            return Ok(Self(encoded));
        }
        Self::parse(&String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DomainTextError {
    kind: &'static str,
}
impl fmt::Display for DomainTextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} must be non-empty, bounded text without control characters or path separators", self.kind)
    }
}
impl std::error::Error for DomainTextError {}
fn validate(value: &str, kind: &'static str) -> Result<(), DomainTextError> {
    if value.is_empty() || value.len() > 255 || value.chars().any(|c| c.is_control() || c == '/' || c == '\\') {
        Err(DomainTextError { kind })
    } else {
        Ok(())
    }
}
macro_rules! text_id {
    ($name:ident, $kind:literal) => {
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);
        impl $name {
            pub fn parse(value: &str) -> Result<Self, DomainTextError> {
                validate(value, $kind)?;
                Ok(Self(value.to_owned()))
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
            pub fn into_string(self) -> String {
                self.0
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
        impl FromStr for $name {
            type Err = DomainTextError;
            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::parse(value)
            }
        }
        impl Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.as_str())
            }
        }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                Self::parse(&String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
            }
        }
    };
}
text_id!(ClientId, "replica id");
text_id!(LibraryId, "library id");
text_id!(DirId, "directory id");

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileName(String);
impl FileName {
    pub fn parse(value: &str) -> Result<Self, DomainTextError> {
        validate(value, "file name")?;
        if value == "." || value == ".." {
            return Err(DomainTextError { kind: "file name" });
        }
        Ok(Self(value.to_owned()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn into_string(self) -> String {
        self.0
    }
}
impl fmt::Display for FileName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl FromStr for FileName {
    type Err = DomainTextError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}
impl Serialize for FileName {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}
impl<'de> Deserialize<'de> for FileName {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::parse(&String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

fn decode_hex_nibble(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        _ => unreachable!("validated hexadecimal identity"),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AgentId(uuid::Uuid);
impl AgentId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }
    pub fn parse(value: &str) -> Result<Self, AgentIdError> {
        let value = uuid::Uuid::parse_str(value).map_err(|_| AgentIdError)?;
        if value.is_nil() {
            Err(AgentIdError)
        } else {
            Ok(Self(value))
        }
    }
    pub fn from_migration_seed(seed: &[u8]) -> Self {
        const NAMESPACE: uuid::Uuid = uuid::Uuid::from_bytes([0x7f, 0xe7, 0x7e, 0x34, 0x29, 0x7f, 0x4a, 0xfa, 0x99, 0x8d, 0x2f, 0x86, 0xf0, 0x58, 0x7b, 0x31]);
        Self(uuid::Uuid::new_v5(&NAMESPACE, seed))
    }
}
impl Default for AgentId {
    fn default() -> Self {
        Self::new()
    }
}
impl fmt::Display for AgentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl Serialize for AgentId {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if s.is_human_readable() {
            s.serialize_str(&self.to_string())
        } else {
            self.0.into_bytes().serialize(s)
        }
    }
}
impl<'de> Deserialize<'de> for AgentId {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        if !d.is_human_readable() {
            let value = uuid::Uuid::from_bytes(<[u8; 16]>::deserialize(d)?);
            return if value.is_nil() { Err(serde::de::Error::custom(AgentIdError)) } else { Ok(Self(value)) };
        }
        Self::parse(&String::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AgentIdError;
impl fmt::Display for AgentIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("agent id must be a non-nil UUID")
    }
}
impl std::error::Error for AgentIdError {}
pub type AuthorId = AgentId;
pub type PublisherId = AgentId;
pub type ContributorId = AgentId;
pub type AuthorIdError = AgentIdError;
