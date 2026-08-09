//! Stable identifiers shared by domain, storage, transport, and application layers.

use serde::{Deserialize, Serialize};
use std::fmt;

/// A non-negative Unix timestamp measured in milliseconds.
pub type UnixMillis = u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ContentHash([u8; 64]);

impl ContentHash {
    pub fn new(value: &str) -> Self {
        let mut bytes = [0_u8; 64];
        bytes.copy_from_slice(value.as_bytes());
        Self(bytes)
    }
    /// Constructs a deterministic full-width hash for fixtures and generated
    /// workloads. Production hashes are computed from the complete file.
    pub fn from_fixture_u64(value: u64) -> Self {
        Self::new(&format!("{value:064x}"))
    }
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0).expect("ContentHash is generated as ASCII hexadecimal text")
    }
}
impl fmt::Display for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_str().fmt(f)
    }
}
impl Serialize for ContentHash {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}
impl<'de> Deserialize<'de> for ContentHash {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self::new(&String::deserialize(deserializer)?))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DomainTextError;
impl fmt::Display for DomainTextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("identifier or file name must be non-empty, bounded text without control characters or path separators")
    }
}
impl std::error::Error for DomainTextError {}
fn validate_text(value: &str) -> Result<(), DomainTextError> {
    if value.is_empty() || value.len() > 255 || value.chars().any(|c| c.is_control() || c == '/' || c == '\\') {
        Err(DomainTextError)
    } else {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct TextIdentifier(String);
impl TextIdentifier {
    pub fn parse(value: &str) -> Result<Self, DomainTextError> {
        Self::try_from(value.to_owned())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl fmt::Display for TextIdentifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl TryFrom<String> for TextIdentifier {
    type Error = DomainTextError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        validate_text(&value)?;
        if value == "." || value == ".." {
            return Err(DomainTextError);
        }
        Ok(Self(value))
    }
}
impl From<TextIdentifier> for String {
    fn from(value: TextIdentifier) -> Self {
        value.0
    }
}
pub type ClientId = TextIdentifier;
pub type LibraryId = TextIdentifier;
pub type DirId = TextIdentifier;
pub type FileName = TextIdentifier;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
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
impl fmt::Display for AgentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl TryFrom<String> for AgentId {
    type Error = AgentIdError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}
impl From<AgentId> for String {
    fn from(value: AgentId) -> Self {
        value.to_string()
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
