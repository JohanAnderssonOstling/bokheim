//! Shared narration state and playback controls.
use crate::settings::{NarrationPauses, NarrationVoice, NarrationVoicePreference, ReaderPreferences};
mod voice_picker;
use audiobook_player::{PlaybackMenu, SleepChoice, SleepPlan, format_playback_rate, sleep_menu, speed_menu};
use components::playback_secondary_button;
use gpui::prelude::*;
use gpui::{AnyElement, App, Context, SharedString, Task, div, px};
use gpui_component::menu::DropdownMenu;
use gpui_component::{Disableable, Icon, IconName};
#[cfg(not(target_arch = "wasm32"))]
use reader_speech::Voice;
use reader_speech::{DesktopSpeech, DeviceVoice, Pauses, SpeechControl, SpeechEvent, SpeechFailure, SpeechPassage, supports_languages};
use std::{collections::BTreeMap, time::Duration};
use ui_components::{self as components, PlaybackTransportAction};
use web_time::Instant;

pub(crate) fn speech_error_message(failure: SpeechFailure, diagnostic: &str) -> &'static str {
    log::warn!("Read aloud failed ({failure:?}): {diagnostic}");
    match failure {
        SpeechFailure::Interrupted => "Read aloud stopped unexpectedly. Start it again; if it persists, restart the app.",
        SpeechFailure::VoiceUnavailable => "Couldn't load the selected voice. Try another voice; if it persists, report the problem.",
        SpeechFailure::AudioOutput => "Couldn't play read-aloud audio. Check your speakers, headphones or Bluetooth connection, then retry.",
        SpeechFailure::Synthesis => "Couldn't generate speech for this passage. Try another voice or passage; if it persists, report the problem.",
        SpeechFailure::Blocked => "The browser refused read aloud. Start it again from the read-aloud button; if it persists, check this site's permissions.",
        SpeechFailure::ServiceUnavailable => "Couldn't start the speech service. Retry; if it persists, restart the app.",
    }
}

#[derive(Debug)]
pub(crate) struct SpeechStartError {
    failure: SpeechFailure,
    diagnostic: String,
    explanation: Option<String>,
}

impl SpeechStartError {
    fn new(failure: SpeechFailure, diagnostic: String) -> Self {
        Self { failure, diagnostic, explanation: None }
    }

    pub fn user_message(&self) -> &str {
        self.explanation.as_deref().unwrap_or_else(|| speech_error_message(self.failure, &self.diagnostic))
    }
}

#[derive(Default)]
pub(crate) struct Narration {
    pub service: Option<DesktopSpeech>,
    languages: Vec<String>,
    device_voice: Option<String>,
    voice_preferences: BTreeMap<String, NarrationVoicePreference>,
    device_voices: Vec<DeviceVoice>,
    voices_loading: bool,
    voices_error: Option<String>,
    voices_revision: u64,
    voices_task: Option<Task<()>>,
    #[cfg(target_arch = "wasm32")]
    availability_watch: Option<reader_speech::AvailabilityWatch>,
    #[cfg(target_arch = "wasm32")]
    availability_task: Option<Task<()>>,
    pub generation: u64,
    pub playing: bool,
    pub paused: bool,
    pub needs_restart: bool,
    pub finished: bool,
    pub current: Option<usize>,
    last_started: Option<(u64, usize)>,
    pub status: String,
    pub unavailable_reason: Option<String>,
    pub events_task: Option<Task<()>>,
    pub navigation_task: Option<Task<()>>,
    pub sleep_task: Option<Task<()>>,
    pub sleep: Option<SleepPlan>,
    pub end_fragment: Option<usize>,
    /// Whether the dock is on screen. It opens when reading starts and stays,
    /// paused, stopped or finished, until its close button.
    pub dock_open: bool,
    rate: Option<f64>,
    pauses: NarrationPauses,
    voice: NarrationVoice,
    media_title: String,
    media_author: String,
    media_chapter: String,
    media_cover: Vec<u8>,
    #[cfg(target_os = "android")]
    volume_button_capture: Option<bool>,
}

/// The pause choices in menu order, with what each asks of the voice.
const PAUSE_CHOICES: [(NarrationPauses, &str, Pauses); 4] = [
    (NarrationPauses::Short, "Short pauses", Pauses { sentence_scale: 0.6, passage_gap: 0.0 }),
    (NarrationPauses::Natural, "Natural pauses", Pauses::NATURAL),
    (NarrationPauses::Long, "Long pauses", Pauses { sentence_scale: 1.5, passage_gap: 0.4 }),
    (NarrationPauses::Longer, "Longer pauses", Pauses { sentence_scale: 2.0, passage_gap: 0.8 }),
];

#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
const VOICE_CHOICES: [(NarrationVoice, &str, &str, Voice); 2] = [(NarrationVoice::Alba, "Alba", "Alba · Scottish English", Voice::Alba), (NarrationVoice::George, "George", "George · British English", Voice::George)];

#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
fn speech_voice(choice: NarrationVoice) -> Voice {
    VOICE_CHOICES.into_iter().find(|(voice, ..)| *voice == choice).map_or(Voice::Alba, |(.., voice)| voice)
}

#[cfg(target_os = "android")]
fn speech_voice(_choice: NarrationVoice) -> Voice {
    // Older saved preferences may still select George, which Android no longer bundles.
    Voice::Alba
}

fn speech_pauses(choice: NarrationPauses) -> Pauses {
    PAUSE_CHOICES.into_iter().find(|(pauses, ..)| *pauses == choice).map_or(Pauses::NATURAL, |(.., speech)| speech)
}

