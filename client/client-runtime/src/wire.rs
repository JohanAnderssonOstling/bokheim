//! The encoded reply shape shared by every host transport.
//!
//! Library work and application work both answer requests with one opaque
//! payload, so the codec belongs to neither side. Browser hosts send these
//! bytes across a shared worker boundary; native hosts pass them between the
//! backend thread and the UI unchanged.

use crate::BackendError;
// Version 4 carries publisher names as strings without publisher identities.
const WIRE_HEADER: &[u8; 5] = b"BKHW\x04";

pub fn encode_worker_message<T: serde::Serialize + ?Sized>(message: &T) -> Result<Vec<u8>, BackendError> {
    let mut bytes = WIRE_HEADER.to_vec();
    message.serialize(&mut rmp_serde::Serializer::new(&mut bytes).with_struct_map()).map_err(BackendError::operation)?;
    Ok(bytes)
}

pub fn decode_worker_message<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, BackendError> {
    let payload = bytes.strip_prefix(WIRE_HEADER).ok_or_else(|| BackendError::message("unsupported backend worker protocol"))?;
    rmp_serde::from_slice(payload).map_err(BackendError::operation)
}

/// One encoded reply. Commands that carry bytes serialize them as `serde_bytes`
/// values inside this message rather than as a second payload shape.
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct WorkerPayload(#[serde(with = "serde_bytes")] pub Vec<u8>);

pub fn encode_reply<T: serde::Serialize>(value: T) -> Result<WorkerPayload, BackendError> {
    encode_worker_message(&value).map(WorkerPayload)
}

pub fn decode_worker_payload<T: serde::de::DeserializeOwned>(payload: WorkerPayload) -> Result<T, BackendError> {
    decode_worker_message(&payload.0)
}
