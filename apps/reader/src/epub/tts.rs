use crate::epub::{LoadState, ReaderView};
use crate::shell::tts::{self, TtsReader};
use gpui::{App, Context};
use html_view_core::cfi::{parse_cfi_spine_only, source_position_cfi};
use html_view_core::{ReadingTarget, RendererCommand};
use reader_speech::{SpeechChunk, SpeechControl, SpeechEvent, next_chunk_with_limit};
use std::time::Duration;

impl ReaderView {
    pub(super) fn tts_document_failed(&mut self, message: &str, cx: &mut Context<Self>) {
        if self.tts.playing && (self.tts.needs_restart || self.tts.finished) {
            self.stop_tts(cx);
            self.tts.status = format!("Could not load the next passage: {message}");
        }
    }

    pub(super) fn tts_navigation_changed(&mut self, cx: &mut Context<Self>) {
        let LoadState::Ready(view) = &self.load_state else { return };
        if view.read(cx).navigation_revision() == self.tts_navigation_revision {
            return;
        }
        self.tts_navigation_revision = view.read(cx).navigation_revision();
        if !self.tts.playing {
            return;
        }
        self.tts.navigated();
        self.with_renderer(cx, |view, cx| view.apply(RendererCommand::SetReadingTarget(None), cx));
        if !self.tts.paused {
            self.schedule_tts(cx);
        }
        cx.notify();
    }

    fn schedule_tts(&mut self, cx: &mut Context<Self>) {
        let timer = cx.background_executor().timer(Duration::from_millis(100));
        self.tts.navigation_task = Some(cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |reader, cx| {
                if reader.tts.playing && !reader.tts.paused {
                    if reader.tts.finished {
                        let ready = match &reader.load_state {
                            LoadState::Ready(view) => view.read(cx).reading_document(),
                            _ => None,
                        };
                        match ready {
                            Some(doc) if doc == reader.tts_doc => reader.advance_tts_document(cx),
                            Some(_) => reader.start_tts(cx),
                            None => reader.schedule_tts(cx),
                        }
                    } else {
                        reader.start_tts(cx);
                    }
                }
            });
        }));
    }

    fn advance_tts_document(&mut self, cx: &mut Context<Self>) {
        if let LoadState::Ready(view) = &self.load_state
            && view.read(cx).reading_document().is_none()
        {
            self.tts.finished = true;
            self.tts.status = "Waiting for navigation…".into();
            if !self.tts.paused {
                self.schedule_tts(cx);
            }
            return;
        }
        if self.tts_doc + 1 >= self.book.document_uris().len() {
            self.stop_tts(cx);
            self.tts.status = "Finished".into();
            return;
        }
        if self.tts.sleep.and_then(|plan| plan.chapter()).is_some_and(|chapter| self.toc.chapter_for_document(self.tts_doc + 1).unwrap_or(self.tts_doc + 1) > chapter) {
            self.tts.finished = true;
            self.sleep_tts(cx);
            return;
        }
        self.tts.navigated();
        self.tts.status = "Loading the next chapter…".into();
        self.with_renderer(cx, |view, cx| {
            view.apply(RendererCommand::SetReadingTarget(None), cx);
            view.advance_reading_document(cx);
        });
        if !self.tts.paused {
            self.schedule_tts(cx);
        }
    }
}

impl TtsReader for ReaderView {
    fn narration_mut(&mut self) -> &mut tts::Narration {
        &mut self.tts
    }

    fn chapter_key(&self, cx: &App) -> usize {
        let cfi = match &self.load_state {
            LoadState::Ready(view) => self.tts.current.and_then(|index| self.tts_fragments.get(index)).and_then(|fragment| view.read(cx).reading_target_cfi(fragment)).or_else(|| view.read(cx).current_cfi()),
            _ => None,
        };
        let cfi = cfi.or_else(|| self.reading_position.as_ref().map(|position| source_position_cfi(&position.source)));
        let cfi = cfi.as_deref();
        cfi.and_then(|cfi| self.toc.chapter_for_cfi(cfi)).or_else(|| cfi.and_then(parse_cfi_spine_only)).unwrap_or(self.tts_doc)
    }

    fn update_tts_chapter_limit(&mut self, cx: &mut Context<Self>) {
        let end = self.tts.sleep.and_then(|plan| plan.chapter()).and_then(|chapter| {
            let LoadState::Ready(view) = &self.load_state else { return None };
            let view = view.read(cx);
            self.tts_fragments.iter().position(|fragment| view.reading_target_cfi(fragment).as_deref().and_then(|cfi| self.toc.chapter_for_cfi(cfi)).unwrap_or(self.tts_doc) > chapter)
        });
        self.tts.limit_to(end);
    }

