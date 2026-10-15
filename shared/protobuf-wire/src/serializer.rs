//! Protobuf storage backend for the shared Serde traversal.
use crate::protobuf::{self, value::Kind, Value};
use crate::protobuf_value;
use crate::serialization::{Backend, Scalar, SequenceOutput, Serializer};
use serde::Serialize;
use serde_value::{SerializerError, Value as Key};
use std::collections::BTreeMap;

pub(crate) fn to_value<T: Serialize + ?Sized>(value: &T) -> Result<Value, SerializerError> {
    value.serialize(Serializer::<Protobuf>::new(false)).map(|node| node.value)
}
pub(crate) struct Protobuf;
pub(crate) struct Node {
    value: Value,
    byte: Option<u8>,
}
fn node(kind: Kind) -> Node {
    Node { value: Value { kind: Some(kind) }, byte: None }
}
pub(crate) struct Sequence {
    values: Vec<Value>,
    bytes: Vec<u8>,
    all_bytes: bool,
    capacity: usize,
}
impl Backend for Protobuf {
    type Output = Node;
    type Sequence = Sequence;
    type FieldKey = &'static str;
    fn scalar(value: Scalar) -> Node {
        Node { value: Value { kind: Some(value.kind()) }, byte: value.byte() }
    }
    fn text(value: &str) -> Result<Node, SerializerError> {
        Ok(node(Kind::StringValue(value.to_owned())))
    }
    fn bytes(value: &[u8]) -> Result<Node, SerializerError> {
        Ok(node(Kind::BytesValue(value.to_vec())))
    }
    fn unit() -> Result<Node, SerializerError> {
        Ok(node(Kind::Unit(protobuf::Unit {})))
    }
    fn optional(value: Option<Node>) -> Result<Node, SerializerError> {
        Ok(node(Kind::OptionValue(Box::new(protobuf::OptionalValue { value: value.map(|value| Box::new(value.value)) }))))
    }
    fn newtype(value: Node) -> Result<Node, SerializerError> {
        Ok(node(Kind::NewtypeValue(Box::new(value.value))))
    }
    fn sequence(capacity: usize) -> Sequence {
        Sequence { values: Vec::new(), bytes: Vec::new(), all_bytes: true, capacity }
    }
    fn map(values: BTreeMap<Key, Node>) -> Result<Node, SerializerError> {
        let entries = values
            .into_iter()
            .map(|(key, value)| Ok(protobuf::MapEntry { key: Some(protobuf_value(key).map_err(|error| <SerializerError as serde::ser::Error>::custom(error.to_string()))?), value: Some(value.value) }))
            .collect::<Result<_, SerializerError>>()?;
        Ok(node(Kind::Map(protobuf::Map { entries })))
    }
    fn field_key(name: &'static str) -> &'static str {
        name
    }
    fn fields(values: BTreeMap<&'static str, Node>) -> Result<Node, SerializerError> {
        let entries = values.into_iter().map(|(name, value)| protobuf::MapEntry { key: Some(node(Kind::StringValue(name.to_owned())).value), value: Some(value.value) }).collect();
        Ok(node(Kind::Map(protobuf::Map { entries })))
    }
    fn variant(name: &str, value: Node) -> Result<Node, SerializerError> {
        Ok(node(Kind::Map(protobuf::Map { entries: vec![protobuf::MapEntry { key: Some(node(Kind::StringValue(name.to_owned())).value), value: Some(value.value) }] })))
    }
}
impl SequenceOutput<Node> for Sequence {
    fn push(&mut self, value: Node) -> Result<(), SerializerError> {
        if self.all_bytes {
            if let Some(byte) = value.byte {
                if self.bytes.is_empty() {
                    self.bytes.reserve(self.capacity);
                }
                self.bytes.push(byte);
                return Ok(());
            }
            self.all_bytes = false;
            self.values.reserve(self.capacity);
            self.values.extend(self.bytes.drain(..).map(|byte| node(Kind::U32Value(byte.into())).value));
        }
        self.values.push(value.value);
        Ok(())
    }
    fn finish(self) -> Result<Node, SerializerError> {
        Ok(if self.all_bytes { node(Kind::BytesValue(self.bytes)) } else { node(Kind::Sequence(protobuf::Sequence { values: self.values })) })
    }
}
