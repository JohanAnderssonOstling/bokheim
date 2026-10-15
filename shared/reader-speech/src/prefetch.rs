//! Bounded read-ahead for text extracted by the reader's UI thread.
use super::{FIRST_CHUNK_CHARS, MAX_CHUNK_CHARS, SpeechChunk, SpeechEvent, SpeechEvents, SpeechPassage};
use std::collections::VecDeque;

const PREFETCH_CHUNKS: usize = 3;

#[derive(Default)]
pub(super) struct TextPrefetch {
    ready: VecDeque<((usize, u32), Option<SpeechPassage>)>,
    requested: Option<(usize, u32)>,
    next: (usize, u32),
    opening_pending: bool,
}

impl TextPrefetch {
    pub fn reset(&mut self, index: usize, glyph_start: u32) {
        self.ready.clear();
        self.requested = None;
        self.next = (index, glyph_start);
        self.opening_pending = true;
    }

    pub fn has_ready(&self) -> bool {
        !self.ready.is_empty()
    }

    pub fn can_request(&self, end: usize) -> bool {
        self.requested.is_none() && self.ready.len() < PREFETCH_CHUNKS && self.next.0 < end
    }

    pub fn request(&mut self, generation: u64, end: usize, events: &SpeechEvents) {
        if self.can_request(end) {
            let (index, glyph_start) = self.next;
            let max_chars = if self.opening_pending { FIRST_CHUNK_CHARS } else { MAX_CHUNK_CHARS };
            self.requested = Some(self.next);
            let _ = events.try_send(SpeechEvent::Requested { generation, index, glyph_start, max_chars });
        }
    }

    pub fn provide(&mut self, index: usize, glyph_start: u32, chunk: Option<SpeechChunk>) {
        if self.requested != Some((index, glyph_start)) {
            return;
        }
        self.next = match chunk.as_ref().and_then(|chunk| chunk.next_glyph) {
            Some(next) => (index, next),
            _ => (index + 1, 0),
        };
        if chunk.as_ref().is_some_and(|chunk| !chunk.text.trim().is_empty()) {
            self.opening_pending = false;
        }
        self.ready.push_back(((index, glyph_start), chunk.map(SpeechPassage::from_chunk)));
        self.requested = None;
    }

    /// Distinguish a pending response from a reader item with no readable text.
    pub fn take(&mut self, index: usize, glyph_start: u32) -> Option<Option<SpeechPassage>> {
        if self.ready.front().is_some_and(|(position, _)| *position == (index, glyph_start)) { self.ready.pop_front().map(|(_, passage)| passage) } else { None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_read_ahead_is_bounded_and_reset_discards_queued_and_pending_text() {
        let (sender, events) = async_channel::unbounded();
        let sender = SpeechEvents::Direct(sender);
        let mut prefetch = TextPrefetch::default();
        prefetch.reset(0, 0);
        for index in 0..3 {
            prefetch.request(1, 10, &sender);
            let chars = if index == 0 { FIRST_CHUNK_CHARS } else { MAX_CHUNK_CHARS };
            assert!(matches!(events.try_recv().unwrap(), SpeechEvent::Requested { index: requested, max_chars, .. } if requested == index && max_chars == chars));
            prefetch.provide(index, 0, Some(SpeechChunk { text: format!("Sentence {index}."), glyph_ranges: vec![], next_glyph: None }));
        }
        prefetch.request(1, 10, &sender);
        assert!(events.try_recv().is_err(), "do not extract the whole document into a text queue");
        assert_eq!(prefetch.take(0, 0).unwrap().unwrap().text(), "Sentence 0.");
        prefetch.request(1, 10, &sender);
        assert!(matches!(events.try_recv().unwrap(), SpeechEvent::Requested { index: 3, .. }));
        prefetch.reset(8, 50);
        prefetch.provide(3, 0, Some(SpeechChunk { text: "Old request.".into(), glyph_ranges: vec![], next_glyph: None }));
        assert!(!prefetch.has_ready());
        prefetch.request(2, 10, &sender);
        assert!(matches!(events.try_recv().unwrap(), SpeechEvent::Requested { index: 8, glyph_start: 50, max_chars: FIRST_CHUNK_CHARS, .. }));
    }

    #[test]
    fn empty_items_do_not_use_up_the_short_opening_chunk() {
        let (sender, events) = async_channel::unbounded();
        let sender = SpeechEvents::Direct(sender);
        let mut prefetch = TextPrefetch::default();
        prefetch.reset(0, 0);
        prefetch.request(1, 3, &sender);
        events.try_recv().unwrap();
        prefetch.provide(0, 0, None);
        prefetch.request(1, 3, &sender);
        assert!(matches!(events.try_recv().unwrap(), SpeechEvent::Requested { index: 1, max_chars: FIRST_CHUNK_CHARS, .. }));
    }
}
