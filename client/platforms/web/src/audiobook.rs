//! Browser audiobook presentation state and lifetime leases.
use crate::transport::resources::{AsyncCallback, LocalMediaLease};
use operation_failure::OperationError;
use serde::Serialize;
use wasm_bindgen::prelude::*;

/// Keeps browser-owned local media alive while the player uses its object URL.
pub struct PlaybackLease {
    _local: Option<LocalMediaLease>,
}
impl PlaybackLease {
    pub fn local(local: Option<LocalMediaLease>) -> Self {
        Self { _local: local }
    }
}

/// Browser playback state parameterized by the library's domain metadata.
pub struct BrowserAudiobook<M, D> {
    pub metadata: M,
    pub url: String,
    pub local_tracks: Vec<String>,
    pub direct: Option<D>,
    renew: Option<AsyncCallback>,
    _lease: Option<async_channel::Receiver<()>>,
}
impl<M, D> BrowserAudiobook<M, D>
where
    D: serde::Serialize,
{
    pub fn new(metadata: M, url: String, local_tracks: Vec<String>, direct: Option<D>, lease: Option<async_channel::Receiver<()>>, renew: Option<AsyncCallback>) -> Self {
        Self { metadata, url, local_tracks, direct, renew, _lease: lease }
    }

    pub fn renewal(&self) -> JsValue {
        self.renew.as_ref().map(|renew| renew.value()).unwrap_or(JsValue::UNDEFINED)
    }
    pub fn direct_config(&self) -> Result<JsValue, OperationError> {
        let Some(config) = &self.direct else { return Ok(JsValue::UNDEFINED) };
        playback_config(config)
    }
}

/// Preserve the playback object's JSON representation without a text round trip.
pub fn playback_config(config: &impl Serialize) -> Result<JsValue, OperationError> {
    config.serialize(&serde_wasm_bindgen::Serializer::json_compatible()).map_err(|error| OperationError::from(JsValue::from(error)))
}
