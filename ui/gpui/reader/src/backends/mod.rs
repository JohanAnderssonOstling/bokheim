//! Platform audio and speech backends the reader drives.
//!
//! Both are traits so builds without the corresponding system feature, and
//! tests, can substitute an inert or fake implementation.

pub(crate) mod audio;
pub(crate) mod tts;

pub use audio::{NativeAudioBackend, NativeAudioState, NativeAudioStatus, SystemAudioBackend};
pub use tts::{SystemTtsProvider, TtsProvider, TtsState};
