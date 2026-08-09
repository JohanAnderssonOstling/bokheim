use std::io::{BufReader, SeekFrom};
use std::num::{NonZeroU16, NonZeroU32};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crossbeam_channel::{Receiver, SendTimeoutError, Sender, TryRecvError, bounded, unbounded};
use rodio::source::Source;
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player};
use wsola::TimeStretch;

use crate::AudiobookReadSeek;
use crate::playback_controller::PlaybackBackend;

type AudioDecoder = Decoder<BufReader<Box<dyn AudiobookReadSeek>>>;

const BLOCK_SAMPLES: usize = 16_384;
const BUFFERED_BLOCKS: usize = 12;
const EOF_CONFIRMATION_READS: usize = 8;
const NO_GENERATION: u64 = u64::MAX;

enum Command {
    Seek { position: Duration, generation: u64 },
    Tempo(f32),
    Stop,
}

#[derive(Default)]
struct PendingCommands {
    seek: Option<(Duration, u64)>,
    tempo: Option<f32>,
    stop: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CommandOutcome {
    Continue,
    Seeked,
    Stop,
}

struct AudioChunk {
    generation: u64,
    tempo: f32,
    samples: Vec<f32>,
}

struct SharedState {
    generation: AtomicU64,
    position: Mutex<PositionState>,
    decoder_end_generation: AtomicU64,
    playback_finished_generation: AtomicU64,
    error: Mutex<Option<String>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PositionState {
    generation: u64,
    micros: u64,
}

impl SharedState {
    fn position_micros(&self) -> u64 {
        self.position.lock().expect("audio position lock poisoned").micros
    }

    fn set_position_if_generation(&self, generation: u64, micros: u64) -> bool {
        let mut position = self.position.lock().expect("audio position lock poisoned");
        if position.generation != generation {
            return false;
        }
        position.micros = micros;
        true
    }

    fn playback_finished(&self) -> bool {
        self.playback_finished_generation.load(Ordering::Acquire) == self.generation.load(Ordering::Acquire)
    }
}

pub struct AudioEngine {
    _output: MixerDeviceSink,
    player: Player,
    command_tx: Sender<Command>,
    worker: Option<JoinHandle<()>>,
    state: Arc<SharedState>,
    duration: Duration,
}

impl AudioEngine {
    pub fn open(reader: Box<dyn AudiobookReadSeek>) -> Result<Self, String> {
        let decoder = open_decoder(reader)?;
        let channels = decoder.channels();
        let sample_rate = decoder.sample_rate();
        let duration = decoder.total_duration().unwrap_or_default();

        let output = DeviceSinkBuilder::open_default_sink().map_err(|error| error.to_string())?;
        let player = Player::connect_new(output.mixer());
        // UI seeks must never block behind decoder work. The worker drains all
        // pending commands before decoding its next block, so bursts naturally
        // converge on the most recent seek.
        let (command_tx, command_rx) = unbounded();
        let (audio_tx, audio_rx) = bounded(BUFFERED_BLOCKS);
        let state = Arc::new(SharedState {
            generation: AtomicU64::new(0),
            position: Mutex::new(PositionState { generation: 0, micros: 0 }),
            decoder_end_generation: AtomicU64::new(NO_GENERATION),
            playback_finished_generation: AtomicU64::new(NO_GENERATION),
            error: Mutex::new(None),
        });
        let worker_state = state.clone();
        let decoder_state = state.clone();
        let worker = thread::Builder::new()
            .name("m4b-decode-wsola".into())
            .spawn(move || {
                if let Err(error) = decode_worker(decoder, channels, sample_rate, command_rx, audio_tx, decoder_state) {
                    *worker_state.error.lock().expect("audio error lock poisoned") = Some(error);
                }
            })
            .map_err(|error| error.to_string())?;

        player.append(WorkerSource::new(audio_rx, state.clone(), channels, sample_rate, duration));

        Ok(Self { _output: output, player, command_tx, worker: Some(worker), state, duration })
    }

    pub fn position(&self) -> Duration {
        Duration::from_micros(self.state.position_micros())
    }

    pub fn is_playing(&self) -> bool {
        !self.player.is_paused() && !self.state.playback_finished() && !self.player.empty()
    }

    pub fn toggle(&self) -> Result<(), String> {
        if self.state.playback_finished() {
            self.seek_to(Duration::ZERO)?;
            self.player.play();
            return Ok(());
        }
        if self.player.is_paused() {
            self.player.play();
        } else {
            self.player.pause();
        }
        Ok(())
    }

