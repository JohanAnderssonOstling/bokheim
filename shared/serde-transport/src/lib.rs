//! Opt-in representations for self-describing transports.
//!
//! Domain types select a representation through these adapters instead of
//! requiring a transport to recognize their Rust names. Ordinary serializers
//! retain their usual human-readable/binary behavior. A transport can implement
//! this Serde extension by checking `uses_binary_representation` in its newtype
//! methods, serializing the wrapped value in binary mode and reading byte
//! payloads directly. The marker is an in-process Serde handshake, never wire data.

use serde::{Serialize, Serializer};
use std::fmt::Display;

pub mod compact;

const BINARY_REPRESENTATION: &str = "$bokheim::binary_representation";

/// Serializer/deserializer side of the adapter contract. No domain type names
/// or field names participate in transport dispatch.
pub fn uses_binary_representation(newtype_name: &str) -> bool {
    newtype_name == BINARY_REPRESENTATION
}

fn prefer_binary<S: Serializer, T: Serialize + ?Sized>(value: &T, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_newtype_struct(BINARY_REPRESENTATION, value)
}

/// Hex identities and similar scalars retain text in JSON and bytes in binary
/// storage, while opting into bytes for transports implementing this contract.
pub fn text_or_bytes<S: Serializer, T: Display + ?Sized>(value: &T, bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
    struct Representation<'a, T: ?Sized> {
        value: &'a T,
        bytes: &'a [u8],
    }
    impl<T: Display + ?Sized> Serialize for Representation<'_, T> {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            if serializer.is_human_readable() {
                serializer.collect_str(self.value)
            } else {
                serializer.serialize_bytes(self.bytes)
            }
        }
    }
    prefer_binary(&Representation { value, bytes }, serializer)
}
