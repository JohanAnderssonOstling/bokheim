//! Installed Microsoft speech voices, synthesized to memory rather than system playback.
use super::{Audio, DeviceVoice, pcm};
use std::time::{Duration, Instant};
use windows::{
    Media::SpeechSynthesis::SpeechSynthesizer,
    Storage::Streams::DataReader,
    Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
    core::HSTRING,
};
use windows_future::AsyncStatus;

struct Apartment(bool);
impl Apartment {
    fn new() -> Result<Self, String> {
        match unsafe { RoInitialize(RO_INIT_MULTITHREADED) } {
            Ok(()) => Ok(Self(true)),
            Err(error) if error.code().0 == 0x80010106u32 as i32 => Ok(Self(false)),
            Err(error) => Err(format!("Could not initialize local Windows speech: {error}")),
        }
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        if self.0 {
            unsafe { RoUninitialize() };
        }
    }
}

pub(super) fn voices() -> Result<Vec<DeviceVoice>, String> {
    let _apartment = Apartment::new()?;
    let voices = SpeechSynthesizer::AllVoices().map_err(|error| format!("Could not list installed Windows voices: {error}"))?;
    let mut result = Vec::new();
    for voice in voices {
        result.push(DeviceVoice {
            id: voice.Id().map_err(|error| error.to_string())?.to_string(),
            name: voice.DisplayName().map_err(|error| error.to_string())?.to_string(),
            language: voice.Language().map_err(|error| error.to_string())?.to_string(),
        });
    }
    Ok(result)
}

pub(super) fn synthesize(id: &str, text: &str, rate: f32, cancelled: impl Fn() -> bool, mut emit: impl FnMut(Audio) -> bool) -> Result<(), String> {
    let _apartment = Apartment::new()?;
    let operation = || -> windows::core::Result<_> {
        let voice = SpeechSynthesizer::AllVoices()?
            .into_iter()
            .find(|voice| voice.Id().is_ok_and(|value| value.to_string() == id))
            .ok_or_else(|| windows::core::Error::new(windows::core::HRESULT(0x80070490u32 as i32), "The selected installed local voice is unavailable."))?;
        let synth = SpeechSynthesizer::new()?;
        synth.SetVoice(&voice)?;
        synth.Options()?.SetSpeakingRate(f64::from(rate))?;
        let operation = synth.SynthesizeTextToStreamAsync(&HSTRING::from(text))?;
        Ok((synth, operation))
    };
    let (synth, operation) = operation().map_err(|error| format!("The installed local Windows voice could not start: {error}"))?;
    let deadline = Instant::now() + Duration::from_secs(60);
    while operation.Status().map_err(|error| error.to_string())? == AsyncStatus::Started {
        if cancelled() || Instant::now() >= deadline {
            let _ = operation.Cancel();
            let _ = synth.Close();
            return Err(if cancelled() { "Cancelled" } else { "The local voice took too long to generate a reading sample. Try another installed voice." }.into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let result = (|| -> Result<Audio, String> {
        let stream = operation.GetResults().map_err(|error| format!("Local voice synthesis failed: {error}"))?;
        let size = stream.Size().map_err(|error| error.to_string())?;
        if size == 0 || size > pcm::MAX_AUDIO_BYTES as u64 {
            return Err("The local voice returned empty or oversized audio.".into());
        }
        let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
        let loaded = reader.LoadAsync(size as u32).map_err(|error| error.to_string())?.join().map_err(|error| error.to_string())?;
        if u64::from(loaded) != size {
            return Err("The local voice audio stream was incomplete.".into());
        }
        let mut bytes = vec![0; size as usize];
        reader.ReadBytes(&mut bytes).map_err(|error| error.to_string())?;
        pcm::wave(&bytes)
    })();
    let _ = synth.Close();
    if cancelled() {
        return Err("Cancelled".into());
    }
    if emit(result?) { Ok(()) } else { Err("Cancelled".into()) }
}