    pub fn seek_to(&self, position: Duration) -> Result<(), String> {
        let position = position.min(self.duration);
        request_seek(&self.state, &self.command_tx, position)
    }

    pub fn set_tempo(&mut self, tempo: f32) -> Result<(), String> {
        let tempo = normalized_tempo(tempo)?;
        self.command_tx.send(Command::Tempo(tempo)).map_err(|_| "audio decoder stopped".to_string())
    }

    pub fn take_error(&self) -> Option<String> {
        self.state.error.lock().ok()?.take()
    }
}

fn open_decoder(mut reader: Box<dyn AudiobookReadSeek>) -> Result<AudioDecoder, String> {
    let byte_len = stream_byte_len(&mut *reader)?;
    Decoder::builder().with_data(BufReader::new(reader)).with_byte_len(byte_len).with_seekable(true).build().map_err(|error| error.to_string())
}

fn stream_byte_len(reader: &mut dyn AudiobookReadSeek) -> Result<u64, String> {
    let original_position = reader.stream_position().map_err(|error| error.to_string())?;
    let byte_len = reader.seek(SeekFrom::End(0)).map_err(|error| error.to_string())?;
    reader.seek(SeekFrom::Start(original_position)).map_err(|error| error.to_string())?;
    Ok(byte_len)
}

fn request_seek(state: &SharedState, command_tx: &Sender<Command>, position: Duration) -> Result<(), String> {
    let mut current_position = state.position.lock().expect("audio position lock poisoned");
    let previous_generation = state.generation.load(Ordering::Acquire);
    let generation = previous_generation.checked_add(1).filter(|generation| *generation != NO_GENERATION).ok_or_else(|| "audio seek generation exhausted".to_owned())?;
    let previous_position = *current_position;
    state.generation.store(generation, Ordering::Release);
    *current_position = PositionState { generation, micros: position.as_micros() as u64 };
    drop(current_position);
    if command_tx.send(Command::Seek { position, generation }).is_ok() {
        return Ok(());
    }

    let mut current = state.position.lock().expect("audio position lock poisoned");
    if state.generation.load(Ordering::Acquire) == generation {
        state.generation.store(previous_generation, Ordering::Release);
        if current.generation == generation {
            *current = previous_position;
        }
    }
    Err("audio decoder stopped".to_string())
}

fn normalized_tempo(tempo: f32) -> Result<f32, String> {
    if !tempo.is_finite() {
        return Err("playback rate must be finite".to_owned());
    }
    Ok(tempo.clamp(wsola::MIN_TEMPO, wsola::MAX_TEMPO))
}

fn collect_pending_commands(first: Option<Command>, command_rx: &Receiver<Command>) -> PendingCommands {
    let mut pending = PendingCommands::default();
    let mut collect = |command| match command {
        Command::Seek { position, generation } => pending.seek = Some((position, generation)),
        Command::Tempo(tempo) => pending.tempo = Some(tempo),
        Command::Stop => pending.stop = true,
    };
    if let Some(first) = first {
        collect(first);
    }
    while let Ok(command) = command_rx.try_recv() {
        collect(command);
    }
    pending
}

fn apply_pending_commands(pending: PendingCommands, decoder: &mut AudioDecoder, stretch: &mut TimeStretch, tempo: &mut f32, generation: &mut u64, state: &SharedState) -> Result<CommandOutcome, String> {
    if pending.stop {
        return Ok(CommandOutcome::Stop);
    }
    if let Some(next) = pending.tempo {
        apply_command(Command::Tempo(next), decoder, stretch, tempo, generation, state)?;
    }
    if let Some((position, next)) = pending.seek {
        apply_command(Command::Seek { position, generation: next }, decoder, stretch, tempo, generation, state)?;
        return Ok(CommandOutcome::Seeked);
    }
    Ok(CommandOutcome::Continue)
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        let _ = self.command_tx.send(Command::Stop);
        self.player.stop();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl PlaybackBackend for AudioEngine {
    fn position(&self) -> Duration {
        AudioEngine::position(self)
    }

    fn is_playing(&self) -> bool {
        AudioEngine::is_playing(self)
    }

    fn toggle(&self) -> Result<(), String> {
        AudioEngine::toggle(self)
    }

    fn seek_to(&self, position: Duration) -> Result<(), String> {
        AudioEngine::seek_to(self, position)
    }

    fn set_tempo(&mut self, tempo: f32) -> Result<(), String> {
        AudioEngine::set_tempo(self, tempo)
    }

    fn take_error(&self) -> Option<String> {
        AudioEngine::take_error(self)
    }
}

fn apply_command(command: Command, decoder: &mut AudioDecoder, stretch: &mut TimeStretch, tempo: &mut f32, generation: &mut u64, state: &SharedState) -> Result<bool, String> {
    match command {
        Command::Tempo(next) => {
            *tempo = next;
            stretch.set_tempo(next);
            Ok(true)
        }
        Command::Seek { position, generation: next } => {
            decoder.try_seek(position).map_err(|error| error.to_string())?;
            stretch.reset();
            *generation = next;
            state.decoder_end_generation.store(NO_GENERATION, Ordering::Release);
            Ok(true)
        }
        Command::Stop => Ok(false),
    }
}

fn read_decoder_block(decoder: &mut impl Iterator<Item = f32>, block_samples: usize) -> Vec<f32> {
    for _ in 0..EOF_CONFIRMATION_READS {
        let samples = decoder.by_ref().take(block_samples).collect::<Vec<_>>();
        if !samples.is_empty() {
            return samples;
        }
    }
    Vec::new()
}

fn decode_worker(mut decoder: AudioDecoder, channels: NonZeroU16, sample_rate: NonZeroU32, command_rx: Receiver<Command>, audio_tx: Sender<AudioChunk>, state: Arc<SharedState>) -> Result<(), String> {
    let mut stretch = TimeStretch::new(sample_rate.get(), channels.get()).map_err(|error| error.to_string())?;
    let mut tempo = 1.0;
    let mut generation = 0;
    let aligned_block = BLOCK_SAMPLES / channels.get() as usize * channels.get() as usize;

    'decode: loop {
        let pending = collect_pending_commands(None, &command_rx);
        if apply_pending_commands(pending, &mut decoder, &mut stretch, &mut tempo, &mut generation, &state)? == CommandOutcome::Stop {
            break;
        }

        // Symphonia's iterator uses `None` for both real EOF and some packet or
        // demuxer errors from which a later call can recover. A single empty
        // read therefore must not send playback to the end of the book.
        let input = read_decoder_block(&mut decoder, aligned_block);
        let at_end = input.is_empty();
        let samples = if at_end {
            stretch.flush()
        } else {
            let aligned = input.len() / channels.get() as usize * channels.get() as usize;
            stretch.push(&input[..aligned]);
            stretch.pull(aligned_block)
        };

        if !samples.is_empty() {
            let mut chunk = AudioChunk { generation, tempo, samples };
            loop {
                match audio_tx.send_timeout(chunk, Duration::from_millis(10)) {
                    Ok(()) => break,
                    Err(SendTimeoutError::Disconnected(_)) => break 'decode,
                    Err(SendTimeoutError::Timeout(returned)) => {
                        chunk = returned;
                        match apply_pending_commands(collect_pending_commands(None, &command_rx), &mut decoder, &mut stretch, &mut tempo, &mut generation, &state)? {
                            CommandOutcome::Stop => break 'decode,
                            CommandOutcome::Seeked => continue 'decode,
                            CommandOutcome::Continue => {}
                        }
                    }
                }
            }
        }
        if at_end {
            state.decoder_end_generation.store(generation, Ordering::Release);
            loop {
                let Ok(command) = command_rx.recv() else { break 'decode };
                match apply_pending_commands(collect_pending_commands(Some(command), &command_rx), &mut decoder, &mut stretch, &mut tempo, &mut generation, &state)? {
                    CommandOutcome::Stop => break 'decode,
                    CommandOutcome::Seeked => continue 'decode,
                    CommandOutcome::Continue => {}
                }
            }
        }
    }
    Ok(())
}

struct WorkerSource {
    audio_rx: Receiver<AudioChunk>,
    state: Arc<SharedState>,
    current: Vec<f32>,
    index: usize,
    current_generation: Option<u64>,
    chunk_tempo: f32,
    channels: NonZeroU16,
    sample_rate: NonZeroU32,
    total_duration: Duration,
    frames_since_update: u32,
    source_position_us: f64,
    disconnected: bool,
}

impl WorkerSource {
    fn new(audio_rx: Receiver<AudioChunk>, state: Arc<SharedState>, channels: NonZeroU16, sample_rate: NonZeroU32, total_duration: Duration) -> Self {
        Self { audio_rx, state, current: Vec::new(), index: 0, current_generation: None, chunk_tempo: 1.0, channels, sample_rate, total_duration, frames_since_update: 0, source_position_us: 0.0, disconnected: false }
    }

    fn next_chunk(&mut self) {
        loop {
            match self.audio_rx.try_recv() {
                Ok(chunk) if chunk.generation == self.state.generation.load(Ordering::Acquire) => {
                    if self.current_generation != Some(chunk.generation) {
                        self.source_position_us = self.state.position_micros() as f64;
                        self.frames_since_update = 0;
                    }
                    self.current = chunk.samples;
                    self.index = 0;
                    self.current_generation = Some(chunk.generation);
                    self.chunk_tempo = chunk.tempo;
                    return;
                }
                Ok(_) => continue,
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => {
                    self.disconnected = true;
                    return;
                }
            }
        }
    }
}

impl Iterator for WorkerSource {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        // A seek changes the generation before the decoder has produced audio
        // at the new position. Drop the currently-playing old chunk immediately
        // so that it cannot overwrite the requested position in SharedState.
        if self.current_generation.is_some_and(|generation| generation != self.state.generation.load(Ordering::Acquire)) {
            self.current.clear();
            self.index = 0;
        }
        if self.index >= self.current.len() {
            self.next_chunk();
        }
        if self.index >= self.current.len() {
            if self.disconnected {
                return None;
            }
            let end_generation = self.state.decoder_end_generation.load(Ordering::Acquire);
            if end_generation != NO_GENERATION && self.state.set_position_if_generation(end_generation, self.total_duration.as_micros() as u64) {
                self.state.playback_finished_generation.store(end_generation, Ordering::Release);
            }
            return Some(0.0);
        }

        let sample = self.current[self.index];
        self.index += 1;
        if self.index.is_multiple_of(self.channels.get() as usize) {
            self.source_position_us += self.chunk_tempo as f64 * 1_000_000.0 / self.sample_rate.get() as f64;
            self.frames_since_update += 1;
            if self.frames_since_update >= 256 {
                if let Some(generation) = self.current_generation {
                    let position = (self.source_position_us as u64).min(self.total_duration.as_micros() as u64);
                    self.state.set_position_if_generation(generation, position);
                }
                self.frames_since_update = 0;
            }
        }
        Some(sample)
    }
}

impl Source for WorkerSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> NonZeroU16 {
        self.channels
    }

