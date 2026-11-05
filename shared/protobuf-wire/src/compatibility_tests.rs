use super::*;
use crate::legacy_serializer as value_serializer;
use prost::Message;
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Deserialize, Serialize};
use serde_value::Value;
use std::collections::BTreeMap;

fn previous_encode<T: Serialize + ?Sized>(value: &T) -> Vec<u8> {
    let value = value_serializer::to_value(value).unwrap();
    protobuf::Envelope { version: VERSION, value: Some(protobuf_value(value).unwrap()) }.encode_to_vec()
}
fn assert_bytes<T: Serialize + ?Sized>(value: &T) -> Vec<u8> {
    assert_eq!(crate::value_serializer::to_value(value).unwrap(), value_serializer::to_value(value).unwrap());
    let previous = previous_encode(value);
    assert_eq!(encode(value).unwrap(), previous);
    assert_eq!(encoded_len(value).unwrap(), previous.len());
    previous
}

#[derive(Debug, Deserialize, PartialEq, Serialize)]
struct Newtype(u64);
#[derive(Debug, Deserialize, PartialEq, Serialize)]
enum Shapes {
    Unit,
    Newtype(Newtype),
    Tuple(u8, u8),
    Fields { flag: bool, values: Vec<u64> },
}
#[derive(Debug, Deserialize, PartialEq, Serialize)]
struct Contract {
    unsigned: (u8, u16, u32, u64),
    signed: (i8, i16, i32, i64),
    text: String,
    character: char,
    floats: (f32, f64),
    none: Option<u64>,
    some: Option<Newtype>,
    unit: (),
    variants: Vec<Shapes>,
    empty: Vec<Shapes>,
    bytes: Vec<u8>,
    #[serde(with = "serde_bytes")]
    compact: Vec<u8>,
    maps: BTreeMap<Vec<u8>, Option<Shapes>>,
}

#[test]
fn direct_codec_and_counter_match_previous_scalar_and_compound_contracts() {
    for length in [0, 1, 127, 128, 255, 256, 16_383, 16_384] {
        let value = Contract {
            unsigned: (u8::MAX, u16::MAX, u32::MAX, u64::MAX),
            signed: (i8::MIN, i16::MAX, i32::MIN, i64::MIN),
            text: "Åäö 🦀".repeat(length / 8),
            character: '🦀',
            floats: (-0.0, f64::NEG_INFINITY),
            none: None,
            some: Some(Newtype(u64::MAX)),
            unit: (),
            variants: vec![Shapes::Unit, Shapes::Newtype(Newtype(0)), Shapes::Tuple(0, 255), Shapes::Fields { flag: false, values: vec![0, 128, u64::MAX] }],
            empty: Vec::new(),
            bytes: (0..length).map(|value| value as u8).collect(),
            compact: vec![0xff; length],
            maps: [(Vec::new(), None), (vec![0, 255], Some(Shapes::Tuple(128, 255)))].into(),
        };
        let bytes = assert_bytes(&value);
        assert_eq!(decode::<Contract>(&bytes, MAX_DECODED_RESPONSE_BYTES).unwrap(), value);
        assert_eq!(decode_byte_fields::<Contract>(&bytes, MAX_DECODED_RESPONSE_BYTES).unwrap(), value);
    }
    for value in [0, 1, 127, 128, 16_383, 16_384, u64::MAX] {
        assert_bytes(&value);
    }
    for value in [i64::MIN, i64::MIN + 1, -128, -1, 0, 1, 127, i64::MAX] {
        assert_bytes(&value);
    }
}

#[test]
fn direct_codec_preserves_float_bits_including_nan() {
    for bits in [0, 0x8000_0000, 0x7fc0_0001, 0x7fa0_0001, u32::MAX] {
        let value = f32::from_bits(bits);
        let bytes = assert_bytes(&value);
        assert_eq!(decode::<f32>(&bytes, 4096).unwrap().to_bits(), bits);
    }
    for bits in [0, 0x8000_0000_0000_0000, 0x7ff8_0000_0000_0001, u64::MAX] {
        let bytes = assert_bytes(&f64::from_bits(bits));
        assert_eq!(decode::<f64>(&bytes, 4096).unwrap().to_bits(), bits);
    }
}

