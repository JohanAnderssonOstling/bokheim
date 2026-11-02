//! Apple's installed on-device voices, rendered to buffers for our existing audio queue.
use super::{Audio, DeviceVoice, pcm::MAX_AUDIO_BYTES};
use block2::RcBlock;
use objc2::rc::autoreleasepool;
use objc2_avf_audio::{AVAudioBuffer, AVAudioPCMBuffer, AVSpeechBoundary, AVSpeechSynthesisVoice, AVSpeechSynthesizer, AVSpeechUtterance, AVSpeechUtteranceDefaultSpeechRate};
use objc2_foundation::{NSDate, NSRunLoop, NSString};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::NonNull;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub(super) fn voices() -> Result<Vec<DeviceVoice>, String> {
    autoreleasepool(|_| unsafe {
        // Custom speech providers have no offline metadata. Only expose Apple's local engines.
        Ok(AVSpeechSynthesisVoice::speechVoices()
            .iter()
            .filter_map(|voice| {
                let id = voice.identifier().to_string();
                id.starts_with("com.apple.").then(|| DeviceVoice { id, name: voice.name().to_string(), language: voice.language().to_string() })
            })
            .collect())
    })
}

#[derive(Default)]
struct Capture {
    samples: Vec<f32>,
    rate: u32,
    done: bool,
    error: Option<String>,
}
impl Capture {
    unsafe fn append(&mut self, buffer: &AVAudioPCMBuffer) -> Result<(), String> {
        let frames = unsafe { buffer.frameLength() } as usize;
        if frames == 0 {
            self.done = true;
            return Ok(());
        }
        let format = unsafe { buffer.format() };
        let channels = unsafe { format.channelCount() } as usize;
        let rate = unsafe { format.sampleRate() };
        if !rate.is_finite() || !(8000.0..=192000.0).contains(&rate) || channels == 0 || channels > 8 || self.samples.len().saturating_add(frames) > MAX_AUDIO_BYTES / 4 {
            return Err("The installed local voice returned invalid or oversized audio.".into());
        }
        if self.rate != 0 && self.rate != rate as u32 {
            return Err("The local voice changed audio format during synthesis.".into());
        }
        self.rate = rate as u32;
        let interleaved = unsafe { format.isInterleaved() };
        let common = unsafe { format.commonFormat() }.0;
        for frame in 0..frames {
            let mut sum = 0.0;
            for channel in 0..channels {
                let plane = if interleaved { 0 } else { channel };
                let index = if interleaved { frame * channels + channel } else { frame };
                let value = unsafe {
                    match common {
                        1 => {
                            let data = buffer.floatChannelData();
                            if data.is_null() {
                                return Err("Missing local voice audio data.".into());
                            }
                            *(*data.add(plane)).as_ptr().add(index)
                        }
                        3 => {
                            let data = buffer.int16ChannelData();
                            if data.is_null() {
                                return Err("Missing local voice audio data.".into());
                            }
                            f32::from(*(*data.add(plane)).as_ptr().add(index)) / 32768.0
                        }
                        4 => {
                            let data = buffer.int32ChannelData();
                            if data.is_null() {
                                return Err("Missing local voice audio data.".into());
                            }
                            *(*data.add(plane)).as_ptr().add(index) as f32 / 2147483648.0
                        }
                        _ => return Err("The installed voice returned an unsupported audio format.".into()),
                    }
                };
                if !value.is_finite() {
                    return Err("The local voice returned invalid audio samples.".into());
                }
                sum += value.clamp(-1.0, 1.0) / channels as f32;
            }
            self.samples.push(sum);
        }
        Ok(())
    }
}

pub(super) fn synthesize(id: &str, text: &str, rate: f32, cancelled: impl Fn() -> bool, mut emit: impl FnMut(Audio) -> bool) -> Result<(), String> {
    autoreleasepool(|_| unsafe {
        if !voices()?.iter().any(|voice| voice.id == id) {
            return Err("The selected installed local voice is unavailable. Choose another local voice.".into());
        }
        let voice = AVSpeechSynthesisVoice::voiceWithIdentifier(&NSString::from_str(id)).ok_or("The selected local voice is no longer installed.")?;
        let utterance = AVSpeechUtterance::speechUtteranceWithString(&NSString::from_str(text));
        utterance.setVoice(Some(&voice));
        utterance.setRate((AVSpeechUtteranceDefaultSpeechRate * rate).clamp(0.0, 1.0));
        let synth = AVSpeechSynthesizer::new();
        let capture = Arc::new(Mutex::new(Capture::default()));
        let output = capture.clone();
        let callback = RcBlock::new(move |buffer: NonNull<AVAudioBuffer>| {
            let mut capture = output.lock().unwrap_or_else(|error| error.into_inner());
            if capture.done || capture.error.is_some() {
                return;
            }
            let result = catch_unwind(AssertUnwindSafe(|| buffer.as_ref().downcast_ref::<AVAudioPCMBuffer>().ok_or_else(|| "The local voice returned unsupported audio buffers.".to_owned()).and_then(|buffer| capture.append(buffer))))
                .unwrap_or_else(|_| Err("The local voice audio callback was interrupted.".into()));
            if let Err(error) = result {
                capture.error = Some(error);
            }
        });
        synth.writeUtterance_toBufferCallback(&utterance, RcBlock::as_ptr(&callback));
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            if cancelled() || Instant::now() >= deadline {
                synth.stopSpeakingAtBoundary(AVSpeechBoundary::Immediate);
                return Err(if cancelled() { "Cancelled" } else { "The local voice took too long. Try another installed voice." }.into());
            }
            {
                let capture = capture.lock().map_err(|_| "The local voice audio callback was interrupted.")?;
                if let Some(error) = &capture.error {
                    synth.stopSpeakingAtBoundary(AVSpeechBoundary::Immediate);
                    return Err(error.clone());
                }
                if capture.done {
                    break;
                }
            }
            NSRunLoop::currentRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.01));
        }
        let mut capture = capture.lock().map_err(|_| "The local voice audio callback was interrupted.")?;
        if capture.samples.is_empty() {
            return Err("The selected local voice returned no audio. Check that its voice data is installed.".into());
        }
        if emit(Audio { samples: std::mem::take(&mut capture.samples), sample_rate: capture.rate, last: true }) { Ok(()) } else { Err("Cancelled".into()) }
    })
}
