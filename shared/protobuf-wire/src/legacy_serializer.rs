//! Serde value trees for protobuf. Named ContentHash values use byte strings;
//! other domain values keep their established human-readable representation.
use serde::ser::{self, Serialize};
use serde_value::{SerializerError, Value};
use std::collections::BTreeMap;

pub(crate) fn to_value<T: Serialize + ?Sized>(value: &T) -> Result<Value, SerializerError> {
    to_value_with_mode(value, false)
}
pub(crate) fn to_value_with_mode<T: Serialize + ?Sized>(value: &T, binary: bool) -> Result<Value, SerializerError> { value.serialize(Serializer(binary)) }
struct Serializer(bool);
struct Sequence {
    values: Vec<Value>,
    variant: Option<String>,
    binary: bool,
}
struct Mapping {
    values: BTreeMap<Value, Value>,
    key: Option<Value>,
    variant: Option<String>,
    binary: bool,
}
macro_rules! scalar { ($($method:ident($ty:ty) => $variant:ident),*) => { $(fn $method(self, value: $ty) -> Result<Value, SerializerError> { Ok(Value::$variant(value)) })* }; }
impl ser::Serializer for Serializer {
    type Ok = Value;
    type Error = SerializerError;
    type SerializeSeq = Sequence;
    type SerializeTuple = Sequence;
    type SerializeTupleStruct = Sequence;
    type SerializeTupleVariant = Sequence;
    type SerializeMap = Mapping;
    type SerializeStruct = Mapping;
    type SerializeStructVariant = Mapping;
    scalar!(serialize_bool(bool)=>Bool, serialize_i8(i8)=>I8, serialize_i16(i16)=>I16, serialize_i32(i32)=>I32, serialize_i64(i64)=>I64,
        serialize_u8(u8)=>U8, serialize_u16(u16)=>U16, serialize_u32(u32)=>U32, serialize_u64(u64)=>U64, serialize_f32(f32)=>F32, serialize_f64(f64)=>F64, serialize_char(char)=>Char);
    fn is_human_readable(&self) -> bool {
        !self.0
    }
    fn serialize_str(self, value: &str) -> Result<Value, SerializerError> {
        Ok(Value::String(value.to_owned()))
    }
    fn serialize_bytes(self, value: &[u8]) -> Result<Value, SerializerError> {
        Ok(Value::Bytes(value.to_vec()))
    }
    fn serialize_none(self) -> Result<Value, SerializerError> {
        Ok(Value::Option(None))
    }
    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<Value, SerializerError> {
        Ok(Value::Option(Some(Box::new(value.serialize(self)?))))
    }
    fn serialize_unit(self) -> Result<Value, SerializerError> {
        Ok(Value::Unit)
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result<Value, SerializerError> {
        self.serialize_unit()
    }
    fn serialize_unit_variant(self, _: &'static str, _: u32, variant: &'static str) -> Result<Value, SerializerError> {
        self.serialize_str(variant)
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(self, name: &'static str, value: &T) -> Result<Value, SerializerError> {
        if matches!(name, "ContentHash" | "BookId") { value.serialize(Serializer(true)) } else { Ok(Value::Newtype(Box::new(value.serialize(self)?))) }
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(self, _: &'static str, _: u32, variant: &'static str, value: &T) -> Result<Value, SerializerError> {
        Ok(Value::Map(BTreeMap::from([(Value::String(variant.to_owned()), value.serialize(self)?)])))
    }
    fn serialize_seq(self, len: Option<usize>) -> Result<Sequence, SerializerError> {
        Ok(Sequence { values: Vec::with_capacity(len.unwrap_or_default()), variant: None, binary: self.0 })
    }
    fn serialize_tuple(self, len: usize) -> Result<Sequence, SerializerError> {
        self.serialize_seq(Some(len))
    }
    fn serialize_tuple_struct(self, _: &'static str, len: usize) -> Result<Sequence, SerializerError> {
        self.serialize_tuple(len)
    }
    fn serialize_tuple_variant(self, _: &'static str, _: u32, variant: &'static str, len: usize) -> Result<Sequence, SerializerError> {
        Ok(Sequence { variant: Some(variant.to_owned()), ..self.serialize_tuple(len)? })
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Mapping, SerializerError> {
        Ok(Mapping { values: BTreeMap::new(), key: None, variant: None, binary: self.0 })
    }
    fn serialize_struct(self, _: &'static str, len: usize) -> Result<Mapping, SerializerError> {
        self.serialize_map(Some(len))
    }
    fn serialize_struct_variant(self, _: &'static str, _: u32, variant: &'static str, len: usize) -> Result<Mapping, SerializerError> {
        Ok(Mapping { variant: Some(variant.to_owned()), ..self.serialize_map(Some(len))? })
    }
}
impl Sequence {
    fn push<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), SerializerError> {
        self.values.push(value.serialize(Serializer(self.binary))?);
        Ok(())
    }
    fn finish(self) -> Result<Value, SerializerError> {
        let value = Value::Seq(self.values);
        Ok(match self.variant {
            Some(name) => Value::Map(BTreeMap::from([(Value::String(name), value)])),
            None => value,
        })
    }
}
macro_rules! sequence {
    ($trait:ident,$method:ident) => {
        impl ser::$trait for Sequence {
            type Ok = Value;
            type Error = SerializerError;
            fn $method<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), SerializerError> {
                self.push(value)
            }
            fn end(self) -> Result<Value, SerializerError> {
                self.finish()
            }
        }
    };
}
sequence!(SerializeSeq, serialize_element);
sequence!(SerializeTuple, serialize_element);
sequence!(SerializeTupleStruct, serialize_field);
sequence!(SerializeTupleVariant, serialize_field);
impl Mapping {
    fn field<T: Serialize + ?Sized>(&mut self, name: &str, value: &T) -> Result<(), SerializerError> {
        self.values.insert(Value::String(name.to_owned()), value.serialize(Serializer(self.binary))?);
        Ok(())
    }
    fn finish(self) -> Result<Value, SerializerError> {
        let value = Value::Map(self.values);
        Ok(match self.variant {
            Some(name) => Value::Map(BTreeMap::from([(Value::String(name), value)])),
            None => value,
        })
    }
}
impl ser::SerializeMap for Mapping {
    type Ok = Value;
    type Error = SerializerError;
    fn serialize_key<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), SerializerError> {
        self.key = Some(value.serialize(Serializer(self.binary))?);
        Ok(())
    }
    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), SerializerError> {
        let key = self.key.take().ok_or_else(|| <SerializerError as ser::Error>::custom("map value has no key"))?;
        self.values.insert(key, value.serialize(Serializer(self.binary))?);
        Ok(())
    }
    fn end(self) -> Result<Value, SerializerError> {
        self.finish()
    }
}
macro_rules! mapping {
    ($trait:ident) => {
        impl ser::$trait for Mapping {
            type Ok = Value;
            type Error = SerializerError;
            fn serialize_field<T: Serialize + ?Sized>(&mut self, name: &'static str, value: &T) -> Result<(), SerializerError> {
                self.field(name, value)
            }
            fn end(self) -> Result<Value, SerializerError> {
                self.finish()
            }
        }
    };
}
mapping!(SerializeStruct);
mapping!(SerializeStructVariant);
