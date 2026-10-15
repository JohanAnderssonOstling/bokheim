//! Exact-length backend for the shared Serde traversal. Payload bytes are
//! counted without being copied; scalar sizes come from the protobuf schema.
use crate::protobuf::Value;
use crate::serialization::{Backend, Scalar, SequenceOutput, Serializer};
use prost::Message;
use serde::Serialize;
use serde_value::{SerializerError, Value as Key};
use std::collections::BTreeMap;

pub(crate) fn encoded_len<T: Serialize + ?Sized>(value: &T) -> Result<usize, SerializerError> {
    let value = value.serialize(Serializer::<Counting>::new(false))?;
    add(1 + varint(crate::VERSION.into()), message(value.len)?)
}
fn add(left: usize, right: usize) -> Result<usize, SerializerError> {
    left.checked_add(right).ok_or_else(|| <SerializerError as serde::ser::Error>::custom("protobuf length overflow"))
}
fn varint(value: u64) -> usize {
    (64usize.saturating_sub(value.leading_zeros() as usize).max(1) + 6) / 7
}
// Every wrapper field tag in the current Value, MapEntry and Envelope is one byte.
fn message(length: usize) -> Result<usize, SerializerError> {
    add(add(1, varint(length as u64))?, length)
}
fn measured(len: usize) -> Size {
    Size { len, byte: false }
}
fn entry_size(key: usize, value: usize) -> Result<usize, SerializerError> {
    message(add(message(key)?, message(value)?)?)
}
pub(crate) struct Counting;
pub(crate) struct Size {
    len: usize,
    byte: bool,
}
pub(crate) struct Sequence {
    count: usize,
    length: usize,
    all_bytes: bool,
}
impl Backend for Counting {
    type Output = Size;
    type Sequence = Sequence;
    type FieldKey = &'static str;
    #[inline]
    fn scalar(value: Scalar) -> Size {
        Size { len: Value { kind: Some(value.kind()) }.encoded_len(), byte: value.byte().is_some() }
    }
    fn text(value: &str) -> Result<Size, SerializerError> {
        Ok(measured(message(value.len())?))
    }
    fn bytes(value: &[u8]) -> Result<Size, SerializerError> {
        Ok(measured(message(value.len())?))
    }
    fn unit() -> Result<Size, SerializerError> {
        Ok(measured(message(0)?))
    }
    fn optional(value: Option<Size>) -> Result<Size, SerializerError> {
        Ok(measured(message(match value {
            Some(value) => message(value.len)?,
            None => 0,
        })?))
    }
    fn newtype(value: Size) -> Result<Size, SerializerError> {
        Ok(measured(message(value.len)?))
    }
    fn sequence(_: usize) -> Sequence {
        Sequence { count: 0, length: 0, all_bytes: true }
    }
    fn map(values: BTreeMap<Key, Size>) -> Result<Size, SerializerError> {
        let mut length = 0;
        for (key, value) in values {
            let key = key.serialize(Serializer::<Counting>::new(false))?;
            length = add(length, entry_size(key.len, value.len)?)?;
        }
        Ok(measured(message(length)?))
    }
    fn field_key(name: &'static str) -> &'static str {
        name
    }
    fn fields(values: BTreeMap<&'static str, Size>) -> Result<Size, SerializerError> {
        let mut length = 0;
        for (name, value) in values {
            length = add(length, entry_size(message(name.len())?, value.len)?)?;
        }
        Ok(measured(message(length)?))
    }
    fn variant(name: &str, value: Size) -> Result<Size, SerializerError> {
        Ok(measured(message(entry_size(message(name.len())?, value.len)?)?))
    }
}
impl SequenceOutput<Size> for Sequence {
    fn push(&mut self, value: Size) -> Result<(), SerializerError> {
        self.count = add(self.count, 1)?;
        self.length = add(self.length, message(value.len)?)?;
        self.all_bytes &= value.byte;
        Ok(())
    }
    fn finish(self) -> Result<Size, SerializerError> {
        Ok(measured(message(if self.all_bytes { self.count } else { self.length })?))
    }
}
