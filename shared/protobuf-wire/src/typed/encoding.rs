//! Borrowed field encoding: payload buffers copy directly into the final body.
use super::{Contract, Field, Kind, Schema, WireError, invalid, limit_exceeded};
use prost::encoding::{WireType, encode_key, encode_varint, encoded_len_varint, key_len};

pub fn varint_len(tag: u32, value: u64) -> usize {
    key_len(tag) + encoded_len_varint(value)
}
pub fn bytes_len(tag: u32, value: &[u8]) -> usize {
    delimited_len(tag, value.len())
}
/// Size of a length-delimited field without constructing its contents.
pub fn delimited_len(tag: u32, length: usize) -> usize {
    key_len(tag) + encoded_len_varint(length as u64) + length
}
pub fn message_len<T: Contract>(tag: u32, value: &T) -> usize {
    let length = value.body_len();
    key_len(tag) + encoded_len_varint(length as u64) + length
}
pub fn write_varint(tag: u32, value: u64, output: &mut Vec<u8>) {
    encode_key(tag, WireType::Varint, output);
    encode_varint(value, output);
}
pub fn write_bytes(tag: u32, value: &[u8], output: &mut Vec<u8>) {
    encode_key(tag, WireType::LengthDelimited, output);
    encode_varint(value.len() as u64, output);
    output.extend_from_slice(value);
}
pub fn write_message<T: Contract>(tag: u32, value: &T, output: &mut Vec<u8>) {
    encode_key(tag, WireType::LengthDelimited, output);
    encode_varint(value.body_len() as u64, output);
    value.encode_body(output);
}

/// One borrowed traversal supports bounded sizing and writing without DTOs.
pub struct Encoder<'a> {
    output: Option<&'a mut Vec<u8>>,
    length: usize,
    schema: Option<&'static [Field]>,
    counts: [usize; 32],
    oneof_seen: bool,
    depth: usize,
    remaining_fields: usize,
    byte_limit: usize,
}
impl Default for Encoder<'_> {
    fn default() -> Self {
        Self { output: None, length: 0, schema: None, counts: [0; 32], oneof_seen: false, depth: usize::MAX, remaining_fields: usize::MAX, byte_limit: usize::MAX }
    }
}
impl<'a> Encoder<'a> {
    pub fn new(output: &'a mut Vec<u8>) -> Self {
        Self { output: Some(output), ..Self::default() }
    }
    /// Check the same schema and structural budgets as the borrowed wire scanner.
    pub fn bounded<S: Schema>(byte_limit: usize) -> Result<Self, WireError> {
        Self::with_limits(S::fields(), S::MAX_DEPTH, S::MAX_FIELDS, byte_limit)
    }
    pub fn with_limits(schema: &'static [Field], depth: usize, remaining_fields: usize, byte_limit: usize) -> Result<Self, WireError> {
        if depth == 0 || schema.len() > 32 {
            return Err(limit_exceeded("protobuf nesting limit exceeded"));
        }
        Ok(Self { schema: Some(schema), depth, remaining_fields, byte_limit, ..Self::default() })
    }
    pub fn len(&self) -> usize {
        self.length
    }
    pub fn is_empty(&self) -> bool {
        self.length == 0
    }

