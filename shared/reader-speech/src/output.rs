//! Pausable device output; retaining the stream also retains queued PCM and
//! the resampler's position. The callback gate covers backends without hardware pause.
use rodio::{
    Player,
    cpal::{
        self, BufferSize, Device, Sample, SampleFormat, SizedSample, Stream, SupportedBufferSize, SupportedStreamConfig,
        traits::{DeviceTrait, HostTrait, StreamTrait},
    },
    mixer::{MixerSource, mixer},
    stream::supported_output_configs,
};
use std::num::{NonZeroU16, NonZeroU32};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

pub(super) trait OutputControl: Send + Sync {
    fn pause(&self);
    fn resume(&self) -> Result<(), String>;
}

#[derive(Default)]
struct GateState {
    paused: bool,
    closed: bool,
}

struct CallbackGate {
    state: Mutex<GateState>,
    changed: Condvar,
    park_on_pause: AtomicBool,
}

impl CallbackGate {
    fn new(park_on_pause: bool) -> Self {
        Self { state: Mutex::new(GateState { paused: true, closed: false }), changed: Condvar::new(), park_on_pause: AtomicBool::new(park_on_pause) }
    }
    fn set_paused(&self, paused: bool) -> bool {
        let mut state = self.state.lock().unwrap();
        if state.paused == paused {
            return false;
        }
        state.paused = paused;
        if !paused {
            self.changed.notify_all();
        }
        true
    }
    fn wait(&self) -> bool {
        let mut state = self.state.lock().unwrap();
        while state.paused && !state.closed && self.park_on_pause.load(Ordering::Acquire) {
            state = self.changed.wait(state).unwrap();
        }
        !state.closed
    }
    fn close(&self) {
        self.state.lock().unwrap().closed = true;
        self.changed.notify_all();
    }
}

pub(super) struct DeviceOutput {
    stream: Stream,
    gate: Arc<CallbackGate>,
}

impl OutputControl for DeviceOutput {
    fn pause(&self) {
        if self.gate.set_paused(true) {
            // Some drivers cannot pause in hardware (and ALSA may return Ok
            // anyway). Their callback parks on the gate instead of generating silence.
            if self.stream.pause().is_err() {
                self.gate.park_on_pause.store(true, Ordering::Release);
            }
        }
    }
    fn resume(&self) -> Result<(), String> {
        if self.gate.set_paused(false) {
            if let Err(error) = self.stream.play() {
                self.gate.set_paused(true);
                return Err(error.to_string());
            }
        }
        Ok(())
    }
}

impl Drop for DeviceOutput {
    fn drop(&mut self) {
        self.gate.close();
    }
}

pub(super) fn open_default() -> Result<(Arc<Player>, DeviceOutput, Duration), String> {
    let host = cpal::default_host();
    let mut first_error = None;
    if let Some(device) = host.default_output_device() {
        match open_device(&device) {
            Ok(output) => return Ok(output),
            Err(error) => first_error = Some(error),
        }
    }
    let devices = host.output_devices().map_err(|error| error.to_string())?;
    for device in devices {
        if device.description().ok().is_some_and(|description| description.driver() == Some("null")) {
            continue;
        }
        match open_device(&device) {
            Ok(output) => return Ok(output),
            Err(error) => {
                first_error.get_or_insert(error);
            }
        }
    }
    Err(first_error.unwrap_or_else(|| "No audio output device is available".into()))
}

fn open_device(device: &Device) -> Result<(Arc<Player>, DeviceOutput, Duration), String> {
    let first_error = match device.default_output_config() {
        Ok(config) => match open_config(device, &config, true) {
            Ok(output) => return Ok(output),
            Err(error) => error,
        },
        Err(error) => error.to_string(),
    };
    for config in supported_output_configs(device).map_err(|error| error.to_string())? {
        if let Ok(output) = open_config(device, &config, false) {
            return Ok(output);
        }
    }
    Err(first_error)
}

fn open_config(device: &Device, supported: &SupportedStreamConfig, fixed_buffer: bool) -> Result<(Arc<Player>, DeviceOutput, Duration), String> {
    let mut config = supported.config();
    if fixed_buffer {
        let frames = (config.sample_rate / 20).max(1);
        let frames = match supported.buffer_size() {
            SupportedBufferSize::Range { min, max } => frames.clamp(*min, *max),
            SupportedBufferSize::Unknown => frames,
        };
        config.buffer_size = BufferSize::Fixed(frames);
    }
    let channels = NonZeroU16::new(config.channels).ok_or("Invalid output channel count")?;
    let rate = NonZeroU32::new(config.sample_rate).ok_or("Invalid output sample rate")?;
    let (controller, samples) = mixer(channels, rate);
    // ALSA reports success even for drivers without hardware pause. On other
    // backends leave callbacks unblocked so the native pause transition can finish.
    let gate = Arc::new(CallbackGate::new(cfg!(target_os = "linux")));
    macro_rules! build {
        ($($format:ident, $sample:ty);+ $(;)?) => {
            match supported.sample_format() {
                $(SampleFormat::$format => build_stream::<$sample>(device, &config, samples, gate.clone()),)+
                _ => return Err("Unsupported audio output format".into()),
            }
        };
    }
    let stream = build!(F32, f32; F64, f64; I8, i8; I16, i16; I24, cpal::I24; I32, i32; I64, i64; U8, u8; U16, u16; U24, cpal::U24; U32, u32; U64, u64)?;
    let drain = match config.buffer_size {
        BufferSize::Fixed(frames) => Duration::from_secs_f64(2.0 * f64::from(frames) / f64::from(config.sample_rate)),
        BufferSize::Default => Duration::from_millis(200),
    } + Duration::from_millis(50);
    Ok((Arc::new(Player::connect_new(&controller)), DeviceOutput { stream, gate }, drain))
}

fn build_stream<T: SizedSample + Sample + cpal::FromSample<f32>>(device: &Device, config: &cpal::StreamConfig, mut samples: MixerSource, gate: Arc<CallbackGate>) -> Result<Stream, String> {
    device
        .build_output_stream::<T, _, _>(
            config,
            move |data, _| {
                if gate.wait() {
                    for sample in data {
                        *sample = T::from_sample(samples.next().unwrap_or(0.0));
                    }
                } else {
                    data.fill(T::EQUILIBRIUM);
                }
            },
            |error| eprintln!("Speech output error: {error}"),
            None,
        )
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::mpsc, thread};

    #[test]
    fn paused_callback_parks_until_resume_and_close_unblocks_shutdown() {
        let gate = Arc::new(CallbackGate::new(true));
        let (entered, entering) = mpsc::channel();
        let (ready, received) = mpsc::channel();
        let callback_gate = gate.clone();
        let callback = thread::spawn(move || {
            entered.send(()).unwrap();
            ready.send(callback_gate.wait()).unwrap();
        });
        entering.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(received.recv_timeout(Duration::from_millis(50)).is_err());
        gate.set_paused(false);
        assert!(received.recv_timeout(Duration::from_secs(3)).unwrap());
        callback.join().unwrap();
        gate.set_paused(true);
        let callback_gate = gate.clone();
        let callback = thread::spawn(move || callback_gate.wait());
        gate.close();
        assert!(!callback.join().unwrap());
    }

    #[test]
    fn hardware_pause_allows_an_in_flight_callback_to_finish_its_transition() {
        let gate = CallbackGate::new(false);
        assert!(gate.wait(), "a native pause must not wait for a callback parked by us");
        gate.close();
        assert!(!gate.wait());
    }
}
