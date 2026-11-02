//! Portable book identity, independent of who stores its bytes.
//!
//! Existing content-addressed books retain their exact 32-byte representation.
//! Local books retain a tagged digest of their library and first byte checksum.
//! OPDS books retain a separately tagged digest of their feed and publication ID.

use crate::ContentHash;
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BookId([u8; 33]);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidBookId;

impl fmt::Display for InvalidBookId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("book identity must be a legacy content hash or a tagged local or OPDS identity")
    }
}
impl std::error::Error for InvalidBookId {}

impl BookId {
    pub const fn from_content_hash(hash: ContentHash) -> Self {
        Self::from_bytes(*hash.as_bytes())
    }

    pub const fn from_bytes(digest: [u8; 32]) -> Self {
        let mut bytes = [0; 33];
        let mut index = 0;
        while index < 32 {
            bytes[index + 1] = digest[index];
            index += 1;
        }
        Self(bytes)
    }

    pub fn new(value: &str) -> Self {
        value.parse().expect("BookId::new requires a valid book identity")
    }

    /// Digest encoded by a legacy content-derived ID. Once a book has local
    /// revisions, verification must use its separately recorded byte checksum.
    pub fn content_hash(self) -> Option<ContentHash> {
        if self.0[0] == 0 {
            Some(ContentHash::try_from(&self.0[1..]).expect("fixed digest length"))
        } else {
            None
        }
    }

    /// Assigned from the library scope and first bytes, then retained across edits.
    /// A distinct tag prevents this identity from being used as a checksum or
    /// colliding with a catalog blob addressed by that original digest.
    pub fn for_local_content(library: &[u8; 16], initial: ContentHash) -> Self {
        let mut hasher = blake3::Hasher::new_derive_key("se.bokheim.local-book-identity.v1");
        hasher.update(library);
        hasher.update(initial.as_bytes());
        let mut id = Self::from_content_hash(hasher.finalize().into());
        id.0[0] = 2;
        id
    }

    /// Identity supplied by an OPDS feed, independent of the downloaded bytes.
    pub fn for_opds_entry(entry: &[u8; 32]) -> Self {
        let mut id = Self::from_bytes(*entry);
        id.0[0] = 3;
        id
    }

    pub const fn is_opds(self) -> bool {
        self.0[0] == 3
    }

    pub const fn is_local(self) -> bool {
        self.0[0] == 2
    }

    pub fn as_bytes(&self) -> &[u8] {
        if self.0[0] != 0 {
            &self.0
        } else {
            &self.0[1..]
        }
    }

    /// Compact digest for UI keys. It is not proof of the file's contents;
    /// storage and synchronization must retain the identity's tag.
    pub fn identity_digest(&self) -> &[u8; 32] {
        self.0[1..].try_into().expect("fixed identity digest length")
    }

    /// Compatible text for existing IDs; local and OPDS IDs retain their tags
    /// and remain safe as filenames on every supported platform.
    pub fn to_hex(&self) -> String {
        self.to_string()
    }

    pub fn storage_bytes(self) -> Vec<u8> {
        self.as_bytes().to_vec()
    }
}

impl From<ContentHash> for BookId {
    fn from(hash: ContentHash) -> Self {
        Self::from_content_hash(hash)
    }
}

impl From<blake3::Hash> for BookId {
    fn from(hash: blake3::Hash) -> Self {
        Self::from_content_hash(ContentHash::from(hash))
    }
}
impl From<[u8; 32]> for BookId {
    fn from(hash: [u8; 32]) -> Self {
        Self::from_bytes(hash)
    }
}

impl TryFrom<&[u8]> for BookId {
    type Error = InvalidBookId;
    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        match value {
            [2 | 3, digest @ ..] if digest.len() == 32 => Ok(Self(value.try_into().map_err(|_| InvalidBookId)?)),
            digest if digest.len() == 32 => Ok(Self::from_content_hash(ContentHash::try_from(digest).map_err(|_| InvalidBookId)?)),
            _ => Err(InvalidBookId),
        }
    }
}