#[test]
fn arbitrary_map_keys_retain_type_order_and_last_duplicate_values() {
    let keys = [Value::U8(1), Value::U16(1), Value::U32(1), Value::U64(1), Value::Bool(false), Value::String("a".into()), Value::Bytes(vec![1, 2]), Value::Seq(vec![Value::U8(1)]), Value::Newtype(Box::new(Value::String("a".into())))];
    let value = Value::Map(keys.into_iter().enumerate().map(|(index, key)| (key, Value::U64(index as u64))).collect());
    let bytes = assert_bytes(&value);
    let envelope = protobuf::Envelope::decode(bytes.as_slice()).unwrap();
    let previous = domain_value(envelope.value.unwrap(), false).unwrap();
    assert_eq!(decode::<Value>(&bytes, 4096).unwrap(), previous);
}

#[test]
fn duplicate_fields_and_mixed_sequences_keep_their_original_encoding() {
    struct Duplicate;
    impl Serialize for Duplicate {
        fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            let mut map = serializer.serialize_map(None)?;
            map.serialize_entry("z", &1u64)?;
            map.serialize_entry("a", &0u64)?;
            map.serialize_entry("z", &2u64)?;
            map.end()
        }
    }
    let bytes = assert_bytes(&Duplicate);
    assert_eq!(decode::<BTreeMap<String, u64>>(&bytes, 4096).unwrap(), [("a".into(), 0), ("z".into(), 2)].into());
    struct Mixed;
    impl Serialize for Mixed {
        fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            let mut values = serializer.serialize_seq(None)?;
            values.serialize_element(&1u8)?;
            values.serialize_element(&255u8)?;
            values.serialize_element(&256u16)?;
            values.end()
        }
    }
    let bytes = assert_bytes(&Mixed);
    assert_eq!(decode::<Vec<u16>>(&bytes, 4096).unwrap(), vec![1, 255, 256]);
}

#[test]
fn canonical_comparison_agrees_with_full_reencoding_for_mutated_inputs() {
    let valid = previous_encode(&vec![Shapes::Unit, Shapes::Tuple(0, 255), Shapes::Fields { flag: true, values: vec![1, 128] }]);
    let check = |bytes: &[u8]| {
        if let Ok(envelope) = protobuf::Envelope::decode(bytes) {
            assert_eq!(canonical::matches(&envelope, bytes), envelope.encode_to_vec() == bytes);
        }
    };
    check(&valid);
    for index in 0..valid.len() {
        for mask in [1, 0x80, 0xff] {
            let mut bytes = valid.clone();
            bytes[index] ^= mask;
            check(&bytes);
        }
        check(&valid[..index]);
    }
    for extra in [&[8, VERSION as u8][..], &[24, 0], &[8, 0x9d, 0]] {
        let mut bytes = valid.clone();
        bytes.extend_from_slice(extra);
        check(&bytes);
    }
    let large = previous_encode(&serde_bytes::ByteBuf::from(vec![0xab; 1024 * 1024]));
    check(&large);
    let mut changed = large;
    let length = changed.len();
    changed[length - 1] ^= 1;
    check(&changed);
}

#[test]
fn invalid_values_are_rejected_even_in_ignored_fields() {
    use protobuf::value::Kind;
    let string = |value: &str| protobuf::Value { kind: Some(Kind::StringValue(value.into())) };
    for invalid in [protobuf::Value { kind: None }, protobuf::Value { kind: Some(Kind::CharValue(0xd800)) }] {
        let envelope = protobuf::Envelope {
            version: VERSION,
            value: Some(protobuf::Value {
                kind: Some(Kind::Map(protobuf::Map { entries: vec![protobuf::MapEntry { key: Some(string("name")), value: Some(string("valid")) }, protobuf::MapEntry { key: Some(string("ignored")), value: Some(invalid) }] })),
            }),
        };
        #[derive(Deserialize)]
        struct Name {
            name: String,
        }
        if let Ok(decoded) = decode::<Name>(&envelope.encode_to_vec(), 4096) {
            panic!("invalid envelope accepted with name {:?}", decoded.name);
        }
    }
}

