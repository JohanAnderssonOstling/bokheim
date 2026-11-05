//! Versioned encoding for diagnostic tooling.

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