impl Narration {
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.media_title = title.into();
        self
    }
    fn sync_media_metadata(&self) {
        if let Some(service) = &self.service {
            service.set_title(&self.media_title);
            service.set_media_metadata(&self.media_author, &self.media_chapter, &self.media_cover);
        }
    }

    pub fn set_chapter(&mut self, chapter: String) {
        if self.media_chapter != chapter {
            self.media_chapter = chapter;
            self.sync_media_metadata();
        }
    }

    /// An active narration session reserves hardware volume keys for Android's
    /// media volume, including while paused or preparing the next passage,
    /// unless the reader explicitly uses them to advance a reading ruler.
    #[cfg(target_os = "android")]
    pub fn sync_volume_buttons(&mut self, ruler_active: bool, cx: &App) {
        let capture = ruler_active || !self.playing;
        if self.volume_button_capture != Some(capture) {
            self.volume_button_capture = Some(capture);
            cx.set_volume_button_capture(capture);
        }
    }

    pub fn for_languages(languages: &[String]) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let unavailable_reason = (!supports_languages(languages)).then(|| "This book’s declared language is not supported. Text to speech currently reads English.".into());
        #[cfg(target_arch = "wasm32")]
        let unavailable_reason = None;
        Self { unavailable_reason, languages: languages.to_vec(), ..Default::default() }
    }

    pub fn available(&self) -> bool {
        true
    }

    pub fn with_availability<C: TtsReader>(mut self, cx: &mut Context<C>) -> Self {
        self.reload_voices(cx);
        #[cfg(target_arch = "wasm32")]
        {
            let (watch, events) = reader_speech::AvailabilityWatch::new();
            self.availability_watch = Some(watch);
            self.availability_task = Some(cx.spawn(async move |this, cx| {
                while events.recv().await.is_ok() {
                    if this
                        .update(cx, |reader, cx| {
                            reader.narration_mut().reload_voices(cx);
                            cx.notify();
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            }));
        }
        self
    }

    fn apply_voices(&mut self, result: Result<Vec<DeviceVoice>, String>) {
        self.voices_loading = false;
        match result {
            Ok(voices) => {
                self.device_voices = voices;
                if self.voice == NarrationVoice::Device {
                    if let Some(voice) = self.device_voices.iter().find(|voice| Some(&voice.id) == self.device_voice.as_ref()) {
                        let language = reader_speech::language_base(&voice.language);
                        if !language.is_empty() {
                            self.voice_preferences.entry(language).or_insert_with(|| NarrationVoicePreference { voice: NarrationVoice::Device, device_voice: Some(voice.id.clone()) });
                        }
                        if !reader_speech::voice_matches_languages(voice, &self.languages) && self.preferred_voice().is_none() {
                            self.device_voice = None;
                        }
                    }
                }
                self.voices_error = None;
            }
            Err(error) => {
                self.device_voices.clear();
                self.voices_error = Some(error);
            }
        }
        self.refresh_availability();
    }

    fn reload_voices<C: TtsReader>(&mut self, cx: &mut Context<C>) {
        self.voices_revision = self.voices_revision.wrapping_add(1);
        #[cfg(target_arch = "wasm32")]
        self.apply_voices(reader_speech::device_voices());
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.voices_loading = true;
            let revision = self.voices_revision;
            self.voices_task = Some(cx.spawn(async move |this, cx| {
                let discovery = cx.background_executor().spawn(async { reader_speech::device_voices() });
                let timer = cx.background_executor().timer(Duration::from_secs(20));
                let result = futures_lite::future::or(async { discovery.await }, async {
                    timer.await;
                    Err("Local voice discovery took too long. Try again.".into())
                })
                .await;
                let _ = this.update(cx, |reader, cx| {
                    let state = reader.narration_mut();
                    if state.voices_revision == revision {
                        state.apply_voices(result);
                        cx.notify();
                    }
                });
            }));
        }
        self.refresh_availability();
    }

    pub(crate) fn chunk_language(&self) -> String {
        if self.voice == NarrationVoice::Device {
            self.device_voices.iter().find(|voice| Some(&voice.id) == self.device_voice.as_ref()).map(|voice| voice.language.clone()).or_else(|| self.languages.first().cloned()).unwrap_or_else(|| "und".into())
        } else {
            "en".into()
        }
    }

    fn matching_voices(&self) -> Vec<DeviceVoice> {
        reader_speech::matching_device_voices(&self.device_voices, &self.languages)
    }

    fn preferred_voice(&self) -> Option<&NarrationVoicePreference> {
        self.languages.iter().find_map(|language| {
            let preference = self.voice_preferences.get(&reader_speech::language_base(language))?;
            #[cfg(target_arch = "wasm32")]
            if preference.voice != NarrationVoice::Device {
                return None;
            }
            Some(preference)
        })
    }

    fn remember_voice(&mut self, language: &str, voice: NarrationVoice, id: Option<String>) {
        let language = reader_speech::language_base(language);
        if language.is_empty() {
            self.select_voice(voice, id);
            return;
        }
        self.voice_preferences.insert(language.clone(), NarrationVoicePreference { voice, device_voice: id.clone() });
        if self.languages.iter().all(|language| reader_speech::language_base(language).is_empty()) || self.languages.iter().any(|book_language| reader_speech::language_base(book_language) == language) {
            self.select_voice(voice, id);
        }
    }

    fn refresh_availability(&mut self) {
        if let Some(preference) = self.preferred_voice().cloned() {
            self.voice = preference.voice;
            self.device_voice = preference.device_voice;
        }
        if self.voice == NarrationVoice::Device {
            self.unavailable_reason = if self.voices_loading {
                Some("Finding installed local voices…".into())
            } else if let Some(error) = &self.voices_error {
                Some(error.clone())
            } else {
                let voices = self.matching_voices();
                if self.device_voice.is_none() {
                    self.device_voice = voices.first().map(|voice| voice.id.clone());
                }
                if voices.iter().any(|voice| Some(&voice.id) == self.device_voice.as_ref()) {
                    None
                } else if self.device_voice.is_some() {
                    Some("The selected local voice is unavailable for this book. Choose another installed voice.".into())
                } else {
                    Some("No installed voice matches this book’s language. Install one in device settings or choose a matching built-in voice.".into())
                }
            };
        } else {
            #[cfg(not(target_arch = "wasm32"))]
            {
                self.unavailable_reason = (!supports_languages(&self.languages)).then(|| "No built-in voice supports this book’s language. Choose a matching installed voice.".into());
            }
            #[cfg(target_arch = "wasm32")]
            {
                self.voice = NarrationVoice::Device;
                self.refresh_availability();
            }
        }
    }

    pub fn start(&mut self, text: Vec<SpeechPassage>, index: usize) -> Result<Option<async_channel::Receiver<SpeechEvent>>, SpeechStartError> {
        self.begin(index, |service| service.replace_passages_from(text, index))
    }

    pub fn start_items(&mut self, count: usize, index: usize, glyph_start: u32) -> Result<Option<async_channel::Receiver<SpeechEvent>>, SpeechStartError> {
        self.begin(index, |service| service.read_items_from(count, index, glyph_start))
    }

    fn begin(&mut self, index: usize, start: impl FnOnce(&DesktopSpeech) -> u64) -> Result<Option<async_channel::Receiver<SpeechEvent>>, SpeechStartError> {
        self.refresh_availability();
        if let Some(reason) = &self.unavailable_reason {
            return Err(SpeechStartError { failure: SpeechFailure::VoiceUnavailable, diagnostic: reason.clone(), explanation: Some(reason.clone()) });
        }
        let events = if self.service.is_none() {
            let service = DesktopSpeech::new().map_err(|diagnostic| SpeechStartError::new(SpeechFailure::ServiceUnavailable, diagnostic))?;
            let events = service.events();
            self.service = Some(service);
            Some(events)
        } else {
            None
        };
        let Some(service) = self.service.as_ref() else {
            return Err(SpeechStartError::new(SpeechFailure::ServiceUnavailable, "speech service is unavailable".into()));
        };
        service.set_title(&self.media_title);
        service.set_media_metadata(&self.media_author, &self.media_chapter, &self.media_cover);
        service.set_rate(self.rate() as f32);
        service.set_pauses(if self.voice == NarrationVoice::Device { Pauses::NATURAL } else { speech_pauses(self.pauses) });
        #[cfg(not(target_arch = "wasm32"))]
        service.set_voice(speech_voice(self.voice));
        let language = self.chunk_language();
        service.set_languages(std::slice::from_ref(&language));
        service.set_device_voice(if self.voice == NarrationVoice::Device { self.device_voice.as_deref() } else { None }).map_err(|diagnostic| SpeechStartError::new(SpeechFailure::VoiceUnavailable, diagnostic))?;
        self.generation = start(service);
        self.playing = true;
        self.paused = false;
        self.needs_restart = false;
        self.finished = false;
        self.current = Some(index);
        self.end_fragment = None;
        self.status = "Preparing text to speech…".into();
        Ok(events)
    }

    pub fn accepts(&self, event: &SpeechEvent) -> bool {
        let generation = match event {
            SpeechEvent::Requested { generation, .. } | SpeechEvent::Started { generation, .. } | SpeechEvent::Finished { generation } | SpeechEvent::Failed { generation, .. } | SpeechEvent::Remote { generation, .. } => *generation,
        };
        self.playing && generation == self.generation
    }

    pub fn started_item(&mut self, index: usize) -> bool {
        let item = (self.generation, index);
        let changed = self.last_started != Some(item);
        self.last_started = Some(item);
        self.current = Some(index);
        self.status = if self.paused { "Paused" } else { "Reading" }.into();
        changed
    }

    pub fn pause(&mut self) {
        if let Some(service) = &self.service {
            service.pause();
        }
        self.paused = true;
        self.status = "Paused".into();
    }

    pub fn resume(&mut self) {
        if let Some(service) = &self.service {
            service.resume();
        }
        self.paused = false;
        self.status = "Reading".into();
    }

    pub fn navigated(&mut self) {
        if let Some(service) = &self.service {
            self.generation = service.cancel();
        }
        self.needs_restart = true;
        self.finished = false;
        self.current = None;
        self.status = if self.paused { "Paused at the new page" } else { "Moving to the new page…" }.into();
    }

    pub fn stop(&mut self) {
        if let Some(service) = &self.service {
            service.stop();
        }
        self.navigation_task = None;
        self.sleep_task = None;
        self.sleep = None;
        self.end_fragment = None;
        self.needs_restart = false;
        self.playing = false;
        self.paused = false;
        self.finished = false;
        self.current = None;
        self.last_started = None;
        self.status = "Stopped".into();
    }
    /// Closing releases this reader's endpoint and listener, and cancels its
    /// navigation/sleep work. Dropping the endpoint also unloads its voice.
    pub fn close(&mut self) {
        self.stop();
        self.events_task = None;
        self.service = None;
        self.dock_open = false;
    }

    /// What the TTS button in the rail or bottom bar shows.
    pub fn button_mark(&self) -> components::ReaderRailMark {
        match (self.playing, self.paused) {
            (true, false) => components::ReaderRailMark::Active,
            (true, true) => components::ReaderRailMark::Paused,
            (false, _) => components::ReaderRailMark::None,
        }
    }

    pub fn rate(&self) -> f64 {
        self.rate.unwrap_or(1.0)
    }

    pub fn set_rate(&mut self, rate: f64) {
        self.rate = Some(rate);
        if let Some(service) = &self.service {
            self.generation = service.set_rate(rate as f32);
        }
    }

    pub fn with_rate(mut self, rate: f64) -> Self {
        self.rate = Some(rate);
        self
    }

    pub fn set_pauses(&mut self, pauses: NarrationPauses) {
        self.pauses = pauses;
        if let Some(service) = &self.service {
            self.generation = service.set_pauses(speech_pauses(pauses));
        }
    }

    pub fn with_pauses(mut self, pauses: NarrationPauses) -> Self {
        self.pauses = pauses;
        self
    }

    pub fn with_voice(mut self, voice: NarrationVoice) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            #[cfg(target_os = "android")]
            let voice = if voice == NarrationVoice::George { NarrationVoice::Alba } else { voice };
            self.voice = voice;
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = voice;
            self.voice = NarrationVoice::Device;
        }
        self.refresh_availability();
        self
    }

    pub fn with_device_voice(mut self, id: Option<String>) -> Self {
        self.device_voice = id;
        self.refresh_availability();
        self
    }

    pub fn with_voice_preferences(mut self, preferences: &ReaderPreferences) -> Self {
        self.voice_preferences = preferences.narration_voices.clone();
        if self.voice != NarrationVoice::Device {
            self.voice_preferences.entry("en".into()).or_insert_with(|| NarrationVoicePreference { voice: self.voice, device_voice: None });
        }
        self.refresh_availability();
        self
    }

    fn select_voice(&mut self, voice: NarrationVoice, id: Option<String>) {
        self.voice = voice;
        self.device_voice = id;
        self.refresh_availability();
        let voice = self.voice;
        if self.unavailable_reason.is_some() {
            if let Some(service) = &self.service {
                service.stop();
            }
            self.playing = false;
            self.needs_restart = true;
            return;
        }
        if let Some(service) = &self.service {
            #[cfg(not(target_arch = "wasm32"))]
            service.set_voice(speech_voice(voice));
            service.set_languages(std::slice::from_ref(&self.chunk_language()));
            service.set_pauses(if voice == NarrationVoice::Device { Pauses::NATURAL } else { speech_pauses(self.pauses) });
            match service.set_device_voice(if voice == NarrationVoice::Device { self.device_voice.as_deref() } else { None }) {
                Ok(generation) => {
                    self.generation = generation;
                    if self.playing && !self.paused {
                        self.status = "Preparing the selected local voice…".into();
                    }
                }
                Err(error) => {
                    service.stop();
                    self.playing = false;
                    self.needs_restart = true;
                    self.unavailable_reason = Some(error);
                }
            }
        }
    }

    pub fn skip_to(&mut self, index: usize) {
        if self.playing
            && !self.needs_restart
            && let Some(service) = &self.service
        {
            self.generation = service.skip_to(index);
            self.finished = false;
            self.current = Some(index);
        }
    }

    pub fn limit_to(&mut self, end: Option<usize>) {
        if self.end_fragment != end {
            self.end_fragment = end;
            if self.playing
                && !self.needs_restart
                && let Some(service) = &self.service
            {
                self.generation = service.set_end_fragment(end);
            }
            self.finished = false;
        }
    }

    fn sleep_label(&self) -> Option<SharedString> {
        self.sleep.map(|plan| plan.label(Instant::now()))
    }
}

