use crate::{
    api,
    http::{Contract, Field, Kind, Schema, WireError, invalid},
};
use prost::Message;

#[derive(Clone, PartialEq, Message)]
pub struct NotificationProto {
    #[prost(uint32, tag = "1")]
    pub kind: u32,
    #[prost(uint64, optional, tag = "2")]
    pub heartbeat_seconds: Option<u64>,
    #[prost(uint64, optional, tag = "3")]
    pub timeout_seconds: Option<u64>,
    #[prost(bytes = "vec", optional, tag = "4")]
    pub library_id: Option<Vec<u8>>,
}
impl Schema for NotificationProto {
    fn fields() -> &'static [Field] {
        &[Field { tag: 1, kind: Kind::Varint(4), count: 1 }, Field { tag: 2, kind: Kind::Varint(u64::MAX), count: 1 }, Field { tag: 3, kind: Kind::Varint(u64::MAX), count: 1 }, Field { tag: 4, kind: Kind::Bytes(16), count: 1 }]
    }
}
impl Contract for api::notifications::NotificationMessage {
    type Proto = NotificationProto;
    fn body_len(&self) -> usize {
        notification_proto(self).encoded_len()
    }
    fn encode_body(&self, output: &mut Vec<u8>) {
        notification_proto(self).encode(output).expect("Vec writes cannot fail");
    }
    fn from_proto(value: Self::Proto) -> Result<Self, WireError> {
        match (value.kind, value.heartbeat_seconds, value.timeout_seconds, value.library_id) {
            (1, Some(heartbeat_seconds), Some(timeout_seconds), None) => Ok(Self::Hello { heartbeat_seconds, timeout_seconds }),
            (2, None, None, Some(id)) => Ok(Self::LibraryChanged { library_id: uuid::Uuid::from_slice(&id).map_err(invalid)? }),
            (3, None, None, None) => Ok(Self::LibrariesChanged),
            (4, None, None, None) => Ok(Self::Reconcile),
            _ => Err(invalid("invalid notification fields")),
        }
    }
}
fn notification_proto(value: &api::notifications::NotificationMessage) -> NotificationProto {
    use api::notifications::NotificationMessage as Notification;
    match value {
        Notification::Hello { heartbeat_seconds, timeout_seconds } => NotificationProto { kind: 1, heartbeat_seconds: Some(*heartbeat_seconds), timeout_seconds: Some(*timeout_seconds), library_id: None },
        Notification::LibraryChanged { library_id } => NotificationProto { kind: 2, library_id: Some(library_id.as_bytes().to_vec()), ..Default::default() },
        Notification::LibrariesChanged => NotificationProto { kind: 3, ..Default::default() },
        Notification::Reconcile => NotificationProto { kind: 4, ..Default::default() },
    }
}
