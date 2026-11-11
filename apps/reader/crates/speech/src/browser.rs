//! Adapter for the browser's Web Speech synthesis service.
use crate::SpeechError;
use crate::{DeviceVoice, FIRST_CHUNK_CHARS, MAX_CHUNK_CHARS, Pauses, SpeechChunk, SpeechControl, SpeechEvent, SpeechFailure, SpeechPassage, next_chunk_with_language};
use async_channel::{Receiver, bounded, unbounded};
use serde::Deserialize;
use serde_json::json;
use std::cell::RefCell;
use wasm_bindgen::{closure::Closure, prelude::*};

#[wasm_bindgen(module = "/src/browser.js")]
extern "C" {
    pub fn available() -> bool;
    #[wasm_bindgen(js_name = deviceVoices)]
    fn browser_device_voices() -> String;
    #[wasm_bindgen(js_name = supportsLanguages)]
    fn browser_supports_languages(languages: &str) -> bool;

    type BrowserVoices;
    #[wasm_bindgen(constructor)]
    fn new(changed: &js_sys::Function) -> BrowserVoices;
    #[wasm_bindgen(method)]
    fn dispose(this: &BrowserVoices);

    type BrowserSpeech;
    #[wasm_bindgen(constructor, catch)]
    fn new(event: &js_sys::Function) -> Result<BrowserSpeech, JsValue>;
    #[wasm_bindgen(method, js_name = readItems)]
    fn read_items(this: &BrowserSpeech, count: usize, index: usize, glyph: u32) -> f64;
    #[wasm_bindgen(method, js_name = replacePassages)]
    fn replace_passages(this: &BrowserSpeech, passages: &str, count: usize, index: usize) -> f64;
    #[wasm_bindgen(method, js_name = provideChunk)]
    fn provide_chunk(this: &BrowserSpeech, generation: f64, index: usize, glyph: u32, chunk: &str);
    #[wasm_bindgen(method, js_name = skipTo)]
    fn skip_to(this: &BrowserSpeech, index: usize) -> f64;
    #[wasm_bindgen(method, js_name = setEndFragment)]
    fn set_end_fragment(this: &BrowserSpeech, end: Option<usize>) -> f64;
    #[wasm_bindgen(method, js_name = setRate)]
    fn set_rate(this: &BrowserSpeech, rate: f32) -> f64;
    #[wasm_bindgen(method, js_name = setPauses)]
    fn set_pauses(this: &BrowserSpeech, scale: f32, gap: f32) -> f64;
    #[wasm_bindgen(method, catch, js_name = setDeviceVoice)]
    fn set_device_voice(this: &BrowserSpeech, id: Option<String>) -> Result<f64, JsValue>;
    #[wasm_bindgen(method, js_name = setLanguages)]
    fn set_languages(this: &BrowserSpeech, languages: &str);
    #[wasm_bindgen(method, js_name = isCurrent)]
    fn is_current(this: &BrowserSpeech) -> bool;
    #[wasm_bindgen(method)]
    fn cancel(this: &BrowserSpeech) -> f64;
    #[wasm_bindgen(method)]
    fn stop(this: &BrowserSpeech);
    #[wasm_bindgen(method)]
    fn pause(this: &BrowserSpeech);
    #[wasm_bindgen(method)]
    fn resume(this: &BrowserSpeech);
    #[wasm_bindgen(method)]
    fn dispose(this: &BrowserSpeech);
}

pub fn device_voices() -> Result<Vec<DeviceVoice>, SpeechError> {
    serde_json::from_str(&browser_device_voices()).map_err(|_| "Could not read installed local voices. Try opening the voice picker again.".into())
}

pub fn supports_languages(languages: &[String]) -> bool {
    browser_supports_languages(&json!(languages).to_string())
}

/// Retains a listener for voices that arrive after the reader opens.
pub struct AvailabilityWatch {
    browser: BrowserVoices,
    _changed: Closure<dyn FnMut()>,
}

impl AvailabilityWatch {
    pub fn new() -> (Self, Receiver<()>) {
        let (sender, events) = bounded(1);
        let changed = Closure::own_aborting(move || {
            let _ = sender.try_send(());
        });
        let browser = BrowserVoices::new(changed.as_ref().unchecked_ref());
        (Self { browser, _changed: changed }, events)
    }
}