pub(crate) fn adjacent_item(current: usize, count: usize, forward: bool) -> Option<usize> {
    if current >= count {
        return None;
    }
    if forward { current.checked_add(1).filter(|index| *index < count) } else { current.checked_sub(1) }
}

pub(crate) trait TtsReader: Sized + 'static {
    fn narration(&self) -> &Narration;
    fn narration_mut(&mut self) -> &mut Narration;
    fn toggle_active_narration(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.narration_mut().playing {
            return false;
        }
        self.play_pause_tts(cx);
        true
    }

    fn chapter_key(&self, cx: &App) -> usize;
    fn chapter_label(&self, cx: &App) -> String {
        format!("Chapter {}", self.chapter_key(cx) + 1)
    }
    fn update_tts_chapter_limit(&mut self, _cx: &mut Context<Self>) {}
    fn skip_tts_element(&mut self, forward: bool, cx: &mut Context<Self>);
    fn sleep_tts(&mut self, cx: &mut Context<Self>) {
        let state = self.narration_mut();
        state.pause();
        state.sleep = None;
        state.sleep_task = None;
        state.limit_to(None);
        cx.notify();
    }
    fn start_tts(&mut self, cx: &mut Context<Self>);
    fn toggle_tts_pause(&mut self, cx: &mut Context<Self>);
    fn stop_tts(&mut self, cx: &mut Context<Self>);
    fn speech_event(&mut self, event: SpeechEvent, cx: &mut Context<Self>);
    fn remote_control(&mut self, command: SpeechControl, cx: &mut Context<Self>) {
        match command {
            SpeechControl::Pause => self.narration_mut().pause(),
            SpeechControl::Resume => {
                if self.narration_mut().paused {
                    self.toggle_tts_pause(cx);
                }
            }
            SpeechControl::Stop => self.stop_tts(cx),
            SpeechControl::Item { generation, index } => {
                let state = self.narration_mut();
                state.generation = generation;
                state.finished = false;
                state.current = Some(index);
                state.status = if state.paused { "Paused" } else { "Reading" }.into();
            }
        }
        cx.notify();
    }
    /// The navigation button toggles the whole narration session and dock.
    fn press_tts(&mut self, cx: &mut Context<Self>) {
        if self.narration_mut().dock_open || self.narration_mut().playing {
            self.close_tts_dock(cx);
        } else {
            self.play_pause_tts(cx);
        }
    }

    /// The dock transport keeps its session open when paused or resumed.
    fn play_pause_tts(&mut self, cx: &mut Context<Self>) {
        if self.narration_mut().playing {
            self.toggle_tts_pause(cx);
        } else {
            self.narration_mut().dock_open = true;
            self.start_tts(cx);
        }
        cx.notify();
    }

    /// The dock's close button stops reading and takes the dock away.
    fn close_tts_dock(&mut self, cx: &mut Context<Self>) {
        self.stop_tts(cx);
        self.narration_mut().close();
        cx.notify();
    }
}

