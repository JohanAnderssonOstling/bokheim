//! Typed HTTP contracts. Stored sync payloads use the separate versioned codec.
//!
//! Validate field sizes, collection counts and nesting over borrowed wire bytes
//! before prost constructs any owned messages. No generic value tree is built.
pub use crate::{MAX_DECODED_REQUEST_BYTES, MAX_DECODED_RESPONSE_BYTES, MEDIA_TYPE, MEDIA_TYPE_BASE, WireError, is_current_media_type, version};
use prost::{
    Message,
    encoding::{WireType, encode_key, encode_varint, encoded_len_varint, key_len},
};
use std::fmt::{Debug, Display};

pub mod encoding;
mod scan;
pub use scan::{Field, Kind, Schema, validate_message};

pub trait Contract: Sized {
    type Proto: Message + Default + Schema + Debug;
    fn body_len(&self) -> usize;
    fn encode_body(&self, output: &mut Vec<u8>);
    fn from_proto(value: Self::Proto) -> Result<Self, WireError>;
}

pub fn invalid(reason: impl Display) -> WireError {
    WireError::Decode(reason.to_string())
}

pub fn limit_exceeded(reason: impl Display) -> WireError {
    WireError::LimitExceeded(reason.to_string())
}

pub fn required<T>(value: Option<T>) -> Result<T, WireError> {
    value.ok_or_else(|| invalid("missing required protobuf message"))
}

pub fn encode<T: Contract>(value: &T) -> Result<Vec<u8>, WireError> {
    let length = value.body_len();
    let mut body = Vec::with_capacity(length + 16);
    // Envelope { uint32 version = 1; Contract payload = 2; }
    encode_key(1, WireType::Varint, &mut body);
    encode_varint(version().into(), &mut body);
    encode_key(2, WireType::LengthDelimited, &mut body);
    encode_varint(length as u64, &mut body);
    value.encode_body(&mut body);
    // Keep producers within exactly the same limits as consumers.
    scan::validate::<T::Proto>(&body, body.len(), version())?;
    Ok(body)
}

/// Exact length from borrowed fields, without encoding or cloning payloads.
pub fn encoded_len<T: Contract>(value: &T) -> Result<usize, WireError> {
    let length = value.body_len();
    Ok(encoding::varint_len(1, version().into()) + key_len(2) + encoded_len_varint(length as u64) + length)
}

pub fn decode<T: Contract>(body: &[u8], limit: usize) -> Result<T, WireError> {
    let payload = scan::validate::<T::Proto>(body, limit, version())?;
    T::from_proto(T::Proto::decode(payload).map_err(invalid)?)
}

#[derive(Clone, PartialEq, Message)]
pub struct Empty {}
impl Schema for Empty {
    fn fields() -> &'static [Field] {
        &[]
    }
}
impl Contract for () {
    type Proto = Empty;
    fn body_len(&self) -> usize {
        0
    }
    fn encode_body(&self, _: &mut Vec<u8>) {}
    fn from_proto(_: Empty) -> Result<Self, WireError> {
        Ok(())
    }
}

#[derive(Clone, PartialEq, Message)]
pub struct List<T: Message + Default + Debug> {
    #[prost(message, repeated, tag = "1")]
    pub items: Vec<T>,
}
impl<T: Message + Default + Schema + Debug> Schema for List<T> {
    fn fields() -> &'static [Field] {
        &[Field { tag: 1, kind: Kind::Message(T::fields), count: 4096 }]
    }
}
impl<T: Contract> Contract for Vec<T> {
    type Proto = List<T::Proto>;
    fn body_len(&self) -> usize {
        self.iter().map(|item| encoding::message_len(1, item)).sum()
    }
    fn encode_body(&self, output: &mut Vec<u8>) {
        for item in self {
            encoding::write_message(1, item, output);
        }
    }
    fn from_proto(value: Self::Proto) -> Result<Self, WireError> {
        value.items.into_iter().map(T::from_proto).collect()
    }
}

/// Typed protobuf framing with a caller-owned version, independent of HTTP.
pub fn encode_message<M: Message + Schema>(message: &M, format_version: u32) -> Result<Vec<u8>, WireError> {
    let mut body = Vec::with_capacity(message.encoded_len() + 16);
    encode_key(1, WireType::Varint, &mut body);
    encode_varint(format_version.into(), &mut body);
    encode_key(2, WireType::LengthDelimited, &mut body);
    encode_varint(message.encoded_len() as u64, &mut body);
    message.encode(&mut body).map_err(invalid)?;
    scan::validate::<M>(&body, body.len(), format_version)?;
    Ok(body)
}

pub fn decode_message<M: Message + Default + Schema>(body: &[u8], limit: usize, format_version: u32) -> Result<M, WireError> {
    let payload = scan::validate::<M>(body, limit, format_version)?;
    M::decode(payload).map_err(invalid)
}
