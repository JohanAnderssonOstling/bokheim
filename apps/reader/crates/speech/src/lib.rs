//! Narration through native offline voices or the browser's speech service.
pub use operation_failure::OperationError as SpeechError;
mod chunks;
#[cfg(not(target_arch = "wasm32"))]
pub mod device;
mod protocol;
mod voices;

pub use chunks::{FIRST_CHUNK_CHARS, MAX_CHUNK_CHARS, next_chunk, next_chunk_with_language, next_chunk_with_limit};
#[cfg(not(target_arch = "wasm32"))]
pub use device::device_voices;
pub use protocol::{Pauses, SpeechChunk, SpeechControl, SpeechEvent, SpeechFailure, SpeechPassage};
pub use voices::{DeviceVoice, language_base, matching_device_voices, preview_text, voice_matches_languages};

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
pub use native::*;

#[cfg(target_arch = "wasm32")]
mod browser;
#[cfg(target_arch = "wasm32")]
pub use browser::*;