#[cfg(target_os = "android")]
pub(crate) fn load_media_metadata<C: TtsReader>(library: std::rc::Rc<library_backend::LibraryClient>, locator: library_model::BookLocator, cx: &mut Context<C>) {
    cx.spawn(async move |this, cx| {
        let detail = library.book_detail(locator.content_hash()).await.ok();
        let cover = library.thumbnail(locator.content_hash(), library_backend::ThumbnailResolution::Browse).await.ok().flatten().unwrap_or_default();
        let _ = this.update(cx, |reader, _| {
            let state = reader.narration_mut();
            if let Some(detail) = detail {
                state.media_title = detail.book.title;
                state.media_author = detail.authors.iter().map(|author| author.name.as_str()).collect::<Vec<_>>().join(", ");
            }
            state.media_cover = cover;
            state.sync_media_metadata();
        });
    })
    .detach();
}

pub(crate) fn listen<C: TtsReader>(events: async_channel::Receiver<SpeechEvent>, cx: &mut Context<C>) -> Task<()> {
    cx.spawn(async move |this, cx| {
        while let Ok(event) = events.recv().await {
            if this
                .update(cx, |reader, cx| {
                    reader.speech_event(event, cx);
                    let chapter = reader.chapter_label(cx);
                    reader.narration_mut().set_chapter(chapter);
                })
                .is_err()
            {
                break;
            }
        }
    })
}