    fn skip_tts_element(&mut self, forward: bool, cx: &mut Context<Self>) {
        if !self.tts.playing || self.tts.needs_restart {
            return;
        }
        let Some(index) = tts::adjacent_item(self.tts.current.unwrap_or(0), self.tts_fragments.len(), forward) else { return };
        if self.tts.end_fragment.is_some_and(|end| index >= end) {
            self.sleep_tts(cx);
        }
        self.tts.skip_to(index);
        let target = self.tts_fragments[index].clone();
        self.with_renderer(cx, |view, cx| view.follow_reading_target(target, cx));
    }

    fn start_tts(&mut self, cx: &mut Context<Self>) {
        if self.tts.playing && self.tts.service.as_ref().is_some_and(|service| !service.is_current()) {
            self.stop_tts(cx);
            return;
        }
        if let Some(reason) = self.tts.unavailable_reason.clone() {
            self.stop_tts(cx);
            self.tts.status = reason;
            return;
        }
        let LoadState::Ready(view) = &self.load_state else { return };
        let Some((doc, start, glyph_start, fragments)) = view.read(cx).reading_items() else {
            self.tts.playing = true;
            self.tts.needs_restart = true;
            self.tts.status = "Loading the chapter…".into();
            if !self.tts.paused {
                self.schedule_tts(cx);
            }
            cx.notify();
            return;
        };
        self.tts_doc = doc;
        self.tts_navigation_revision = view.read(cx).navigation_revision();
        self.tts_fragments = fragments;
        if self.tts_fragments.is_empty() {
            self.tts.playing = true;
            self.tts.paused = false;
            self.advance_tts_document(cx);
            cx.notify();
            return;
        }
        match self.tts.start_items(self.tts_fragments.len(), start, glyph_start) {
            Ok(Some(events)) => self.tts.events_task = Some(tts::listen(events, cx)),
            Ok(None) => {}
            Err(error) => {
                self.tts.stop();
                self.tts.status = error;
            }
        }
        self.update_tts_chapter_limit(cx);
        cx.notify();
    }

    fn toggle_tts_pause(&mut self, cx: &mut Context<Self>) {
        if self.tts.paused {
            if self.tts.needs_restart {
                self.tts.paused = false;
                self.start_tts(cx);
            } else if self.tts.finished {
                self.tts.resume();
                self.advance_tts_document(cx);
            } else {
                self.tts.resume();
            }
        } else {
            self.tts.pause();
        }
        cx.notify();
    }

    fn stop_tts(&mut self, cx: &mut Context<Self>) {
        self.tts.stop();
        self.tts_fragments = Vec::new();
        self.with_renderer(cx, |view, cx| view.apply(RendererCommand::SetReadingTarget(None), cx));
        cx.notify();
    }

    fn speech_event(&mut self, event: SpeechEvent, cx: &mut Context<Self>) {
        self.tts_navigation_changed(cx);
        if !self.tts.accepts(&event) {
            return;
        }
        match event {
            SpeechEvent::Remote { command, .. } => {
                let item = match &command {
                    SpeechControl::Item { index, .. } => Some(*index),
                    _ => None,
                };
                self.remote_control(command, cx);
                if let Some(index) = item {
                    if let Some(target) = self.tts_fragments.get(index).cloned() {
                        self.with_renderer(cx, |view, cx| view.follow_reading_target(target, cx));
                    }
                }
            }
            SpeechEvent::Requested { generation, index, glyph_start, max_chars } => {
                if let Some(target) = self.tts_fragments.get(index)
                    && let LoadState::Ready(view) = &self.load_state
                    && let Some(service) = &self.tts.service
                {
                    let chunk = view.read(cx).reading_chunk(target, glyph_start, max_chars, |text| next_chunk_with_limit(text, 0, max_chars)).map(|(fragment, next_glyph)| SpeechChunk {
                        text: fragment.text,
                        glyph_ranges: fragment.target.glyph_ranges,
                        next_glyph,
                    });
                    service.provide_chunk(generation, index, glyph_start, chunk);
                }
                return;
            }
            SpeechEvent::Started { index, glyph_ranges, .. } => {
                self.tts.current = Some(index);
                self.tts.status = if self.tts.paused { "Paused" } else { "Reading" }.into();
                if let Some(fragment) = self.tts_fragments.get(index) {
                    let target = ReadingTarget { element: fragment.element, glyph_ranges };
                    self.with_renderer(cx, |view, cx| view.follow_reading_target(target, cx));
                }
            }
            SpeechEvent::Finished { .. } => {
                if self.tts.end_fragment.is_some() && self.tts.sleep.and_then(|plan| plan.chapter()).is_some() {
                    self.tts.finished = true;
                    self.sleep_tts(cx);
                } else if self.tts.paused {
                    self.tts.finished = true;
                } else {
                    self.advance_tts_document(cx);
                }
            }
            SpeechEvent::Failed { message, .. } => {
                self.stop_tts(cx);
                self.tts.status = message;
            }
        }
        cx.notify();
    }
}
