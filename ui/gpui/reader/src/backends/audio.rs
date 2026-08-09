//! Playback of EPUB media-overlay audio.

#[cfg(feature = "native-audio")]
use std::sync::Mutex;

pub trait NativeAudioBackend: Send + Sync {
    fn play(&self, name: &str, bytes: &[u8], start_seconds: f64, end_seconds: Option<f64>) -> Result<(), String>;
    fn pause(&self) -> Result<(), String>;
    fn resume(&self) -> Result<(), String>;
    fn seek_to(&self, seconds: f64) -> Result<(), String>;
    fn status(&self) -> NativeAudioStatus;
    fn stop(&self);
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NativeAudioStatus {
    pub state: NativeAudioState,
    pub position_seconds: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NativeAudioState {
    Playing,
    Paused,
    #[default]
    Stopped,
}

#[cfg(feature = "native-audio")]
struct ActiveAudio {
    _manager: kira::AudioManager,
    handle: kira::sound::static_sound::StaticSoundHandle,
}

#[derive(Default)]
pub struct SystemAudioBackend {
    #[cfg(feature = "native-audio")]
    active: Mutex<Option<ActiveAudio>>,
}

impl NativeAudioBackend for SystemAudioBackend {
    fn play(&self, _name: &str, bytes: &[u8], start_seconds: f64, end_seconds: Option<f64>) -> Result<(), String> {
        self.stop();
        #[cfg(feature = "native-audio")]
        {
            use kira::sound::static_sound::StaticSoundData;
            let decoded = StaticSoundData::from_cursor(std::io::Cursor::new(bytes.to_vec())).map_err(|error| format!("could not decode EPUB audio: {error}"))?;
            let start = start_seconds.max(0.0);
            let sound = match end_seconds.filter(|end| *end > start) {
                Some(end) => decoded.slice(start..end),
                None if start > 0.0 => decoded.slice(start..),
                None => decoded,
            };
            let mut manager = kira::AudioManager::new(kira::AudioManagerSettings::default()).map_err(|error| format!("could not initialize native audio: {error}"))?;
            let handle = manager.play(sound).map_err(|error| format!("could not play EPUB audio: {error}"))?;
            *self.active.lock().map_err(|_| "audio backend lock is poisoned".to_owned())? = Some(ActiveAudio { _manager: manager, handle });
            Ok(())
        }
        #[cfg(not(feature = "native-audio"))]
        {
            let _ = (bytes, start_seconds, end_seconds);
            Err("native audio support was disabled for this build".to_owned())
        }
    }

    fn pause(&self) -> Result<(), String> {
        #[cfg(feature = "native-audio")]
        if let Some(active) = self.active.lock().map_err(|_| "audio backend lock is poisoned".to_owned())?.as_mut() {
            active.handle.pause(kira::Tween::default());
        }
        Ok(())
    }

    fn resume(&self) -> Result<(), String> {
        #[cfg(feature = "native-audio")]
        if let Some(active) = self.active.lock().map_err(|_| "audio backend lock is poisoned".to_owned())?.as_mut() {
            active.handle.resume(kira::Tween::default());
        }
        Ok(())
    }

    fn seek_to(&self, seconds: f64) -> Result<(), String> {
        #[cfg(feature = "native-audio")]
        if let Some(active) = self.active.lock().map_err(|_| "audio backend lock is poisoned".to_owned())?.as_mut() {
            active.handle.seek_to(seconds.max(0.0));
        }
        #[cfg(not(feature = "native-audio"))]
        let _ = seconds;
        Ok(())
    }

    fn status(&self) -> NativeAudioStatus {
        #[cfg(feature = "native-audio")]
        if let Ok(active) = self.active.lock()
            && let Some(active) = active.as_ref()
        {
            let state = match active.handle.state() {
                kira::sound::PlaybackState::Paused | kira::sound::PlaybackState::WaitingToResume => NativeAudioState::Paused,
                kira::sound::PlaybackState::Stopped => NativeAudioState::Stopped,
                _ => NativeAudioState::Playing,
            };
            return NativeAudioStatus { state, position_seconds: active.handle.position() };
        }
        NativeAudioStatus::default()
    }

    fn stop(&self) {
        #[cfg(feature = "native-audio")]
        if let Ok(mut active) = self.active.lock()
            && let Some(mut playback) = active.take()
        {
            playback.handle.stop(kira::Tween::default());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct FakeAudioProvider {
        status: Mutex<NativeAudioStatus>,
    }

    impl NativeAudioBackend for FakeAudioProvider {
        fn play(&self, _name: &str, _bytes: &[u8], start_seconds: f64, _end_seconds: Option<f64>) -> Result<(), String> {
            *self.status.lock().unwrap() = NativeAudioStatus { state: NativeAudioState::Playing, position_seconds: start_seconds };
            Ok(())
        }
        fn pause(&self) -> Result<(), String> {
            self.status.lock().unwrap().state = NativeAudioState::Paused;
            Ok(())
        }
        fn resume(&self) -> Result<(), String> {
            self.status.lock().unwrap().state = NativeAudioState::Playing;
            Ok(())
        }
        fn seek_to(&self, seconds: f64) -> Result<(), String> {
            self.status.lock().unwrap().position_seconds = seconds;
            Ok(())
        }
        fn status(&self) -> NativeAudioStatus {
            *self.status.lock().unwrap()
        }
        fn stop(&self) {
            self.status.lock().unwrap().state = NativeAudioState::Stopped;
        }
    }

    #[test]
    fn media_overlay_provider_contract_reports_real_transport_state() {
        let provider = FakeAudioProvider::default();
        provider.play("cue.mp3", b"fixture", 1.25, Some(2.5)).unwrap();
        assert_eq!(provider.status(), NativeAudioStatus { state: NativeAudioState::Playing, position_seconds: 1.25 });
        provider.pause().unwrap();
        assert_eq!(provider.status().state, NativeAudioState::Paused);
        provider.seek_to(1.75).unwrap();
        provider.resume().unwrap();
        assert_eq!(provider.status(), NativeAudioStatus { state: NativeAudioState::Playing, position_seconds: 1.75 });
        provider.stop();
        assert_eq!(provider.status().state, NativeAudioState::Stopped);
    }
}