fn choose_sleep<C: TtsReader>(reader: &mut C, choice: SleepChoice, cx: &mut Context<C>) {
    let chapter = reader.chapter_key(cx);
    let plan = choice.plan(chapter);
    let state = reader.narration_mut();
    state.sleep_task = None;
    state.sleep = plan;
    reader.update_tts_chapter_limit(cx);
    if let Some(until) = plan.and_then(SleepPlan::deadline) {
        reader.narration_mut().sleep_task = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let keep_waiting = this
                    .update(cx, |reader, cx| {
                        if reader.narration_mut().sleep.and_then(SleepPlan::deadline) != Some(until) {
                            return false;
                        }
                        if Instant::now() >= until {
                            reader.sleep_tts(cx);
                            return false;
                        }
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !keep_waiting {
                    break;
                }
            }
        }));
    }
    cx.notify();
}

/// The narration dock under the page, laid out like the audiobook dock but
/// with transport controls in the centre and tools alongside them on desktop.
/// Compact docks balance equal-width tool groups around the centred transport.
/// Every compact option opens a window-owned bottom sheet.
pub(crate) fn dock<C: TtsReader>(state: &Narration, compact: bool, theme: components::BrowserTheme, cx: &mut Context<C>) -> AnyElement {
    let playing = state.playing && !state.paused;
    let eligible = state.unavailable_reason.is_none();
    let status = state.unavailable_reason.clone().unwrap_or_else(|| if state.status.is_empty() { "Start reading".into() } else { state.status.clone() });
    let skippable = state.playing && !state.needs_restart;
    let transport = components::playback_transport_buttons("tts", playing, components::TransportLayout::Dock, None, components::PlaybackSkipKind::Elements, theme)
        .into_iter()
        .map(|(action, button)| {
            let button = button.when(compact, |button| button.size(px(if matches!(action, PlaybackTransportAction::PlayPause) { 64.0 } else { 32.0 }))).debug_selector(move || {
                match action {
                    PlaybackTransportAction::PlayPause => "tts-play",
                    PlaybackTransportAction::Back | PlaybackTransportAction::Previous => "tts-back",
                    PlaybackTransportAction::Forward | PlaybackTransportAction::Next => "tts-next",
                }
                .into()
            });
            match action {
                PlaybackTransportAction::PlayPause => button.disabled(!eligible).tooltip(status.clone()).on_click(cx.listener(|reader, _, _, cx| reader.play_pause_tts(cx))),
                PlaybackTransportAction::Back | PlaybackTransportAction::Previous => button.disabled(!skippable).on_click(cx.listener(|reader, _, _, cx| reader.skip_tts_element(false, cx))),
                PlaybackTransportAction::Forward | PlaybackTransportAction::Next => button.disabled(!skippable).on_click(cx.listener(|reader, _, _, cx| reader.skip_tts_element(true, cx))),
            }
        })
        .collect::<Vec<_>>();

    let value_button = |id: &'static str, label: SharedString| {
        if compact { playback_secondary_button(components::base_button(id).ghost()).size(px(32.0)).child(div().text_size(px(14.0)).whitespace_nowrap().child(label)) } else { components::player_dock_text_button(id, label) }
    };
    let rate = state.rate();
    let entity = cx.entity().downgrade();
    let set_rate = move |rate, cx: &mut App| {
        let _ = entity.update(cx, |reader, cx| {
            reader.narration_mut().set_rate(rate);
            crate::settings::ReaderSettings::update(cx, |preferences| preferences.audiobook_speed = rate);
            cx.notify();
        });
    };
    let speed = value_button("tts-speed", format_playback_rate(rate).into()).debug_selector(|| "tts-speed".into()).tooltip("Reading speed");
    let speed = if compact {
        speed.on_click(move |_, window, cx| speed_menu(rate).title("Reading speed").open_sheet(window, cx, set_rate.clone())).into_any_element()
    } else {
        speed.dropdown_menu_with_anchor(gpui::Anchor::BottomRight, move |menu, _, _| speed_menu(rate).dropdown(menu, set_rate.clone())).into_any_element()
    };

    let voice = voice_picker::button(state, compact, cx);

    let current_pauses = state.pauses;
    let entity = cx.entity().downgrade();
    let set_pauses = move |choice, cx: &mut App| {
        let _ = entity.update(cx, |reader, cx| {
            reader.narration_mut().set_pauses(choice);
            crate::settings::ReaderSettings::update(cx, |preferences| preferences.narration_pauses = choice);
            cx.notify();
        });
    };
    let pauses = playback_secondary_button(components::outlined_icon_button("tts-pauses", "Pauses between sentences and paragraphs", Icon::empty().path("icons/hourglass.svg")))
        .when(compact, |button| button.size(px(32.0)))
        .disabled(state.voice == NarrationVoice::Device)
        .when(state.voice == NarrationVoice::Device, |button| button.tooltip("Custom pauses are unavailable for device voices"))
        .debug_selector(|| "tts-pauses".into());
    let pauses = if compact {
        pauses
            .on_click(move |_, window, cx| {
                let choices = PAUSE_CHOICES.into_iter().map(|(choice, label, _)| (choice, label.into(), choice == current_pauses));
                PlaybackMenu::new("tts-pauses", "Pauses", choices).open_sheet(window, cx, set_pauses.clone());
            })
            .into_any_element()
    } else {
        pauses
            .dropdown_menu_with_anchor(gpui::Anchor::BottomRight, move |mut menu, _, _| {
                for (choice, label, _) in PAUSE_CHOICES {
                    let set_pauses = set_pauses.clone();
                    menu = menu.item(components::menu_item(label, move |_, _, cx| set_pauses(choice, cx)).checked(choice == current_pauses));
                }
                menu
            })
            .into_any_element()
    };

    let sleep_label = state.sleep_label();
    let sleep = match sleep_label.clone() {
        Some(label) => {
            let value = if compact {
                match state.sleep {
                    Some(SleepPlan::Until(deadline)) => format!("{}m", deadline.saturating_duration_since(Instant::now()).as_secs().div_ceil(60)).into(),
                    Some(SleepPlan::EndOfChapter(_)) => "Ch.".into(),
                    None => label.clone(),
                }
            } else {
                label.clone()
            };
            value_button("tts-sleep", value).tooltip(format!("Sleep timer: {label}"))
        }
        None => components::outlined_icon_button("tts-sleep", "Sleep timer", Icon::empty().path("icons/alarm.svg")),
    };
    let entity = cx.entity().downgrade();
    let set_sleep = move |choice, cx: &mut App| {
        let _ = entity.update(cx, |reader, cx| choose_sleep(reader, choice, cx));
    };
    let sleep = playback_secondary_button(sleep).when(compact, |button| button.size(px(32.0))).debug_selector(|| "tts-sleep".into()).when(sleep_label.is_some(), |button| button.text_color(theme.text_accent));
    let sleep = if compact {
        sleep.on_click(move |_, window, cx| sleep_menu(sleep_label.as_deref()).open_sheet(window, cx, set_sleep.clone())).into_any_element()
    } else {
        sleep.dropdown_menu_with_anchor(gpui::Anchor::BottomLeft, move |menu, _, _| sleep_menu(sleep_label.as_deref()).dropdown(menu, set_sleep.clone())).into_any_element()
    };
    let close = playback_secondary_button(components::outlined_icon_button("tts-close", "Close player", IconName::Close))
        .when(compact, |button| button.size(px(32.0)))
        .debug_selector(|| "tts-close".into())
        .on_click(cx.listener(|reader, _, _, cx| reader.close_tts_dock(cx)));

    if compact {
        let leading = components::row(0.0).flex_1().min_w_0().child(voice).child(speed).child(pauses);
        let transport = components::row(0.0).flex_none().children(transport);
        let trailing = components::row(0.0).flex_1().min_w_0().justify_end().child(sleep).child(close);
        return components::bottom_bar(theme)
            .occlude()
            .relative()
            .id("tts-dock")
            .debug_selector(|| "tts-dock".into())
            .child(components::row(0.0).w_full().h(gpui::rems(components::PLAYER_DOCK_HEIGHT_REM)).child(leading).child(transport).child(trailing))
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .into_any_element();
    }

    let trailing = components::player_dock_tools();
    let trailing = trailing.child(voice);
    let trailing = trailing.child(sleep).child(speed).child(pauses).child(close);

    let transport = components::player_dock_transport(None, transport, theme);
    components::player_dock(div(), transport, trailing, theme).id("tts-dock").on_scroll_wheel(|_, _, cx| cx.stop_propagation()).into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{AppContext, TestAppContext};

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn saved_voice_preferences_use_a_bundled_voice() {
        assert_eq!(speech_voice(NarrationVoice::Alba), Voice::Alba);
        let expected = if cfg!(target_os = "android") { Voice::Alba } else { Voice::George };
        assert_eq!(speech_voice(NarrationVoice::George), expected);
    }

    #[test]
    fn voices_are_remembered_per_language_and_regional_variants_share_a_choice() {
        let english = DeviceVoice { id: "english".into(), name: "English".into(), language: "en-GB".into() };
        let swedish = DeviceVoice { id: "swedish".into(), name: "Swedish".into(), language: "sv-SE".into() };
        let mut state = Narration::for_languages(&["en".into()]).with_voice(NarrationVoice::Device).with_device_voice(Some(english.id.clone()));
        state.apply_voices(Ok(vec![english.clone(), swedish.clone()]));
        state.current = Some(12);
        state.remember_voice("swe", NarrationVoice::Device, Some(swedish.id.clone()));
        assert_eq!(state.device_voice.as_deref(), Some("english"));
        assert_eq!(state.current, Some(12));
        assert_eq!(state.voice_preferences.len(), 2);
        state.remember_voice("en-US", NarrationVoice::Device, Some("another-english".into()));
        assert_eq!(state.voice_preferences.len(), 2);
        assert_eq!(state.voice_preferences["en"].device_voice.as_deref(), Some("another-english"));
        assert_eq!(state.voice_preferences["sv"].device_voice.as_deref(), Some("swedish"));

        let preferences = ReaderPreferences { narration_voices: state.voice_preferences, ..Default::default() };
        let mut restored = Narration::for_languages(&["sv".into()]).with_voice_preferences(&preferences);
        restored.apply_voices(Ok(vec![english, swedish]));
        assert_eq!(restored.voice, NarrationVoice::Device);
        assert_eq!(restored.device_voice.as_deref(), Some("swedish"));
        assert!(restored.unavailable_reason.is_none());
        restored.apply_voices(Ok(vec![]));
        assert_eq!(restored.device_voice.as_deref(), Some("swedish"));
        assert!(restored.unavailable_reason.is_some());
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn bundled_voice_replaces_only_its_own_language_selection() {
        let mut state = Narration::for_languages(&["sv".into()]).with_voice(NarrationVoice::Device);
        state.apply_voices(Ok(vec![DeviceVoice { id: "swedish".into(), name: "Swedish".into(), language: "sv-SE".into() }]));
        state.remember_voice("sv", NarrationVoice::Device, Some("swedish".into()));
        state.remember_voice("en-GB", NarrationVoice::Alba, None);
        assert_eq!(state.voice, NarrationVoice::Device);
        assert_eq!(state.device_voice.as_deref(), Some("swedish"));
        assert_eq!(state.voice_preferences["en"].voice, NarrationVoice::Alba);
        assert_eq!(state.voice_preferences["sv"].device_voice.as_deref(), Some("swedish"));
        let preferences = ReaderPreferences { narration_voices: state.voice_preferences, ..Default::default() };
        let restored = Narration::for_languages(&["en-US".into()]).with_voice_preferences(&preferences);
        assert_eq!(restored.voice, NarrationVoice::Alba);
        assert!(restored.device_voice.is_none());
    }

    #[test]
    fn missing_saved_local_voice_requires_explicit_replacement() {
        let mut state = Narration::for_languages(&["sv".into()]).with_voice(NarrationVoice::Device).with_device_voice(Some("removed".into()));
        state.apply_voices(Ok(vec![DeviceVoice { id: "installed".into(), name: "Swedish".into(), language: "sv-SE".into() }]));
        assert_eq!(state.device_voice.as_deref(), Some("removed"));
        assert!(state.unavailable_reason.is_some());
        assert!(state.start(vec![], 0).is_err());
        assert!(state.service.is_none());
        state.select_voice(NarrationVoice::Device, Some("installed".into()));
        assert!(state.unavailable_reason.is_none());
        assert_eq!(state.chunk_language(), "sv-SE");
    }

    #[test]
    fn catalogue_failure_and_language_mismatch_never_enable_a_fallback() {
        let mut state = Narration::for_languages(&["sv".into()]).with_voice(NarrationVoice::Device);
        state.apply_voices(Ok(vec![DeviceVoice { id: "english".into(), name: "English".into(), language: "en-US".into() }]));
        assert!(state.matching_voices().is_empty());
        assert!(state.device_voice.is_none());
        assert!(state.start(vec![], 0).is_err());
        state.apply_voices(Err("Engine unavailable".into()));
        assert_eq!(state.unavailable_reason.as_deref(), Some("Engine unavailable"));
        assert!(state.device_voices.is_empty());
    }

    #[test]
    fn highlighting_changes_once_per_item_and_restarts_for_new_generations() {
        let mut state = Narration { generation: 7, current: Some(2), status: "Preparing text to speech…".into(), ..Default::default() };
        assert!(state.started_item(2), "the first chunk must highlight even though start already selected this item");
        assert_eq!(state.status, "Reading");
        assert!(!state.started_item(2), "later chunks must not repeat whole-item highlighting");
        assert!(state.started_item(3));
        state.generation = 8;
        state.paused = true;
        assert!(state.started_item(3), "a new generation follows the item again");
        assert_eq!(state.status, "Paused");
        assert!(!state.started_item(3));
    }

    struct PanelReader {
        tts: Narration,
    }

    impl TtsReader for PanelReader {
        fn narration(&self) -> &Narration {
            &self.tts
        }
        fn narration_mut(&mut self) -> &mut Narration {
            &mut self.tts
        }
        fn chapter_key(&self, _: &App) -> usize {
            0
        }
        fn skip_tts_element(&mut self, _: bool, _: &mut Context<Self>) {}
        fn start_tts(&mut self, _: &mut Context<Self>) {
            self.tts.playing = true;
            self.tts.paused = false;
        }
        fn toggle_tts_pause(&mut self, _: &mut Context<Self>) {
            if self.tts.paused {
                self.tts.resume();
            } else {
                self.tts.pause();
            }
        }
        fn stop_tts(&mut self, _: &mut Context<Self>) {
            self.tts.stop();
        }
        fn speech_event(&mut self, _: SpeechEvent, _: &mut Context<Self>) {}
    }

    #[gpui::test]
    fn notification_controls_update_the_dock_and_reject_old_audio(cx: &mut TestAppContext) {
        let reader = cx.new(|_| PanelReader { tts: Narration { generation: 7, playing: true, ..Default::default() } });
        reader.update(cx, |reader, cx| {
            reader.remote_control(SpeechControl::Pause, cx);
            assert!(reader.tts.paused);
            reader.remote_control(SpeechControl::Resume, cx);
            assert!(!reader.tts.paused);
            reader.remote_control(SpeechControl::Item { generation: 8, index: 2 }, cx);
            assert_eq!(reader.tts.current, Some(2));
            assert!(!reader.tts.accepts(&SpeechEvent::Started { generation: 7, index: 0, glyph_ranges: vec![0..1] }));
            assert!(reader.tts.accepts(&SpeechEvent::Started { generation: 8, index: 0, glyph_ranges: vec![0..1] }));
            reader.remote_control(SpeechControl::Stop, cx);
            assert!(!reader.tts.playing);
            assert!(!reader.tts.accepts(&SpeechEvent::Started { generation: 8, index: 0, glyph_ranges: vec![0..1] }));
        });
    }

    #[gpui::test]
    fn drawing_the_dock_while_the_reader_is_borrowed_does_not_panic(cx: &mut TestAppContext) {
        let reader = cx.new(|_| PanelReader { tts: Narration::default() });
        reader.update(cx, |reader, cx| {
            let _dock = dock(&reader.tts, false, components::BrowserTheme::default(), cx);
            assert!(reader.tts.service.is_none(), "drawing the dock must not start synthesis");
            reader.tts = Narration::for_languages(&["sv".into()]);
            let _dock = dock(&reader.tts, true, components::BrowserTheme::default(), cx);
            assert!(reader.tts.service.is_none());
        });
    }

    #[gpui::test]
    fn dock_stays_when_paused_or_stopped_until_closed(cx: &mut TestAppContext) {
        let reader = cx.new(|_| PanelReader { tts: Narration::default() });
        reader.update(cx, |reader, cx| {
            reader.press_tts(cx);
            assert!(reader.tts.dock_open && reader.tts.playing);
            reader.play_pause_tts(cx);
            assert!(reader.tts.dock_open && reader.tts.paused);
            reader.play_pause_tts(cx);
            assert!(reader.tts.dock_open && !reader.tts.paused);
            reader.stop_tts(cx);
            assert!(reader.tts.dock_open && !reader.tts.playing);
            reader.close_tts_dock(cx);
            assert!(!reader.tts.dock_open && !reader.tts.playing);
        });
    }

    #[gpui::test]
    fn playback_key_only_toggles_an_active_narration_session(cx: &mut TestAppContext) {
        let reader = cx.new(|_| PanelReader { tts: Narration::default() });
        reader.update(cx, |reader, cx| {
            assert!(!reader.toggle_active_narration(cx));
            assert!(!reader.tts.playing && !reader.tts.dock_open);
            reader.press_tts(cx);
            assert!(reader.toggle_active_narration(cx));
            assert!(reader.tts.playing && reader.tts.paused && reader.tts.dock_open);
            assert!(reader.toggle_active_narration(cx));
            assert!(reader.tts.playing && !reader.tts.paused);
            reader.stop_tts(cx);
            assert!(!reader.toggle_active_narration(cx));
        });
    }

    #[gpui::test]
    fn navigation_tts_button_closes_playing_and_paused_sessions_and_cancels_work(cx: &mut TestAppContext) {
        let reader = cx.new(|_| PanelReader { tts: Narration::default() });
        for paused in [false, true] {
            reader.update(cx, |reader, cx| {
                reader.press_tts(cx);
                if paused {
                    reader.play_pause_tts(cx);
                }
                choose_sleep(reader, SleepChoice::Minutes(5), cx);
                reader.tts.navigation_task = Some(cx.spawn(async |_, _| std::future::pending().await));
                reader.tts.events_task = Some(cx.spawn(async |_, _| std::future::pending().await));
                reader.press_tts(cx);
                assert!(!reader.tts.dock_open && !reader.tts.playing && !reader.tts.paused);
                assert!(reader.tts.service.is_none() && reader.tts.events_task.is_none());
                assert!(reader.tts.navigation_task.is_none() && reader.tts.sleep_task.is_none() && reader.tts.sleep.is_none());
                assert!(!reader.tts.accepts(&SpeechEvent::Finished { generation: reader.tts.generation }));
            });
        }
    }

    #[gpui::test]
    fn closing_reader_cancels_sleep_navigation_and_event_work(cx: &mut TestAppContext) {
        let reader = cx.new(|_| PanelReader { tts: Narration { playing: true, paused: true, needs_restart: true, ..Default::default() } });
        reader.update(cx, |reader, cx| {
            choose_sleep(reader, SleepChoice::Minutes(5), cx);
            assert!(reader.tts.sleep_task.is_some());
            reader.tts.navigation_task = Some(cx.spawn(async |_, _| std::future::pending().await));
            reader.tts.events_task = Some(cx.spawn(async |_, _| std::future::pending().await));
            reader.tts.close();
            assert!(!reader.tts.playing && !reader.tts.paused && !reader.tts.needs_restart);
            assert!(reader.tts.service.is_none());
            assert!(reader.tts.sleep_task.is_none() && reader.tts.sleep.is_none());
            assert!(reader.tts.navigation_task.is_none() && reader.tts.events_task.is_none());
            assert!(!reader.tts.accepts(&SpeechEvent::Finished { generation: reader.tts.generation }));
        });
    }

    #[test]
    fn item_skips_stop_at_the_batch_boundaries() {
        assert_eq!(adjacent_item(1, 3, true), Some(2));
        assert_eq!(adjacent_item(1, 3, false), Some(0));
        assert_eq!(adjacent_item(0, 3, false), None);
        assert_eq!(adjacent_item(2, 3, true), None);
        assert_eq!(adjacent_item(0, 0, true), None);
    }

    #[gpui::test]
    fn sleep_pauses_at_the_current_element_and_can_be_cancelled(cx: &mut TestAppContext) {
        let reader = cx.new(|_| PanelReader { tts: Narration { playing: true, current: Some(4), ..Default::default() } });
        reader.update(cx, |reader, cx| {
            choose_sleep(reader, SleepChoice::EndOfChapter, cx);
            assert_eq!(reader.tts.sleep.and_then(SleepPlan::chapter), Some(0));
            choose_sleep(reader, SleepChoice::Minutes(5), cx);
            assert!(reader.tts.sleep_task.is_some() && reader.tts.sleep.and_then(SleepPlan::deadline).is_some());
            choose_sleep(reader, SleepChoice::Off, cx);
            assert!(reader.tts.sleep.is_none() && reader.tts.sleep_task.is_none());
            reader.sleep_tts(cx);
            assert!(reader.tts.playing && reader.tts.paused, "sleep must pause, keeping the position available to resume");
            assert_eq!(reader.tts.current, Some(4));
            reader.tts.close();
            assert!(!reader.tts.playing);
        });
    }

    #[test]
    fn unsupported_language_cannot_start_the_speech_worker() {
        let mut narration = Narration::for_languages(&["sv".into()]);
        assert!(narration.start(vec![SpeechPassage::new("A passage", true)], 0).is_err());
        assert!(narration.service.is_none());
        assert!(!narration.playing);
        assert!(Narration::for_languages(&[]).unavailable_reason.is_none());
        assert!(Narration::for_languages(&["en-GB".into()]).unavailable_reason.is_none());
    }

    #[test]
    fn stale_playback_events_cannot_advance_a_new_page_or_a_stopped_session() {
        let mut narration = Narration { generation: 5, playing: true, ..Default::default() };
        assert!(!narration.accepts(&SpeechEvent::Started { generation: 4, index: 0, glyph_ranges: vec![0..1] }));
        assert!(!narration.accepts(&SpeechEvent::Finished { generation: 4 }));
        assert!(narration.accepts(&SpeechEvent::Started { generation: 5, index: 0, glyph_ranges: vec![0..1] }));
        narration.pause();
        narration.navigated();
        assert!(narration.paused && narration.needs_restart, "navigation must not resume a paused session");
        narration.stop();
        assert!(!narration.accepts(&SpeechEvent::Started { generation: 5, index: 0, glyph_ranges: vec![0..1] }));
    }
}