impl Drop for AvailabilityWatch {
    fn drop(&mut self) {
        self.browser.dispose();
    }
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum BrowserEvent {
    Requested { generation: u64, index: usize, glyph_start: u32, max_chars: usize },
    Started { generation: u64, index: usize, glyph_ranges: Vec<[u32; 2]> },
    Finished { generation: u64 },
    Failed { generation: u64, failure: SpeechFailure, message: SpeechError },
    Stopped { generation: u64 },
}

/// Uses the same reader endpoint contract as native narration.
pub struct DesktopSpeech {
    browser: BrowserSpeech,
    languages: RefCell<Vec<String>>,
    events: Receiver<SpeechEvent>,
    _event: Closure<dyn FnMut(JsValue)>,
}

impl DesktopSpeech {
    pub fn new() -> Result<Self, SpeechError> {
        let (sender, events) = unbounded();
        let event = Closure::own_aborting(move |event: JsValue| {
            let parsed = js_sys::JSON::stringify(&event).ok().and_then(|value| value.as_string()).and_then(|value| serde_json::from_str::<BrowserEvent>(&value).ok());
            let event = match parsed {
                Some(BrowserEvent::Requested { generation, index, glyph_start, max_chars }) => SpeechEvent::Requested { generation, index, glyph_start, max_chars },
                Some(BrowserEvent::Started { generation, index, glyph_ranges }) => SpeechEvent::Started { generation, index, glyph_ranges: glyph_ranges.into_iter().map(|[start, end]| start..end).collect() },
                Some(BrowserEvent::Finished { generation }) => SpeechEvent::Finished { generation },
                Some(BrowserEvent::Failed { generation, failure, message }) => SpeechEvent::Failed { generation, failure, message },
                Some(BrowserEvent::Stopped { generation }) => SpeechEvent::Remote { generation, command: SpeechControl::Stop },
                None => return,
            };
            let _ = sender.try_send(event);
        });
        let browser = BrowserSpeech::new(event.as_ref().unchecked_ref()).map_err(SpeechError::from)?;
        Ok(Self { browser, languages: RefCell::new(Vec::new()), events, _event: event })
    }

    pub fn events(&self) -> Receiver<SpeechEvent> {
        self.events.clone()
    }
    pub fn set_title(&self, _title: &str) {}
    pub fn set_media_metadata(&self, _author: &str, _chapter: &str, _cover: &[u8]) {}
    pub fn set_device_voice(&self, id: Option<&str>) -> Result<u64, SpeechError> {
        self.browser.set_device_voice(id.map(str::to_owned)).map(|generation| generation as u64).map_err(|_| "The selected installed local voice is unavailable for this book’s language. Choose another local voice.".into())
    }
    pub fn set_languages(&self, languages: &[String]) {
        *self.languages.borrow_mut() = languages.to_vec();
        self.browser.set_languages(&json!(languages).to_string());
    }

    pub fn replace_passages_from(&self, passages: Vec<SpeechPassage>, start: usize) -> u64 {
        let count = passages.len();
        let mut chunks = Vec::new();
        let language = self.languages.borrow().first().cloned().unwrap_or_else(|| "und".into());
        for (index, passage) in passages.iter().enumerate() {
            let mut offset = 0;
            while let Some(span) = next_chunk_with_language(passage.text(), offset, if index == start && offset == 0 { FIRST_CHUNK_CHARS } else { MAX_CHUNK_CHARS }, &language) {
                offset = span.end;
                let ends_block = passage.ends_block() && passage.text()[offset..].trim().is_empty();
                chunks.push(json!({ "index": index, "text": &passage.text()[span], "ends_block": ends_block }));
            }
        }
        self.browser.replace_passages(&json!(chunks).to_string(), count, start) as u64
    }

    pub fn read_items_from(&self, count: usize, start: usize, glyph_start: u32) -> u64 {
        self.browser.read_items(count, start, glyph_start) as u64
    }

    pub fn provide_chunk(&self, generation: u64, index: usize, glyph_start: u32, chunk: Option<SpeechChunk>) {
        let chunk = chunk.map(|chunk| {
            json!({
                "text": chunk.text,
                "glyph_ranges": chunk.glyph_ranges.into_iter().map(|range| [range.start, range.end]).collect::<Vec<_>>(),
                "next_glyph": chunk.next_glyph,
                "ends_block": chunk.next_glyph.is_none(),
            })
        });
        self.browser.provide_chunk(generation as f64, index, glyph_start, &json!(chunk).to_string());
    }

    pub fn skip_to(&self, index: usize) -> u64 {
        self.browser.skip_to(index) as u64
    }
    pub fn set_end_fragment(&self, end: Option<usize>) -> u64 {
        self.browser.set_end_fragment(end) as u64
    }
    pub fn set_rate(&self, rate: f32) -> u64 {
        self.browser.set_rate(rate) as u64
    }
    pub fn set_pauses(&self, pauses: Pauses) -> u64 {
        self.browser.set_pauses(pauses.sentence_scale, pauses.passage_gap) as u64
    }
    pub fn cancel(&self) -> u64 {
        self.browser.cancel() as u64
    }
    pub fn stop(&self) {
        self.browser.stop();
    }
    pub fn pause(&self) {
        self.browser.pause();
    }
    pub fn resume(&self) {
        self.browser.resume();
    }
    pub fn is_current(&self) -> bool {
        self.browser.is_current()
    }
}

impl Drop for DesktopSpeech {
    fn drop(&mut self) {
        self.browser.dispose();
    }
}
