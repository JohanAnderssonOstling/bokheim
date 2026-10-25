//! Register keys retain book digests as bytes while other identifiers stay text.
use crate::ContentHash;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StateKey {
    Book(ContentHash),
    Text(String),
}
impl StateKey {
    pub fn book(&self) -> Option<ContentHash> {
        match self {
            Self::Book(hash) => Some(*hash),
            Self::Text(_) => None,
        }
    }
    pub fn text(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(text),
            Self::Book(_) => None,
        }
    }
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Book(hash) => hash.as_bytes(),
            Self::Text(text) => text.as_bytes(),
        }
    }
    pub fn from_storage(kind: &str, bytes: &[u8]) -> Result<Self, String> {
        if book_kind(kind) { ContentHash::try_from(bytes).map(Self::Book).map_err(|error| error.to_string()) } else { String::from_utf8(bytes.to_vec()).map(Self::Text).map_err(|error| error.to_string()) }
    }
}
pub(crate) fn book_kind(kind: &str) -> bool {
    // The relay can still carry opaque entries from older clients. Retaining
    // their key encoding does not make the current replica author those kinds.
    matches!(kind, "book_add" | "enrichment" | "book_lifecycle" | "placement" | "reading_position" | "chapter_read_status" | "book_facts" | "metadata" | "description" | "pdf_reader_metadata" | "book_toc")
}
impl From<ContentHash> for StateKey {
    fn from(value: ContentHash) -> Self {
        Self::Book(value)
    }
}
impl From<String> for StateKey {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}
impl From<&str> for StateKey {
    fn from(value: &str) -> Self {
        Self::Text(value.into())
    }
}
impl fmt::Display for StateKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Book(hash) => hash.fmt(formatter),
            Self::Text(text) => text.fmt(formatter),
        }
    }
}
#[derive(Serialize)]
enum BinaryKeyRef<'a> {
    Book(&'a ContentHash),
    Text(&'a str),
}
#[derive(Deserialize)]
enum BinaryKey {
    Book(ContentHash),
    Text(String),
}
impl Serialize for StateKey {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            match self {
                Self::Book(hash) => hash.serialize(serializer),
                Self::Text(text) => text.serialize(serializer),
            }
        } else {
            match self {
                Self::Book(hash) => BinaryKeyRef::Book(hash).serialize(serializer),
                Self::Text(text) => BinaryKeyRef::Text(text).serialize(serializer),
            }
        }
    }
}
impl<'de> Deserialize<'de> for StateKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if !deserializer.is_human_readable() {
            return BinaryKey::deserialize(deserializer).map(|key| match key {
                BinaryKey::Book(hash) => Self::Book(hash),
                BinaryKey::Text(text) => Self::Text(text),
            });
        }
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = StateKey;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a text identifier or raw book hash")
            }
            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
                Ok(StateKey::Text(value.into()))
            }
            fn visit_bytes<E: serde::de::Error>(self, value: &[u8]) -> Result<Self::Value, E> {
                ContentHash::try_from(value).map(StateKey::Book).map_err(E::custom)
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut sequence: A) -> Result<Self::Value, A::Error> {
                let mut bytes = [0; 32];
                for byte in &mut bytes {
                    *byte = sequence.next_element()?.ok_or_else(|| serde::de::Error::custom("book key needs 32 bytes"))?;
                }
                if sequence.next_element::<u8>()?.is_some() {
                    return Err(serde::de::Error::custom("book key needs exactly 32 bytes"));
                }
                Ok(StateKey::Book(ContentHash::from_bytes(bytes)))
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}
#[cfg(feature = "sqlite")]
mod sqlite {
    use super::StateKey;
    use crate::ContentHash;
    use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, ValueRef};
    impl ToSql for StateKey {
        fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
            Ok(ToSqlOutput::Borrowed(match self {
                StateKey::Book(hash) => ValueRef::Blob(hash.as_bytes()),
                StateKey::Text(text) => ValueRef::Text(text.as_bytes()),
            }))
        }
    }
    impl FromSql for StateKey {
        fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
            match value {
                ValueRef::Blob(bytes) => ContentHash::try_from(bytes).map(StateKey::Book).map_err(|error| FromSqlError::Other(Box::new(error))),
                ValueRef::Text(bytes) => Ok(StateKey::Text(std::str::from_utf8(bytes).map_err(|error| FromSqlError::Other(Box::new(error)))?.into())),
                _ => Err(FromSqlError::InvalidType),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StateAddress;

    #[test]
    fn book_addresses_keep_json_text_but_use_binary_digest_keys() {
        let hash = ContentHash::from_bytes([0x42; 32]);
        let address = StateAddress { kind: "book_lifecycle".into(), entity_key: hash.into(), entity_subkey: String::new() };
        let json = serde_json::to_string(&address).unwrap();
        assert_eq!(serde_json::from_str::<StateAddress>(&json).unwrap(), address);
        assert!(json.contains(&format!("\"entity_key\":\"{hash}\"")));
        let wire = crate::wire::encode(&address).unwrap();
        assert_eq!(crate::wire::decode::<StateAddress>(&wire, 1024).unwrap(), address);
        let transport = crate::transport::encode(&address).unwrap();
        assert_eq!(crate::transport::decode::<StateAddress>(&transport, 1024).unwrap(), address);
        assert!(transport.windows(32).any(|window| window == hash.as_bytes()));
        assert!(!transport.windows(64).any(|window| window == hash.to_hex().as_bytes()));
    }

    #[test]
    fn immutable_and_legacy_book_entries_keep_raw_hash_keys() {
        let hash = ContentHash::from_bytes([0xff; 32]);
        for kind in ["book_add", "enrichment", "book_facts", "metadata", "description", "pdf_reader_metadata", "book_toc"] {
            let address = StateAddress { kind: kind.into(), entity_key: hash.into(), entity_subkey: String::new() };
            assert_eq!(StateKey::from_storage(kind, hash.as_bytes()).unwrap(), StateKey::Book(hash));
            let json = serde_json::to_string(&address).unwrap();
            assert_eq!(serde_json::from_str::<StateAddress>(&json).unwrap(), address);
            let transport = crate::transport::encode(&address).unwrap();
            assert_eq!(crate::transport::decode::<StateAddress>(&transport, 1024).unwrap(), address);
        }
    }

    #[test]
    fn annotation_ids_that_look_like_hashes_stay_text() {
        let address = StateAddress { kind: "annotation".into(), entity_key: "a".repeat(64).into(), entity_subkey: String::new() };
        let json = serde_json::to_string(&address).unwrap();
        assert_eq!(serde_json::from_str::<StateAddress>(&json).unwrap(), address);
        let transport = crate::transport::encode(&address).unwrap();
        assert_eq!(crate::transport::decode::<StateAddress>(&transport, 1024).unwrap(), address);
        assert_eq!(StateKey::from_storage("annotation", address.entity_key.as_bytes()).unwrap(), address.entity_key);
    }

    #[cfg(feature = "sqlite")]
    #[test]
    fn sqlite_register_keys_preserve_digest_and_text_types() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE registers(kind TEXT,key BLOB,PRIMARY KEY(kind,key))").unwrap();
        for (kind, key) in [("book_lifecycle", StateKey::Book(ContentHash::from_bytes([0xff; 32]))), ("annotation", StateKey::Text("note".into()))] {
            conn.execute("INSERT INTO registers VALUES(?1,?2)", rusqlite::params![kind, key]).unwrap();
            let stored: StateKey = conn.query_row("SELECT key FROM registers WHERE kind=?1 AND key=?2", rusqlite::params![kind, key], |row| row.get(0)).unwrap();
            assert_eq!(stored, key);
        }
    }
}