#[test]
fn string_map_fast_path_preserves_unsorted_duplicate_and_compact_values() {
    use protobuf::value::Kind;
    let string = |key: &str| protobuf::Value { kind: Some(Kind::StringValue(key.into())) };
    for names in [vec![], vec!["a"], vec!["a", "b", "🦀"], vec!["🦀", "b", "a"], vec!["b", "a", "b"]] {
        let value = protobuf::Value {
            kind: Some(Kind::Map(protobuf::Map {
                entries: names.iter().enumerate().map(|(index, name)| protobuf::MapEntry { key: Some(string(name)), value: Some(protobuf::Value { kind: Some(Kind::BytesValue(vec![index as u8, 255])) }) }).collect(),
            })),
        };
        let bytes = protobuf::Envelope { version: VERSION, value: Some(value.clone()) }.encode_to_vec();
        assert_eq!(decode::<Value>(&bytes, 4096).unwrap(), domain_value(value.clone(), false).unwrap());
        assert_eq!(decode_byte_fields::<Value>(&bytes, 4096).unwrap(), domain_value(value, true).unwrap());
    }
}

#[test]
fn single_entry_enum_fast_path_keeps_duplicate_variant_and_invalid_map_rules() {
    use protobuf::value::Kind;
    let entry = |name: &str, value| protobuf::MapEntry {
        key: Some(protobuf::Value { kind: Some(Kind::StringValue(name.into())) }),
        value: Some(protobuf::Value { kind: Some(Kind::NewtypeValue(Box::new(protobuf::Value { kind: Some(Kind::U64Value(value)) }))) }),
    };
    let encode_map = |entries| protobuf::Envelope { version: VERSION, value: Some(protobuf::Value { kind: Some(Kind::Map(protobuf::Map { entries })) }) }.encode_to_vec();
    for entries in [vec![entry("Newtype", 2)], vec![entry("Newtype", 1), entry("Newtype", 2)]] {
        assert_eq!(decode::<Shapes>(&encode_map(entries), 4096).unwrap(), Shapes::Newtype(Newtype(2)));
    }
    assert!(decode::<Shapes>(&encode_map(Vec::new()), 4096).is_err());
    assert!(decode::<Shapes>(&encode_map(vec![entry("Newtype", 1), entry("other", 2)]), 4096).is_err());
}

#[test]
fn static_struct_fields_keep_last_duplicates_enum_shapes_and_empty_structs() {
    use serde::ser::{SerializeStruct, SerializeStructVariant};
    struct Duplicate;
    impl Serialize for Duplicate {
        fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            let mut fields = serializer.serialize_struct("Duplicate", 4)?;
            fields.serialize_field("z", &"obsolete long value")?;
            fields.serialize_field("a", &"first")?;
            fields.serialize_field("z", &"last")?;
            fields.serialize_field("🦀", &"unicode")?;
            fields.end()
        }
    }
    let bytes = assert_bytes(&Duplicate);
    assert_eq!(decode::<BTreeMap<String, String>>(&bytes, 4096).unwrap(), [("a".into(), "first".into()), ("z".into(), "last".into()), ("🦀".into(), "unicode".into())].into());
    struct Variant;
    impl Serialize for Variant {
        fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            let mut fields = serializer.serialize_struct_variant("Shapes", 0, "Fields", 3)?;
            fields.serialize_field("flag", &true)?;
            fields.serialize_field("values", &vec![u64::MAX; 10])?;
            fields.serialize_field("values", &vec![1u64])?;
            fields.end()
        }
    }
    let bytes = assert_bytes(&Variant);
    assert_eq!(decode::<Shapes>(&bytes, 4096).unwrap(), Shapes::Fields { flag: true, values: vec![1] });
    #[derive(Serialize)]
    struct Empty {}
    assert_bytes(&Empty {});
}