    fn field(&mut self, tag: u32) -> Result<Option<&'static Kind>, WireError> {
        self.remaining_fields = self.remaining_fields.checked_sub(1).ok_or_else(|| limit_exceeded("protobuf total field-count limit exceeded"))?;
        let Some(schema) = self.schema else {
            return Ok(None);
        };
        let index = schema.iter().position(|field| field.tag == tag).ok_or_else(|| invalid(format_args!("unknown protobuf field {tag}")))?;
        let field = &schema[index];
        if self.counts[index] >= field.count {
            return Err(limit_exceeded(format_args!("protobuf field {tag} collection/count limit exceeded: maximum {}", field.count)));
        }
        self.counts[index] += 1;
        if matches!(field.kind, Kind::Oneof(_)) {
            if self.oneof_seen {
                return Err(invalid("multiple protobuf oneof alternatives"));
            }
            self.oneof_seen = true;
        }
        Ok(Some(&field.kind))
    }
    fn add(&mut self, length: usize) -> Result<(), WireError> {
        self.length = self.length.checked_add(length).filter(|length| *length <= self.byte_limit).ok_or_else(|| limit_exceeded("protobuf total byte-size limit exceeded"))?;
        Ok(())
    }
    fn byte_field(&mut self, tag: u32, length: usize) -> Result<(), WireError> {
        match self.field(tag)? {
            Some(Kind::Bytes(maximum)) if length > *maximum => return Err(limit_exceeded(format_args!("protobuf field {tag} byte-size limit exceeded: maximum {maximum}"))),
            Some(Kind::Bytes(_)) | None => {}
            _ => return Err(invalid("protobuf field has incorrect wire type")),
        }
        self.add(key_len(tag))?;
        self.add(encoded_len_varint(length as u64))?;
        self.add(length)
    }
    pub fn bytes(&mut self, tag: u32, value: &[u8]) -> Result<(), WireError> {
        self.byte_field(tag, value.len())?;
        if let Some(output) = &mut self.output {
            write_bytes(tag, value, output);
        }
        Ok(())
    }
    /// Write a fixed-width numeric column directly, without a temporary byte buffer.
    pub fn byte_chunks<const N: usize>(&mut self, tag: u32, chunks: impl ExactSizeIterator<Item = [u8; N]>) -> Result<(), WireError> {
        let length = chunks.len().checked_mul(N).ok_or_else(|| limit_exceeded("numeric column size overflow"))?;
        self.byte_field(tag, length)?;
        if let Some(output) = &mut self.output {
            encode_key(tag, WireType::LengthDelimited, output);
            encode_varint(length as u64, output);
            let start = output.len();
            for chunk in chunks {
                if output.len() - start >= length {
                    return Err(invalid("numeric column iterator length mismatch"));
                }
                output.extend_from_slice(&chunk);
            }
            if output.len() - start != length {
                return Err(invalid("numeric column iterator length mismatch"));
            }
        }
        Ok(())
    }
    pub fn varint(&mut self, tag: u32, value: u64) -> Result<(), WireError> {
        match self.field(tag)? {
            Some(Kind::Varint(maximum)) if value > *maximum => return Err(invalid("protobuf scalar exceeds its type range")),
            Some(Kind::Varint(_)) | None => {}
            _ => return Err(invalid("protobuf field has incorrect wire type")),
        }
        self.add(varint_len(tag, value))?;
        if let Some(output) = &mut self.output {
            write_varint(tag, value, output);
        }
        Ok(())
    }
    pub fn float(&mut self, tag: u32, value: f32) -> Result<(), WireError> {
        if !matches!(self.field(tag)?, Some(Kind::Fixed32) | None) {
            return Err(invalid("protobuf field has incorrect wire type"));
        }
        self.add(key_len(tag) + 4)?;
        if let Some(output) = &mut self.output {
            encode_key(tag, WireType::ThirtyTwoBit, output);
            output.extend_from_slice(&value.to_le_bytes());
        }
        Ok(())
    }
    pub fn message(&mut self, tag: u32, encode: impl Fn(&mut Encoder<'_>) -> Result<(), WireError>) -> Result<(), WireError> {
        let fields = match self.field(tag)? {
            Some(Kind::Message(fields) | Kind::Oneof(fields)) => Some(fields()),
            None => None,
            _ => return Err(invalid("protobuf field has incorrect wire type")),
        };
        // Reserve the key and minimum length prefix before descending.
        self.add(key_len(tag) + 1)?;
        let mut counter = match fields {
            Some(fields) => Self::with_limits(fields, self.depth - 1, self.remaining_fields, self.byte_limit - self.length)?,
            None => Encoder::default(),
        };
        encode(&mut counter)?;
        self.remaining_fields = counter.remaining_fields;
        self.add(encoded_len_varint(counter.length as u64) - 1)?;
        self.add(counter.length)?;
        if let Some(output) = &mut self.output {
            encode_key(tag, WireType::LengthDelimited, output);
            encode_varint(counter.length as u64, output);
            encode(&mut Encoder::new(output))?;
        }
        Ok(())
    }
    /// Format into the output without a temporary string; stop counting at the limit.
    pub fn display(&mut self, tag: u32, value: impl std::fmt::Display) -> Result<(), WireError> {
        use std::fmt::Write;
        struct Text<'a> {
            output: Option<&'a mut Vec<u8>>,
            length: usize,
            limit: usize,
        }
        impl std::fmt::Write for Text<'_> {
            fn write_str(&mut self, value: &str) -> std::fmt::Result {
                self.length = self.length.checked_add(value.len()).filter(|length| *length <= self.limit).ok_or(std::fmt::Error)?;
                if let Some(output) = &mut self.output {
                    output.extend_from_slice(value.as_bytes());
                }
                Ok(())
            }
        }
        let limit = match self.schema.and_then(|fields| fields.iter().find(|field| field.tag == tag)) {
            Some(Field { kind: Kind::Bytes(maximum), .. }) => (*maximum).min(self.byte_limit - self.length),
            _ => self.byte_limit - self.length,
        };
        let mut counter = Text { output: None, length: 0, limit };
        write!(&mut counter, "{value}").map_err(|_| limit_exceeded(format_args!("protobuf field {tag} formatted byte-size limit exceeded: maximum {limit}")))?;
        self.byte_field(tag, counter.length)?;
        if let Some(output) = &mut self.output {
            encode_key(tag, WireType::LengthDelimited, output);
            encode_varint(counter.length as u64, output);
            write!(&mut Text { output: Some(output), length: 0, limit }, "{value}").map_err(invalid)?;
        }
        Ok(())
    }
}
