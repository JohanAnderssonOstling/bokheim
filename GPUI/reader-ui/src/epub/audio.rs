//! Spoken playback: publisher-authored media overlays, and read-aloud when a
//! book has none.
//!
//! Both are polled rather than event-driven — the backends report a transport
//! state, and the reader checks it on a timer. Each playback carries a
//! generation so a poll belonging to a superseded cue stops instead of
//! advancing to the next one.

use epub_provider::MediaOverlayCue;

/// Which of the two forms of audio a book offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AudioMode {
    /// Publisher-authored narration synchronized to the text.
    MediaOverlay,
    /// Synthesized speech, used when the book has no overlay.
    ReadAloud,
}

#[derive(Default)]
pub(crate) struct AudioState {
    /// Narration cues in reading order; empty for books without overlays.
    pub(crate) cues: Vec<MediaOverlayCue>,
    /// The cue currently playing, or the one to resume from.
    pub(crate) cue_index: usize,
    playing: bool,
    paused: bool,
    /// Invalidates polls belonging to a superseded cue.
    overlay_generation: u64,
    speaking: bool,
    /// Invalidates polls belonging to a superseded utterance.
    speech_generation: u64,
}

impl AudioState {
    pub(crate) fn new(cues: Vec<MediaOverlayCue>) -> Self {
        Self { cues, ..Self::default() }
    }

    /// Whichever form of audio this book uses. Overlays win when present:
    /// publisher narration is always better than synthesis.
    pub(crate) fn mode(&self) -> AudioMode {
        if self.cues.is_empty() { AudioMode::ReadAloud } else { AudioMode::MediaOverlay }
    }

    pub(crate) fn is_playing(&self) -> bool {
        self.playing
    }

    pub(crate) fn is_paused(&self) -> bool {
        self.paused
    }

    pub(crate) fn is_speaking(&self) -> bool {
        self.speaking
    }

    /// Starts a cue, returning the generation its polls must carry.
    pub(crate) fn begin_cue(&mut self) -> u64 {
        self.overlay_generation = self.overlay_generation.wrapping_add(1);
        self.overlay_generation
    }

    /// Marks a cue as playing after the backend accepted it.
    pub(crate) fn cue_started(&mut self, index: usize) {
        self.cue_index = index;
        self.playing = true;
        self.paused = false;
    }

    /// Whether a poll for `(index, generation)` still describes what is
    /// playing. A stale poll must not advance to the next cue.
    pub(crate) fn cue_poll_is_current(&self, index: usize, generation: u64) -> bool {
        self.playing && self.overlay_generation == generation && self.cue_index == index
    }

    /// Whether there is another cue after `index`.
    pub(crate) fn has_cue_after(&self, index: usize) -> bool {
        index + 1 < self.cues.len()
    }

    /// Stops overlay playback. Returns whether anything was playing, so a
    /// redundant stop neither touches the backend nor repaints.
    pub(crate) fn stop_overlay(&mut self) -> bool {
        if !self.playing && !self.paused {
            return false;
        }
        self.overlay_generation = self.overlay_generation.wrapping_add(1);
        self.playing = false;
        self.paused = false;
        true
    }

    pub(crate) fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    /// Starts an utterance, returning the generation its polls must carry.
    pub(crate) fn begin_speech(&mut self) -> u64 {
        self.speech_generation = self.speech_generation.wrapping_add(1);
        self.speaking = true;
        self.speech_generation
    }

    pub(crate) fn speech_poll_is_current(&self, generation: u64) -> bool {
        self.speaking && self.speech_generation == generation
    }

    /// Stops speaking. Returns whether it was speaking, so a redundant stop is
    /// a no-op.
    pub(crate) fn stop_speech(&mut self) -> bool {
        if !self.speaking {
            return false;
        }
        self.speech_generation = self.speech_generation.wrapping_add(1);
        self.speaking = false;
        true
    }

    /// Marks speech as finished without invalidating anything, for the poll
    /// that observes the backend going idle on its own.
    pub(crate) fn speech_finished(&mut self) {
        self.speaking = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cue(href: &str) -> MediaOverlayCue {
        MediaOverlayCue { text_href: format!("{href}#text"), audio_href: format!("{href}.mp3"), clip_begin_seconds: 0.0, clip_end_seconds: None }
    }

    #[test]
    fn a_book_without_cues_reads_aloud_instead() {
        assert_eq!(AudioState::default().mode(), AudioMode::ReadAloud);
        assert_eq!(AudioState::new(vec![cue("ch1")]).mode(), AudioMode::MediaOverlay);
    }

    #[test]
    fn a_poll_from_a_superseded_cue_does_not_advance_playback() {
        let mut audio = AudioState::new(vec![cue("ch1"), cue("ch2")]);
        let first = audio.begin_cue();
        audio.cue_started(0);
        assert!(audio.cue_poll_is_current(0, first));

        let second = audio.begin_cue();
        audio.cue_started(1);

        assert!(!audio.cue_poll_is_current(0, first), "the earlier cue's poll must stop");
        assert!(audio.cue_poll_is_current(1, second));
    }

    #[test]
    fn stopping_overlay_playback_twice_reports_work_only_once() {
        let mut audio = AudioState::new(vec![cue("ch1")]);
        audio.begin_cue();
        audio.cue_started(0);

        assert!(audio.stop_overlay());
        assert!(!audio.stop_overlay(), "a redundant stop must not touch the backend");
    }

    #[test]
    fn a_poll_outliving_its_cue_stops_after_a_stop() {
        let mut audio = AudioState::new(vec![cue("ch1"), cue("ch2")]);
        let generation = audio.begin_cue();
        audio.cue_started(0);
        audio.stop_overlay();

        assert!(!audio.cue_poll_is_current(0, generation), "a stop must not leave a poll able to start the next cue");
    }

    #[test]
    fn the_last_cue_has_nothing_after_it() {
        let audio = AudioState::new(vec![cue("ch1"), cue("ch2")]);
        assert!(audio.has_cue_after(0));
        assert!(!audio.has_cue_after(1));
    }

    #[test]
    fn a_new_utterance_supersedes_the_previous_poll() {
        let mut audio = AudioState::default();
        let first = audio.begin_speech();
        let second = audio.begin_speech();

        assert!(!audio.speech_poll_is_current(first));
        assert!(audio.speech_poll_is_current(second));
    }

    #[test]
    fn stopping_speech_twice_reports_work_only_once() {
        let mut audio = AudioState::default();
        audio.begin_speech();

        assert!(audio.stop_speech());
        assert!(!audio.stop_speech());
    }
}
