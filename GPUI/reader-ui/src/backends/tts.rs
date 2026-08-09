//! Read-aloud support, backed by the platform speech synthesizer when present.

pub trait TtsProvider: Send + Sync {
    fn is_available(&self) -> bool;
    fn speak(&self, text: &str) -> Result<(), String>;
    fn status(&self) -> TtsState;
    fn stop(&self);
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TtsState {
    Speaking,
    #[default]
    Idle,
    Unavailable,
}

#[cfg(feature = "system-tts")]
enum SpeechCommand {
    Speak(String, std::sync::mpsc::Sender<Result<(), String>>),
    Status(std::sync::mpsc::Sender<TtsState>),
    Stop,
}

#[derive(Default)]
pub struct SystemTtsProvider {
    #[cfg(feature = "system-tts")]
    commands: std::sync::mpsc::Sender<SpeechCommand>,
}

impl TtsProvider for SystemTtsProvider {
    fn is_available(&self) -> bool {
        cfg!(feature = "system-tts")
    }

    fn speak(&self, text: &str) -> Result<(), String> {
        #[cfg(not(feature = "system-tts"))]
        {
            let _ = text;
            Err("system TTS support was disabled for this build".to_owned())
        }
        #[cfg(feature = "system-tts")]
        {
            let (reply, result) = std::sync::mpsc::channel();
            self.commands.send(SpeechCommand::Speak(text.to_owned(), reply)).map_err(|_| "system TTS worker stopped".to_owned())?;
            result.recv().map_err(|_| "system TTS worker stopped".to_owned())?
        }
    }

    fn status(&self) -> TtsState {
        #[cfg(not(feature = "system-tts"))]
        {
            TtsState::Unavailable
        }
        #[cfg(feature = "system-tts")]
        {
            let (reply, result) = std::sync::mpsc::channel();
            if self.commands.send(SpeechCommand::Status(reply)).is_err() {
                return TtsState::Unavailable;
            }
            result.recv().unwrap_or(TtsState::Unavailable)
        }
    }

    fn stop(&self) {
        #[cfg(feature = "system-tts")]
        let _ = self.commands.send(SpeechCommand::Stop);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[derive(Default)]
    struct FakeTtsProvider {
        speaking: AtomicBool,
    }

    impl TtsProvider for FakeTtsProvider {
        fn is_available(&self) -> bool {
            true
        }
        fn speak(&self, text: &str) -> Result<(), String> {
            if text.trim().is_empty() {
                return Err("empty speech".to_owned());
            }
            self.speaking.store(true, Ordering::Release);
            Ok(())
        }
        fn status(&self) -> TtsState {
            if self.speaking.load(Ordering::Acquire) { TtsState::Speaking } else { TtsState::Idle }
        }
        fn stop(&self) {
            self.speaking.store(false, Ordering::Release);
        }
    }

    #[test]
    fn tts_provider_contract_supports_start_and_stop_without_platform_audio() {
        let provider = FakeTtsProvider::default();
        assert!(provider.is_available());
        provider.speak("A sentence.").expect("fake accepts speech");
        assert!(provider.speaking.load(Ordering::Acquire));
        assert_eq!(provider.status(), TtsState::Speaking);
        provider.stop();
        assert!(!provider.speaking.load(Ordering::Acquire));
        assert_eq!(provider.status(), TtsState::Idle);
    }

    #[cfg(not(feature = "system-tts"))]
    #[test]
    fn low_memory_system_tts_provider_is_inert() {
        let provider = SystemTtsProvider::default();
        assert!(!provider.is_available());
        assert_eq!(provider.status(), TtsState::Unavailable);
        assert!(provider.speak("text").is_err());
    }
}
