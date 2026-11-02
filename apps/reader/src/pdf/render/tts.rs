use ui_text::text;

use super::PdfReaderView;
use crate::shell::persistence::StoredPosition;
use crate::shell::tts::{self, TtsReader};
use gpui::{App, Context};
use reader_speech::{SpeechControl, SpeechEvent, SpeechPassage};
use std::time::Duration;
use ui_components::user_facing_error;

impl PdfReaderView {
    pub(super) fn tts_navigation_changed(&mut self, cx: &mut Context<Self>) {
        if !self.tts.is_active() || self.pdf.read(cx).navigation_revision() == self.tts_navigation_revision {
            return;
        }
        self.tts_navigation_revision = self.pdf.read(cx).navigation_revision();
        self.tts_request += 1;
        self.tts.navigated();
        self.pdf.update(cx, |pdf, cx| pdf.set_reading_highlight(None, cx));
        if !self.tts.is_paused() {
            let timer = cx.background_executor().timer(Duration::from_millis(100));
            self.tts.navigate_with(cx.spawn(async move |this, cx| {
                timer.await;
                let _ = this.update(cx, |reader, cx| {
                    if reader.tts.is_running() {
                        reader.start_tts(cx);
                    }
                });
            }));
        }
        cx.notify();
    }

    fn advance_tts_page(&mut self, cx: &mut Context<Self>) {
        if self.tts_page + 1 >= self.page_count {
            self.stop_tts_with_status(text!(reader::finished), cx);
            return;
        }
        if self.tts.sleep_chapter().is_some_and(|chapter| self.toc.chapter_at(self.tts_page + 1) > chapter) {
            self.tts.finish_batch();
            self.sleep_tts(cx);
            return;
        }
        self.tts_page += 1;
        let page = self.tts_page;
        self.pdf.update(cx, |pdf, cx| pdf.advance_reading_page(page, cx));
        self.start_tts(cx);
    }
}

impl TtsReader for PdfReaderView {
    fn narration(&self) -> &tts::Narration {
        &self.tts
    }
    fn narration_mut(&mut self) -> &mut tts::Narration {
        &mut self.tts
    }

    fn chapter_key(&self, cx: &App) -> usize {
        self.toc.chapter_at(self.pdf.read(cx).current_page())
    }

    fn chapter_label(&self, _cx: &App) -> String {
        self.toc.narration_chapter_label(self.tts_page)
    }

    fn skip_tts_element(&mut self, forward: bool, cx: &mut Context<Self>) {
        self.tts_navigation_changed(cx);
        if !self.tts.can_skip() {
            return;
        }
        let Some(index) = tts::adjacent_item(self.tts.current_item().unwrap_or(0), self.tts_fragments.len(), forward) else { return };
        if self.tts.at_limit(index) {
            self.sleep_tts(cx);
        }
        self.tts.skip_to(index);
        let rects = self.tts_fragments[index].clone();
        let page = self.tts_page;
        self.pdf.update(cx, |pdf, cx| pdf.follow_reading_fragment(page, rects, cx));
    }

