use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MutationId(pub(crate) Uuid);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MutationIdError;
impl fmt::Display for MutationIdError { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str("mutation id must be a non-nil UUID") } }
impl std::error::Error for MutationIdError {}
impl MutationId {
    pub fn new() -> Self { Self(Uuid::new_v4()) }
    pub fn parse(value: &str) -> Result<Self, MutationIdError> { let value = Uuid::parse_str(value).map_err(|_| MutationIdError)?; if value.is_nil() { Err(MutationIdError) } else { Ok(Self(value)) } }
}
impl Default for MutationId { fn default() -> Self { Self::new() } }
impl fmt::Display for MutationId { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { self.0.fmt(f) } }
impl Serialize for MutationId { fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> { if s.is_human_readable() { s.serialize_str(&self.to_string()) } else { self.0.into_bytes().serialize(s) } } }
impl<'de> Deserialize<'de> for MutationId { fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> { if !d.is_human_readable() { let value = Uuid::from_bytes(<[u8; 16]>::deserialize(d)?); return if value.is_nil() { Err(serde::de::Error::custom(MutationIdError)) } else { Ok(Self(value)) }; } Self::parse(&String::deserialize(d)?).map_err(serde::de::Error::custom) } }
