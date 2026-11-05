//! Installed local speech engines. No adapter may submit text to a network voice.
use crate::DeviceVoice;
#[cfg(any(test, target_os = "android", target_os = "windows", target_os = "macos"))]
mod pcm;

#[cfg(any(target_os = "android", test))]
mod android;
#[cfg(target_os = "android")]
pub use android::{audio, complete, initialize, started};
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "android")]
use android as platform;
#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(target_os = "windows")]
use windows as platform;

/// Mono audio produced by a local voice, before playback and highlighting begin.
pub(crate) struct Audio {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub last: bool,
}

pub fn device_voices() -> Result<Vec<DeviceVoice>, String> {
    #[cfg(any(target_os = "android", target_os = "linux", target_os = "macos", target_os = "windows"))]
    {
        platform::voices()
    }
    #[cfg(not(any(target_os = "android", target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        Ok(Vec::new())
    }
}

pub(crate) fn synthesize(id: &str, text: &str, rate: f32, cancelled: impl Fn() -> bool, emit: impl FnMut(Audio) -> bool) -> Result<(), String> {
    #[cfg(any(target_os = "android", target_os = "linux", target_os = "macos", target_os = "windows"))]
    {
        platform::synthesize(id, text, rate, cancelled, emit)
    }
    #[cfg(not(any(target_os = "android", target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let _ = (id, text, rate, cancelled, emit);
        Err("Local device voices are unavailable on this platform.".into())
    }
}