impl FromStr for BookId {
    type Err = InvalidBookId;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.len() {
            64 => Ok(Self::from_content_hash(value.parse().map_err(|_| InvalidBookId)?)),
            66 if value.starts_with("02") || value.starts_with("03") => {
                let hash: ContentHash = value[2..].parse().map_err(|_| InvalidBookId)?;
                let mut id = Self::from_content_hash(hash);
                id.0[0] = if value.starts_with("03") { 3 } else { 2 };
                Ok(id)
            }
            _ => Err(InvalidBookId),
        }
    }
}

impl fmt::Display for BookId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0[0] != 0 {
            write!(formatter, "{:02x}", self.0[0])?;
        }
        ContentHash::try_from(&self.0[1..]).expect("fixed digest length").fmt(formatter)
    }
}
impl fmt::Debug for BookId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

impl Serialize for BookId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serde_transport::text_or_bytes(self, &self.storage_bytes(), serializer)
    }
}

impl<'de> Deserialize<'de> for BookId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = BookId;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a book identity encoded as bytes or hexadecimal text")
            }
            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<BookId, E> {
                value.parse().map_err(E::custom)
            }
            fn visit_bytes<E: serde::de::Error>(self, value: &[u8]) -> Result<BookId, E> {
                BookId::try_from(value).map_err(E::custom)
            }
            fn visit_byte_buf<E: serde::de::Error>(self, value: Vec<u8>) -> Result<BookId, E> {
                self.visit_bytes(&value)
            }
            fn visit_newtype_struct<D: serde::Deserializer<'de>>(self, deserializer: D) -> Result<BookId, D::Error> {
                BookId::deserialize(deserializer)
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut sequence: A) -> Result<BookId, A::Error> {
                let mut bytes = Vec::with_capacity(33);
                while let Some(byte) = sequence.next_element::<u8>()? {
                    if bytes.len() == 33 {
                        return Err(serde::de::Error::custom(InvalidBookId));
                    }
                    bytes.push(byte);
                }
                self.visit_bytes(&bytes)
            }
        }
        if deserializer.is_human_readable() {
            deserializer.deserialize_any(Visitor)
        } else {
            deserializer.deserialize_bytes(Visitor)
        }
    }
}

#[cfg(feature = "sqlite")]
mod sqlite {
    use super::BookId;
    use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, ValueRef};

    impl ToSql for BookId {
        fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
            Ok(ToSqlOutput::Borrowed(ValueRef::Blob(self.as_bytes())))
        }
    }
    impl FromSql for BookId {
        fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
            Self::try_from(value.as_blob()?).map_err(|error| FromSqlError::Other(Box::new(error)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_addressed_ids_preserve_legacy_encodings() {
        let hash = ContentHash::from(blake3::hash(b"book bytes"));
        let id = BookId::from(hash);
        assert_eq!(id.content_hash(), Some(hash));
        assert_eq!(serde_json::to_vec(&id).unwrap(), serde_json::to_vec(&hash).unwrap());
        assert_eq!(wire::encode(&id).unwrap(), wire::encode(&hash).unwrap());
        assert_eq!(protobuf_wire::encode(&id).unwrap(), protobuf_wire::encode(&hash).unwrap());
        assert_eq!(id.storage_bytes(), hash.as_bytes());
    }

    #[test]
    fn local_identity_is_stable_and_separate_from_verification() {
        let checksum = ContentHash::from(blake3::hash(b"initial bytes"));
        let id = BookId::for_local_content(&[3; 16], checksum);
        assert_eq!(id, BookId::for_local_content(&[3; 16], checksum));
        assert_ne!(id, BookId::for_local_content(&[4; 16], checksum));
        assert!(id.is_local());
        assert_eq!(id.content_hash(), None);
        assert_ne!(id, BookId::from(checksum));
        assert_eq!(BookId::try_from(id.storage_bytes().as_slice()).unwrap(), id);
        assert_eq!(id.to_hex().parse::<BookId>().unwrap(), id);
        assert!(id.to_hex().parse::<ContentHash>().is_err());
        assert_eq!(serde_json::from_slice::<BookId>(&serde_json::to_vec(&id).unwrap()).unwrap(), id);
        assert_eq!(wire::decode::<BookId>(&wire::encode(&id).unwrap(), 1024).unwrap(), id);
        assert_eq!(protobuf_wire::decode::<BookId>(&protobuf_wire::encode(&id).unwrap(), 1024).unwrap(), id);
    }
}