    fn start_tts(&mut self, cx: &mut Context<Self>) {
        let continuation = self.tts.is_active();
        if continuation && self.tts.lost_playback() {
            self.stop_tts(cx);
            return;
        }
        let current_page = self.pdf.read(cx).current_page();
        let Some(page) = self.toc.narration_start_page(current_page, self.page_count) else {
            self.stop_tts_with_status(text!(reader::finished), cx);
            cx.notify();
            return;
        };
        if page != current_page {
            if self.tts.sleep_chapter().is_some_and(|chapter| self.toc.chapter_at(page) > chapter) {
                self.tts.finish_batch();
                self.sleep_tts(cx);
                cx.notify();
                return;
            }
            self.pdf.update(cx, |pdf, cx| pdf.advance_reading_page(page, cx));
        }
        let pdf = self.pdf.read(cx);
        let position = pdf.full_page_position();
        self.tts_navigation_revision = pdf.navigation_revision();
        let Some(request) = pdf.reading_request(page) else {
            self.tts.wait_for_page(text!(reader::the_document_is_still_loading));
            cx.notify();
            return;
        };
        self.tts_request += 1;
        let revision = self.tts_request;
        self.tts.prepare_page();
        self.tts_page = page;
        self.tts.navigate_with(cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |reader, cx| {
                if revision != reader.tts_request || !reader.tts.is_active() || reader.tts.is_paused() {
                    return;
                }
                if continuation && reader.tts.lost_playback() {
                    reader.stop_tts(cx);
                    return;
                }
                match result {
                    Ok(fragments) => {
                        let start = fragments.iter().position(|fragment| fragment.rects.iter().any(|rect| rect.bottom() >= position)).unwrap_or(fragments.len());
                        if fragments.is_empty() {
                            reader.stop_tts_with_status(text!(reader::no_readable_text_here_scanned_pages_require_ocr), cx);
                        } else {
                            // PDF fragments are geometry-based reading blocks.
                            let (rects, passages) = fragments.into_iter().map(|fragment| (fragment.rects, SpeechPassage::new(fragment.text, true))).unzip();
                            reader.tts_fragments = rects;
                            reader.tts.set_chapter(reader.chapter_label(cx));
                            match reader.tts.start(passages, start) {
                                Ok(Some(events)) => reader.tts.watch_events(events, cx),
                                Ok(None) => {}
                                Err(error) => {
                                    reader.stop_tts_with_status(error.user_message(), cx);
                                }
                            }
                        }
                    }
                    Err(error) => {
                        reader.stop_tts_with_status(user_facing_error(&error.to_string(), text!(reader::couldn_t_read_this_pdf_page_s_text_try_another_page_or)), cx);
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn toggle_tts_pause(&mut self, cx: &mut Context<Self>) {
        match self.tts.toggle_pause() {
            tts::PlaybackContinuation::Restart => self.start_tts(cx),
            tts::PlaybackContinuation::Advance => self.advance_tts_page(cx),
            tts::PlaybackContinuation::None => {}
        }
        cx.notify();
    }

    fn stop_tts(&mut self, cx: &mut Context<Self>) {
        self.tts_request += 1;
        self.tts.stop();
        self.tts_fragments = Vec::new();
        self.pdf.update(cx, |pdf, cx| pdf.set_reading_highlight(None, cx));
        cx.notify();
    }

    fn speech_event(&mut self, event: SpeechEvent, cx: &mut Context<Self>) {
        self.tts_navigation_changed(cx);
        if !self.tts.accepts(&event) {
            return;
        }
        match event {
            SpeechEvent::Requested { .. } => return,
            SpeechEvent::Remote { command, .. } => {
                let item = match &command {
                    SpeechControl::Item { index, .. } => Some(*index),
                    _ => None,
                };
                self.remote_control(command, cx);
                if let Some(index) = item {
                    if let Some(fragment) = self.tts_fragments.get(index) {
                        let rects = fragment.clone();
                        let page = self.tts_page;
                        self.pdf.update(cx, |pdf, cx| pdf.follow_reading_fragment(page, rects, cx));
                    }
                }
            }
            SpeechEvent::Started { index, .. } => {
                if !self.tts.started_item(index) {
                    return;
                }
                if let Some(fragment) = self.tts_fragments.get(index) {
                    let rects = fragment.clone();
                    let page = self.tts_page;
                    if let Some(bounds) = rects.first() {
                        self.positions.record_narration(StoredPosition::pdf_page(page, bounds.top(), self.page_count), self.pdf.read(cx).navigation_revision());
                    }
                    self.pdf.update(cx, |pdf, cx| {
                        pdf.follow_reading_fragment(page, rects, cx);
                    });
                }
            }
            SpeechEvent::Finished { .. } => {
                if self.tts.is_paused() {
                    self.tts.finish_batch();
                } else {
                    self.advance_tts_page(cx);
                }
            }
            SpeechEvent::Failed { failure, message, .. } => {
                self.stop_tts_with_status(tts::speech_error_message(failure, &message), cx);
            }
        }
        cx.notify();
    }
}
