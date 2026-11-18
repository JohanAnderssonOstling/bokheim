use super::{WireError, invalid};
use prost::encoding::{WireType, decode_key, decode_varint};

pub trait Schema {
    const MAX_FIELDS: usize = DEFAULT_MAX_FIELDS;
    const MAX_DEPTH: usize = DEFAULT_MAX_DEPTH;
    fn fields() -> &'static [Field];
}

pub struct Field {
    pub tag: u32,
    pub kind: Kind,
    pub count: usize,
}

pub enum Kind {
    Varint(u64),
    Bytes(usize),
    Fixed32,
    Oneof(fn() -> &'static [Field]),
    Message(fn() -> &'static [Field]),
}

// Bounds structural expansion independently of the accepted byte-body limit.
// Field counts bound message and Vec overhead; byte buffers are additionally
// bounded by the request body and their individual schema limits.
const DEFAULT_MAX_FIELDS: usize = 131_072;
const DEFAULT_MAX_DEPTH: usize = 32;

fn take<'a>(input: &mut &'a [u8]) -> Result<&'a [u8], WireError> {
    let length = usize::try_from(decode_varint(input).map_err(invalid)?).map_err(invalid)?;
    if length > input.len() {
        return Err(invalid("truncated protobuf field"));
    }
    let (value, rest) = input.split_at(length);
    *input = rest;
    Ok(value)
}

pub(super) fn validate<T: Schema>(body: &[u8], limit: usize, expected_version: u32) -> Result<&[u8], WireError> {
    if body.len() > limit {
        return Err(WireError::TooLarge { limit });
    }
    let mut input = body;
    if decode_key(&mut input).map_err(invalid)? != (1, WireType::Varint) {
        return Err(WireError::InvalidHeader);
    }
    let actual = decode_varint(&mut input).map_err(invalid)?;
    if actual != u64::from(expected_version) {
        return Err(WireError::UnsupportedVersion(u32::try_from(actual).map_err(invalid)?));
    }
    if decode_key(&mut input).map_err(invalid)? != (2, WireType::LengthDelimited) {
        return Err(WireError::InvalidHeader);
    }
    let payload = take(&mut input)?;
    if !input.is_empty() {
        return Err(invalid("trailing protobuf envelope fields"));
    }
    validate_message::<T>(payload, limit)?;
    Ok(payload)
}

pub fn validate_message<T: Schema>(body: &[u8], limit: usize) -> Result<(), WireError> {
    if body.len() > limit {
        return Err(WireError::TooLarge { limit });
    }
    let mut remaining = T::MAX_FIELDS;
    scan(body, T::fields(), T::MAX_DEPTH, &mut remaining)
}

fn scan(mut input: &[u8], schema: &[Field], depth_left: usize, remaining: &mut usize) -> Result<(), WireError> {
    if depth_left == 0 || schema.len() > 32 {
        return Err(invalid("protobuf schema nesting limit exceeded"));
    }
    let mut counts = [0usize; 32];
    let mut oneof_seen = false;
    while !input.is_empty() {
        *remaining = remaining.checked_sub(1).ok_or_else(|| invalid("protobuf structural field budget exceeded"))?;
        let (tag, wire) = decode_key(&mut input).map_err(invalid)?;
        let index = schema.iter().position(|field| field.tag == tag).ok_or_else(|| invalid("unknown protobuf field"))?;
        let field = &schema[index];
        counts[index] += 1;
        if counts[index] > field.count {
            return Err(WireError::TooLarge { limit: field.count });
        }
        if matches!(field.kind, Kind::Oneof(_)) {
            if oneof_seen {
                return Err(invalid("multiple protobuf oneof alternatives"));
            }
            oneof_seen = true;
        }
        match (&field.kind, wire) {
            (Kind::Varint(maximum), WireType::Varint) => {
                if decode_varint(&mut input).map_err(invalid)? > *maximum {
                    return Err(invalid("protobuf scalar exceeds its type range"));
                }
            }
            (Kind::Bytes(maximum), WireType::LengthDelimited) => {
                if take(&mut input)?.len() > *maximum {
                    return Err(WireError::TooLarge { limit: *maximum });
                }
            }
            (Kind::Message(fields) | Kind::Oneof(fields), WireType::LengthDelimited) => scan(take(&mut input)?, fields(), depth_left - 1, remaining)?,
            (Kind::Fixed32, WireType::ThirtyTwoBit) => {
                if input.len() < 4 {
                    return Err(invalid("truncated fixed32"));
                }
                input = &input[4..];
            }
            _ => return Err(invalid("protobuf field has incorrect wire type")),
        }
    }
    Ok(())
}
