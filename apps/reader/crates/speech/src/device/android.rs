//! Android synthesizes installed offline voices; Rust retains ownership of playback.
use crate::SpeechError;
use super::{Audio, DeviceVoice, pcm};
use serde_json::json;
use std::collections::HashMap;
use std::sync::{
    Arc, Condvar, Mutex, OnceLock,
    atomic::{AtomicU64, Ordering},
};
use std::time::{Duration, Instant};

type Bridge = dyn Fn(&str) -> Result<String, SpeechError> + Send + Sync;
static BRIDGE: OnceLock<Box<Bridge>> = OnceLock::new();
static JOBS: OnceLock<Mutex<HashMap<u64, Arc<Job>>>> = OnceLock::new();
static NEXT: AtomicU64 = AtomicU64::new(1);
#[derive(Default)]
struct State {
    bytes: Vec<u8>,
    format: u16,
    channels: u16,
    rate: u32,
    done: bool,
    error: Option<SpeechError>,
}
#[derive(Default)]
struct Job {
    state: Mutex<State>,
    changed: Condvar,
}
fn jobs() -> &'static Mutex<HashMap<u64, Arc<Job>>> {
    JOBS.get_or_init(Default::default)
}
fn job(id: u64) -> Option<Arc<Job>> {
    jobs().lock().ok()?.get(&id).cloned()
}
fn dispatch(value: serde_json::Value) -> Result<String, SpeechError> {
    let response = BRIDGE.get().ok_or("Android local voice integration is unavailable.")?(&value.to_string())?;
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&response) {
        if let Some(error) = value.get("error").and_then(|value| value.as_str()) {
            return Err(serde_json::from_value(json!({"message":error,"failure":value.get("failure")})).unwrap_or_else(|_| SpeechError::from(error)));
        }
    }
    Ok(response)
}
pub fn initialize(bridge: impl Fn(&str) -> Result<String, SpeechError> + Send + Sync + 'static) {
    let _ = BRIDGE.set(Box::new(bridge));
}

pub(super) fn voices() -> Result<Vec<DeviceVoice>, SpeechError> {
    let encoded = dispatch(json!({"command":"voices"}))?;
    let value: serde_json::Value = serde_json::from_str(&encoded).map_err(|_| "Android returned an invalid local voice catalogue.")?;
    if let Some(error) = value.get("error").and_then(|value| value.as_str()) {
        return Err(serde_json::from_value(json!({"message":error,"failure":value.get("failure")})).unwrap_or_else(|_| SpeechError::from(error)));
    }
    serde_json::from_value(value).map_err(|_| "Could not read installed local Android voices.".into())
}

/// Platform callbacks have opaque request identities. Late/cancelled callbacks are ignored.
pub fn started(id: u64, rate: u32, encoding: i32, channels: i32) -> bool {
    let Some(job) = job(id) else {
        return false;
    };
    let Ok(mut state) = job.state.lock() else {
        return false;
    };
    let format = match encoding {
        2 => 16,
        3 => 8,
        4 => 32,
        _ => 0,
    };
    if format == 0 || !(1..=8).contains(&channels) || !(8000..=192000).contains(&rate) || state.done || state.rate != 0 {
        state.error = Some("The local voice returned an unsupported audio format.".into());
        state.done = true;
        job.changed.notify_all();
        return false;
    }
    state.rate = rate;
    state.format = format;
    state.channels = channels as u16;
    true
}
pub fn audio(id: u64, bytes: &[u8]) -> bool {
    let Some(job) = job(id) else {
        return false;
    };
    let Ok(mut state) = job.state.lock() else {
        return false;
    };
    if state.done {
        return false;
    }
    if state.rate == 0 || state.bytes.len().saturating_add(bytes.len()) > pcm::MAX_AUDIO_BYTES {
        state.error = Some("The local voice returned invalid or oversized audio.".into());
        state.done = true;
        job.changed.notify_all();
        return false;
    }
    state.bytes.extend_from_slice(bytes);
    true
}
pub fn complete(id: u64, error: Option<SpeechError>) {
    let Some(job) = job(id) else {
        return;
    };
    if let Ok(mut state) = job.state.lock() {
        if state.done {
            return;
        }
        state.done = true;
        state.error = error;
        job.changed.notify_all();
    }
}
struct Pending(u64);
impl Drop for Pending {
    fn drop(&mut self) {
        if let Ok(mut jobs) = jobs().lock() {
            jobs.remove(&self.0);
        }
        let _ = dispatch(json!({"command":"cancel", "id":self.0}));
    }
}

pub(super) fn synthesize(id: &str, text: &str, rate: f32, cancelled: impl Fn() -> bool, mut emit: impl FnMut(Audio) -> bool) -> Result<(), SpeechError> {
    let request = NEXT.fetch_add(1, Ordering::Relaxed);
    let job = Arc::new(Job::default());
    jobs().lock().map_err(|_| "Local voice requests were interrupted.")?.insert(request, job.clone());
    let _pending = Pending(request);
    dispatch(json!({"command":"synthesize", "id":request, "voice":id, "text":text, "rate":rate}))?;
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut state = job.state.lock().map_err(|_| "Local voice audio was interrupted.")?;
    loop {
        if cancelled() {
            return Err("Cancelled".into());
        }
        if state.done {
            break;
        }
        if Instant::now() >= deadline {
            return Err("The installed local voice took too long. Try another installed voice.".into());
        }
        state = job.changed.wait_timeout(state, Duration::from_millis(20)).map_err(|_| "Local voice synthesis was interrupted.")?.0;
    }
    if let Some(error) = state.error.take() {
        return Err(error);
    }
    if state.bytes.is_empty() {
        return Err("The installed local voice returned no audio. Check that its voice data is installed.".into());
    }
    let audio = pcm::decode(&state.bytes, state.format, state.channels, state.rate)?;
    if emit(audio) { Ok(()) } else { Err("Cancelled".into()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelled_and_completed_requests_ignore_late_platform_callbacks() {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let active = Arc::new(Job::default());
        jobs().lock().unwrap().insert(id, active.clone());
        assert!(started(id, 22050, 2, 1));
        assert!(audio(id, &[0, 64]));
        complete(id, None);
        assert!(!audio(id, &[0, 0]));
        complete(id, Some("late error".into()));
        assert!(active.state.lock().unwrap().error.is_none());
        jobs().lock().unwrap().remove(&id);
        assert!(!started(id, 22050, 2, 1));
        assert!(!audio(id, &[0, 0]));
        complete(id, None);
    }

    #[test]
    fn audio_before_format_and_format_changes_fail_without_queueing_audio() {
        for malformed in [false, true] {
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            let active = Arc::new(Job::default());
            jobs().lock().unwrap().insert(id, active.clone());
            if malformed {
                assert!(started(id, 22050, 2, 1));
                assert!(!started(id, 22050, 4, 1));
            } else {
                assert!(!audio(id, &[0, 0]));
            }
            complete(id, None);
            let state = active.state.lock().unwrap();
            assert!(state.done && state.error.is_some() && state.bytes.is_empty());
            jobs().lock().unwrap().remove(&id);
        }
    }
}
