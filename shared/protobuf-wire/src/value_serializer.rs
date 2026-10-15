//! Representation backend for adapters and canonical map-key ordering.
use crate::serialization::{Backend, Scalar, SequenceOutput, Serializer};
use serde::Serialize;
use serde_value::{SerializerError, Value};
use std::collections::BTreeMap;

pub(crate) fn to_value<T: Serialize + ?Sized>(value: &T) -> Result<Value, SerializerError> {
    to_value_with_mode(value, false)
}
pub(crate) fn to_value_with_mode<T: Serialize + ?Sized>(value: &T, binary: bool) -> Result<Value, SerializerError> {
    value.serialize(Serializer::<Representation>::new(binary))
}
pub(crate) struct Representation;
impl Backend for Representation {
    type Output = Value;
    type Sequence = Vec<Value>;
    type FieldKey = Value;
    fn scalar(value: Scalar) -> Value {
        match value {
            Scalar::Bool(value) => Value::Bool(value),
            Scalar::U8(value) => Value::U8(value),
            Scalar::U16(value) => Value::U16(value),
            Scalar::U32(value) => Value::U32(value),
            Scalar::U64(value) => Value::U64(value),
            Scalar::I8(value) => Value::I8(value),
            Scalar::I16(value) => Value::I16(value),
            Scalar::I32(value) => Value::I32(value),
            Scalar::I64(value) => Value::I64(value),
            Scalar::F32(value) => Value::F32(value),
            Scalar::F64(value) => Value::F64(value),
            Scalar::Char(value) => Value::Char(value),
        }
    }
    fn text(value: &str) -> Result<Value, SerializerError> {
        Ok(Value::String(value.to_owned()))
    }
    fn bytes(value: &[u8]) -> Result<Value, SerializerError> {
        Ok(Value::Bytes(value.to_vec()))
    }
    fn unit() -> Result<Value, SerializerError> {
        Ok(Value::Unit)
    }
    fn optional(value: Option<Value>) -> Result<Value, SerializerError> {
        Ok(Value::Option(value.map(Box::new)))
    }
    fn newtype(value: Value) -> Result<Value, SerializerError> {
        Ok(Value::Newtype(Box::new(value)))
    }
    fn sequence(capacity: usize) -> Vec<Value> {
        Vec::with_capacity(capacity)
    }
    fn map(values: BTreeMap<Value, Value>) -> Result<Value, SerializerError> {
        Ok(Value::Map(values))
    }
    fn field_key(name: &'static str) -> Value {
        Value::String(name.to_owned())
    }
    fn fields(values: BTreeMap<Value, Value>) -> Result<Value, SerializerError> {
        Self::map(values)
    }
}
impl SequenceOutput<Value> for Vec<Value> {
    fn push(&mut self, value: Value) -> Result<(), SerializerError> {
        Vec::push(self, value);
        Ok(())
    }
    fn finish(self) -> Result<Value, SerializerError> {
        Ok(Value::Seq(self))
    }
}