    fn sample_rate(&self) -> NonZeroU32 {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<Duration> {
        Some(self.total_duration)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_state(generation: u64, position_us: u64) -> Arc<SharedState> {
        Arc::new(SharedState {
            generation: AtomicU64::new(generation),
            position: Mutex::new(PositionState { generation, micros: position_us }),
            decoder_end_generation: AtomicU64::new(NO_GENERATION),
            playback_finished_generation: AtomicU64::new(NO_GENERATION),
            error: Mutex::new(None),
        })
    }

    fn test_source(audio_rx: Receiver<AudioChunk>, state: Arc<SharedState>, channels: u16, sample_rate: u32) -> WorkerSource {
        WorkerSource::new(audio_rx, state, NonZeroU16::new(channels).unwrap(), NonZeroU32::new(sample_rate).unwrap(), Duration::from_secs(100))
    }

    #[test]
    fn seek_discards_the_current_old_generation_before_updating_position() {
        let (_audio_tx, audio_rx) = bounded(1);
        let state = test_state(1, 42_000_000);
        let mut source = test_source(audio_rx, state.clone(), 1, 1);
        source.current = vec![0.75];
        source.current_generation = Some(0);
        source.source_position_us = 5_000_000.0;

        assert_eq!(source.next(), Some(0.0));
        assert_eq!(state.position_micros(), 42_000_000);
    }

    #[test]
    fn stale_queued_chunks_are_skipped_after_a_seek() {
        let (audio_tx, audio_rx) = bounded(2);
        audio_tx.send(AudioChunk { generation: 0, tempo: 1.0, samples: vec![0.25] }).unwrap();
        audio_tx.send(AudioChunk { generation: 1, tempo: 1.0, samples: vec![0.75] }).unwrap();
        let state = test_state(1, 10_000_000);
        let mut source = test_source(audio_rx, state, 1, 1);

        assert_eq!(source.next(), Some(0.75));
    }

    #[test]
    fn position_accumulates_across_chunk_boundaries() {
        let (audio_tx, audio_rx) = bounded(2);
        for _ in 0..2 {
            audio_tx.send(AudioChunk { generation: 0, tempo: 1.0, samples: vec![0.5; 128] }).unwrap();
        }
        let state = test_state(0, 0);
        let mut source = test_source(audio_rx, state.clone(), 1, 256);

        for _ in 0..256 {
            assert_eq!(source.next(), Some(0.5));
        }
        assert_eq!(state.position_micros(), 1_000_000);
    }

    #[test]
    fn position_uses_source_time_at_non_default_tempo() {
        let (audio_tx, audio_rx) = bounded(1);
        audio_tx.send(AudioChunk { generation: 0, tempo: 2.0, samples: vec![0.5; 256] }).unwrap();
        let state = test_state(0, 3_000_000);
        let mut source = test_source(audio_rx, state.clone(), 1, 256);

        for _ in 0..256 {
            source.next();
        }
        assert_eq!(state.position_micros(), 5_000_000);
    }

    #[test]
    fn stereo_samples_advance_position_once_per_frame() {
        let (audio_tx, audio_rx) = bounded(1);
        audio_tx.send(AudioChunk { generation: 0, tempo: 1.0, samples: vec![0.5; 512] }).unwrap();
        let state = test_state(0, 0);
        let mut source = test_source(audio_rx, state.clone(), 2, 256);

        for _ in 0..512 {
            source.next();
        }
        assert_eq!(state.position_micros(), 1_000_000);
    }

    #[test]
    fn disconnected_source_ends_after_buffered_audio() {
        let (audio_tx, audio_rx) = bounded(1);
        audio_tx.send(AudioChunk { generation: 0, tempo: 1.0, samples: vec![0.5] }).unwrap();
        drop(audio_tx);
        let mut source = test_source(audio_rx, test_state(0, 0), 1, 1);

        assert_eq!(source.next(), Some(0.5));
        assert_eq!(source.next(), None);
    }

    #[test]
    fn decoder_end_keeps_source_alive_and_marks_playback_finished() {
        let (_audio_tx, audio_rx) = bounded(1);
        let state = test_state(0, 90_000_000);
        state.decoder_end_generation.store(0, Ordering::Release);
        let mut source = test_source(audio_rx, state.clone(), 1, 1);

        assert_eq!(source.next(), Some(0.0));
        assert_eq!(state.position_micros(), 100_000_000);
        assert!(state.playback_finished());
    }

    #[test]
    fn stale_end_of_stream_cannot_overwrite_a_newer_chapter_seek() {
        let (_audio_tx, audio_rx) = bounded(1);
        let state = test_state(2, 25_000_000);
        state.decoder_end_generation.store(1, Ordering::Release);
        let mut source = test_source(audio_rx, state.clone(), 1, 1);

        assert_eq!(source.next(), Some(0.0));
        assert_eq!(state.position_micros(), 25_000_000);
        assert!(!state.playback_finished());
    }

    #[test]
    fn failed_seek_submission_rolls_back_generation_and_position() {
        let (command_tx, command_rx) = unbounded();
        drop(command_rx);
        let state = test_state(7, 12_000_000);

        assert_eq!(request_seek(&state, &command_tx, Duration::from_secs(80)), Err("audio decoder stopped".to_owned()));
        assert_eq!(state.generation.load(Ordering::Acquire), 7);
        assert_eq!(state.position_micros(), 12_000_000);
    }

    #[test]
    fn rapid_seek_burst_preserves_the_latest_request_without_backpressure() {
        let (command_tx, command_rx) = unbounded();
        let state = test_state(0, 0);

        for second in 1..=10_000 {
            request_seek(&state, &command_tx, Duration::from_secs(second)).unwrap();
        }

        assert_eq!(command_rx.len(), 10_000);
        assert_eq!(state.generation.load(Ordering::Acquire), 10_000);
        assert_eq!(state.position_micros(), 10_000_000_000);
        let pending = collect_pending_commands(None, &command_rx);
        assert_eq!(pending.seek, Some((Duration::from_secs(10_000), 10_000)));
        assert_eq!(command_rx.len(), 0);
    }

    #[test]
    fn stale_position_updates_never_win_during_long_seek_sequence() {
        let state = test_state(0, 0);

        for generation in 1..=1_000 {
            state.generation.store(generation, Ordering::Release);
            *state.position.lock().unwrap() = PositionState { generation, micros: generation * 1_000_000 };
            assert!(!state.set_position_if_generation(generation - 1, u64::MAX));
            assert_eq!(state.position_micros(), generation * 1_000_000);
        }
    }

    #[test]
    fn seeking_after_finish_immediately_clears_finished_state() {
        let (command_tx, _command_rx) = unbounded();
        let state = test_state(3, 100_000_000);
        state.playback_finished_generation.store(3, Ordering::Release);
        assert!(state.playback_finished());

        request_seek(&state, &command_tx, Duration::from_secs(20)).unwrap();

        assert!(!state.playback_finished());
        assert_eq!(state.position_micros(), 20_000_000);
    }

    #[test]
    fn tempo_rejects_non_finite_values_and_clamps_extremes() {
        assert!(normalized_tempo(f32::NAN).is_err());
        assert!(normalized_tempo(f32::INFINITY).is_err());
        assert_eq!(normalized_tempo(0.0).unwrap(), wsola::MIN_TEMPO);
        assert_eq!(normalized_tempo(100.0).unwrap(), wsola::MAX_TEMPO);
        assert_eq!(normalized_tempo(1.25).unwrap(), 1.25);
    }

    #[test]
    fn seek_generation_exhaustion_fails_without_mutating_state() {
        let (command_tx, _command_rx) = unbounded();
        let state = test_state(NO_GENERATION - 1, 17_000_000);

        assert_eq!(request_seek(&state, &command_tx, Duration::from_secs(80)), Err("audio seek generation exhausted".to_owned()));
        assert_eq!(state.generation.load(Ordering::Acquire), NO_GENERATION - 1);
        assert_eq!(state.position_micros(), 17_000_000);
    }

    #[test]
    fn decoded_position_never_advances_past_total_duration() {
        let (audio_tx, audio_rx) = bounded(1);
        audio_tx.send(AudioChunk { generation: 0, tempo: 4.0, samples: vec![0.5; 256] }).unwrap();
        let state = test_state(0, 99_000_000);
        let mut source = test_source(audio_rx, state.clone(), 1, 256);

        for _ in 0..256 {
            source.next();
        }
        assert_eq!(state.position_micros(), 100_000_000);
    }

    #[test]
    fn collecting_no_commands_produces_no_work() {
        let (_command_tx, command_rx) = unbounded();

        let pending = collect_pending_commands(None, &command_rx);

        assert_eq!(pending.seek, None);
        assert_eq!(pending.tempo, None);
        assert!(!pending.stop);
    }

    #[test]
    fn mixed_command_burst_coalesces_seek_and_tempo_independently() {
        let (command_tx, command_rx) = unbounded();
        command_tx.send(Command::Seek { position: Duration::from_secs(4), generation: 1 }).unwrap();
        command_tx.send(Command::Tempo(0.75)).unwrap();
        command_tx.send(Command::Seek { position: Duration::from_secs(9), generation: 2 }).unwrap();
        command_tx.send(Command::Tempo(1.5)).unwrap();

        let pending = collect_pending_commands(None, &command_rx);

        assert_eq!(pending.seek, Some((Duration::from_secs(9), 2)));
        assert_eq!(pending.tempo, Some(1.5));
        assert!(!pending.stop);
    }

    #[test]
    fn first_command_and_queued_commands_are_coalesced_together() {
        let (command_tx, command_rx) = unbounded();
        command_tx.send(Command::Seek { position: Duration::from_secs(8), generation: 4 }).unwrap();
        command_tx.send(Command::Tempo(2.0)).unwrap();

        let pending = collect_pending_commands(Some(Command::Tempo(0.5)), &command_rx);

        assert_eq!(pending.seek, Some((Duration::from_secs(8), 4)));
        assert_eq!(pending.tempo, Some(2.0));
    }

    #[test]
    fn stop_is_preserved_while_the_queue_is_drained() {
        let (command_tx, command_rx) = unbounded();
        command_tx.send(Command::Stop).unwrap();
        command_tx.send(Command::Seek { position: Duration::from_secs(12), generation: 7 }).unwrap();

        let pending = collect_pending_commands(None, &command_rx);

        assert!(pending.stop);
        assert_eq!(pending.seek, Some((Duration::from_secs(12), 7)));
        assert!(command_rx.is_empty());
    }

    #[test]
    fn successful_seek_updates_state_and_enqueues_the_same_generation() {
        let (command_tx, command_rx) = unbounded();
        let state = test_state(41, 3_000_000);
        let target = Duration::from_micros(7_654_321);

        request_seek(&state, &command_tx, target).unwrap();

        assert_eq!(state.generation.load(Ordering::Acquire), 42);
        assert_eq!(state.position_micros(), 7_654_321);
        match command_rx.recv().unwrap() {
            Command::Seek { position, generation } => {
                assert_eq!(position, target);
                assert_eq!(generation, 42);
            }
            _ => panic!("expected seek command"),
        }
    }

    #[test]
    fn failed_seek_restores_a_preexisting_finished_state() {
        let (command_tx, command_rx) = unbounded();
        drop(command_rx);
        let state = test_state(5, 100_000_000);
        state.playback_finished_generation.store(5, Ordering::Release);

        assert!(request_seek(&state, &command_tx, Duration::from_secs(10)).is_err());

        assert_eq!(state.generation.load(Ordering::Acquire), 5);
        assert_eq!(state.position_micros(), 100_000_000);
        assert!(state.playback_finished());
    }

    #[test]
    fn position_update_only_accepts_the_current_position_generation() {
        let state = test_state(3, 1_000_000);

        assert!(!state.set_position_if_generation(2, 99_000_000));
        assert_eq!(state.position_micros(), 1_000_000);
        assert!(state.set_position_if_generation(3, 2_000_000));
        assert_eq!(state.position_micros(), 2_000_000);
    }

    #[test]
    fn finished_flag_only_applies_to_the_current_generation() {
        let state = test_state(9, 0);

        state.playback_finished_generation.store(8, Ordering::Release);
        assert!(!state.playback_finished());
        state.playback_finished_generation.store(9, Ordering::Release);
        assert!(state.playback_finished());
        state.generation.store(10, Ordering::Release);
        assert!(!state.playback_finished());
    }

    #[test]
    fn an_empty_live_source_yields_silence_without_finishing() {
        let (_audio_tx, audio_rx) = bounded(1);
        let state = test_state(0, 15_000_000);
        let mut source = test_source(audio_rx, state.clone(), 1, 48_000);

        assert_eq!(source.next(), Some(0.0));
        assert_eq!(state.position_micros(), 15_000_000);
        assert!(!state.playback_finished());
    }

    #[test]
    fn eof_waits_until_the_buffered_chunk_has_been_consumed() {
        let (audio_tx, audio_rx) = bounded(1);
        audio_tx.send(AudioChunk { generation: 0, tempo: 1.0, samples: vec![0.25] }).unwrap();
        let state = test_state(0, 99_000_000);
        state.decoder_end_generation.store(0, Ordering::Release);
        let mut source = test_source(audio_rx, state.clone(), 1, 1);

        assert_eq!(source.next(), Some(0.25));
        assert!(!state.playback_finished());
        assert_eq!(source.next(), Some(0.0));
        assert!(state.playback_finished());
        assert_eq!(state.position_micros(), 100_000_000);
    }

    #[test]
    fn eof_on_a_zero_duration_source_finishes_at_zero() {
        let (_audio_tx, audio_rx) = bounded(1);
        let state = test_state(0, 0);
        state.decoder_end_generation.store(0, Ordering::Release);
        let mut source = WorkerSource::new(audio_rx, state.clone(), NonZeroU16::new(1).unwrap(), NonZeroU32::new(48_000).unwrap(), Duration::ZERO);

        assert_eq!(source.next(), Some(0.0));
        assert_eq!(state.position_micros(), 0);
        assert!(state.playback_finished());
    }

    #[test]
    fn surround_audio_advances_position_once_per_complete_frame() {
        let (audio_tx, audio_rx) = bounded(1);
        audio_tx.send(AudioChunk { generation: 0, tempo: 1.0, samples: vec![0.5; 256 * 6] }).unwrap();
        let state = test_state(0, 0);
        let mut source = test_source(audio_rx, state.clone(), 6, 256);

        for _ in 0..256 * 6 {
            source.next();
        }

        assert_eq!(state.position_micros(), 1_000_000);
    }

    #[test]
    fn position_is_published_at_the_exact_update_interval() {
        let (audio_tx, audio_rx) = bounded(2);
        audio_tx.send(AudioChunk { generation: 0, tempo: 1.0, samples: vec![0.5; 255] }).unwrap();
        audio_tx.send(AudioChunk { generation: 0, tempo: 1.0, samples: vec![0.5] }).unwrap();
        let state = test_state(0, 4_000_000);
        let mut source = test_source(audio_rx, state.clone(), 1, 256);

        for _ in 0..255 {
            source.next();
        }
        assert_eq!(state.position_micros(), 4_000_000);
        source.next();
        assert_eq!(state.position_micros(), 5_000_000);
    }

    #[test]
    fn generation_change_resets_partial_position_accounting() {
        let (audio_tx, audio_rx) = bounded(2);
        audio_tx.send(AudioChunk { generation: 0, tempo: 1.0, samples: vec![0.5; 255] }).unwrap();
        let state = test_state(0, 0);
        let mut source = test_source(audio_rx, state.clone(), 1, 256);
        for _ in 0..255 {
            source.next();
        }

        state.generation.store(1, Ordering::Release);
        *state.position.lock().unwrap() = PositionState { generation: 1, micros: 10_000_000 };
        audio_tx.send(AudioChunk { generation: 1, tempo: 1.0, samples: vec![0.75; 256] }).unwrap();
        for _ in 0..256 {
            assert_eq!(source.next(), Some(0.75));
        }

        assert_eq!(state.position_micros(), 11_000_000);
    }

    #[test]
    fn tempo_changes_between_chunks_accumulate_source_time_correctly() {
        let (audio_tx, audio_rx) = bounded(2);
        audio_tx.send(AudioChunk { generation: 0, tempo: 0.5, samples: vec![0.5; 128] }).unwrap();
        audio_tx.send(AudioChunk { generation: 0, tempo: 1.5, samples: vec![0.5; 128] }).unwrap();
        let state = test_state(0, 2_000_000);
        let mut source = test_source(audio_rx, state.clone(), 1, 256);

        for _ in 0..256 {
            source.next();
        }

        assert_eq!(state.position_micros(), 3_000_000);
    }

    #[test]
    fn stale_chunks_followed_by_disconnect_end_cleanly() {
        let (audio_tx, audio_rx) = bounded(2);
        audio_tx.send(AudioChunk { generation: 2, tempo: 1.0, samples: vec![0.25; 8] }).unwrap();
        drop(audio_tx);
        let state = test_state(3, 7_000_000);
        let mut source = test_source(audio_rx, state.clone(), 1, 1);

        assert_eq!(source.next(), None);
        assert_eq!(state.position_micros(), 7_000_000);
    }

    #[test]
    fn source_reports_its_audio_format_and_duration() {
        let (_audio_tx, audio_rx) = bounded(1);
        let source = WorkerSource::new(audio_rx, test_state(0, 0), NonZeroU16::new(2).unwrap(), NonZeroU32::new(44_100).unwrap(), Duration::from_secs(321));

        assert_eq!(source.channels(), NonZeroU16::new(2).unwrap());
        assert_eq!(source.sample_rate(), NonZeroU32::new(44_100).unwrap());
        assert_eq!(source.total_duration(), Some(Duration::from_secs(321)));
        assert_eq!(source.current_span_len(), None);
    }

    struct IntermittentDecoder {
        reads: std::vec::IntoIter<Option<f32>>,
    }

    impl Iterator for IntermittentDecoder {
        type Item = f32;

        fn next(&mut self) -> Option<Self::Item> {
            self.reads.next().flatten()
        }
    }

    #[test]
    fn transient_empty_decoder_reads_are_not_treated_as_eof() {
        for empty_reads in 1..EOF_CONFIRMATION_READS {
            let mut reads = vec![None; empty_reads];
            reads.extend([Some(0.25), Some(0.5), None]);
            let mut decoder = IntermittentDecoder { reads: reads.into_iter() };

            assert_eq!(read_decoder_block(&mut decoder, 16), [0.25, 0.5]);
        }
    }

    #[test]
    fn repeated_empty_decoder_reads_confirm_eof() {
        let mut decoder = IntermittentDecoder { reads: vec![None; EOF_CONFIRMATION_READS].into_iter() };

        assert!(read_decoder_block(&mut decoder, 16).is_empty());
    }

    #[test]
    fn byte_length_probe_restores_type_erased_reader_position() {
        let mut reader: Box<dyn AudiobookReadSeek> = Box::new(std::io::Cursor::new(vec![0_u8; 4096]));
        reader.seek(SeekFrom::Start(137)).unwrap();

        assert_eq!(stream_byte_len(&mut *reader), Ok(4096));
        assert_eq!(reader.stream_position().unwrap(), 137);
    }
}
