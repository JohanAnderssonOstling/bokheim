//! Read enum names from Serde without serializing their payloads or maintaining
//! another catalogue. Both directions honor Serde's rename attributes.

pub(crate) use serde::Serialize;
pub(crate) use serde::de;
pub(crate) use serde::ser::{self, Impossible, SerializeStructVariant, SerializeTupleVariant};
use std::fmt::Display;

pub(crate) type Error = de::value::Error;
pub(crate) type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub(crate) struct Names(pub(crate) &'static [&'static str]);
impl Display for Names {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("enum variant names")
    }
}
impl std::error::Error for Names {}
impl de::Error for Names {
    fn custom<T: Display>(_: T) -> Self {
        Self(&[])
    }
    fn unknown_variant(_: &str, expected: &'static [&'static str]) -> Self {
        Self(expected)
    }
}

pub(crate) struct NameSerializer;
pub(crate) struct Name(pub(crate) &'static str);

pub(crate) fn unsupported<T>() -> Result<T> {
    Err(ser::Error::custom("expected an enum variant"))
}

impl ser::Serializer for NameSerializer {
    type Ok = &'static str;
    type Error = Error;
    type SerializeSeq = Impossible<Self::Ok, Error>;
    type SerializeTuple = Impossible<Self::Ok, Error>;
    type SerializeTupleStruct = Impossible<Self::Ok, Error>;
    type SerializeTupleVariant = Name;
    type SerializeMap = Impossible<Self::Ok, Error>;
    type SerializeStruct = Impossible<Self::Ok, Error>;
    type SerializeStructVariant = Name;

    fn serialize_unit_variant(self, _: &'static str, _: u32, variant: &'static str) -> Result<Self::Ok> {
        Ok(variant)
    }
    fn serialize_newtype_variant<T: ?Sized + Serialize>(self, _: &'static str, _: u32, variant: &'static str, _: &T) -> Result<Self::Ok> {
        Ok(variant)
    }
    fn serialize_tuple_variant(self, _: &'static str, _: u32, variant: &'static str, _: usize) -> Result<Name> {
        Ok(Name(variant))
    }
    fn serialize_struct_variant(self, _: &'static str, _: u32, variant: &'static str, _: usize) -> Result<Name> {
        Ok(Name(variant))
    }

    fn serialize_bool(self, _: bool) -> Result<Self::Ok> {
        unsupported()
    }
    fn serialize_i8(self, _: i8) -> Result<Self::Ok> {
        unsupported()
    }
    fn serialize_i16(self, _: i16) -> Result<Self::Ok> {
        unsupported()
    }
    fn serialize_i32(self, _: i32) -> Result<Self::Ok> {
        unsupported()
    }
    fn serialize_i64(self, _: i64) -> Result<Self::Ok> {
        unsupported()
    }
    fn serialize_u8(self, _: u8) -> Result<Self::Ok> {
        unsupported()
    }
    fn serialize_u16(self, _: u16) -> Result<Self::Ok> {
        unsupported()
    }
    fn serialize_u32(self, _: u32) -> Result<Self::Ok> {
        unsupported()
    }
    fn serialize_u64(self, _: u64) -> Result<Self::Ok> {
        unsupported()
    }
    fn serialize_f32(self, _: f32) -> Result<Self::Ok> {
        unsupported()
    }
    fn serialize_f64(self, _: f64) -> Result<Self::Ok> {
        unsupported()
    }
    fn serialize_char(self, _: char) -> Result<Self::Ok> {
        unsupported()
    }
    fn serialize_str(self, _: &str) -> Result<Self::Ok> {
        unsupported()
    }
    fn serialize_bytes(self, _: &[u8]) -> Result<Self::Ok> {
        unsupported()
    }
    fn serialize_none(self) -> Result<Self::Ok> {
        unsupported()
    }
    fn serialize_some<T: ?Sized + Serialize>(self, _: &T) -> Result<Self::Ok> {
        unsupported()
    }
    fn serialize_unit(self) -> Result<Self::Ok> {
        unsupported()
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result<Self::Ok> {
        unsupported()
    }
    fn serialize_newtype_struct<T: ?Sized + Serialize>(self, _: &'static str, _: &T) -> Result<Self::Ok> {
        unsupported()
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq> {
        unsupported()
    }
    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple> {
        unsupported()
    }
    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Result<Self::SerializeTupleStruct> {
        unsupported()
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap> {
        unsupported()
    }
    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self::SerializeStruct> {
        unsupported()
    }
}

impl SerializeStructVariant for Name {
    type Ok = &'static str;
    type Error = Error;
    fn serialize_field<T: ?Sized + Serialize>(&mut self, _: &'static str, _: &T) -> Result<()> {
        Ok(())
    }
    fn end(self) -> Result<Self::Ok> {
        Ok(self.0)
    }
}
impl SerializeTupleVariant for Name {
    type Ok = &'static str;
    type Error = Error;
    fn serialize_field<T: ?Sized + Serialize>(&mut self, _: &T) -> Result<()> {
        Ok(())
    }
    fn end(self) -> Result<Self::Ok> {
        Ok(self.0)
    }
}
