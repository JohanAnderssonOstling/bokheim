use std::time::Duration;

use audiobook_library::AudiobookChapter;

pub(crate) trait PlaybackBackend {
    fn position(&self) -> Duration;
    fn is_playing(&self) -> bool;
    fn toggle(&self) -> Result<(), String>;
    fn seek_to(&self, position: Duration) -> Result<(), String>;
    fn set_tempo(&mut self, tempo: f32) -> Result<(), String>;
    fn take_error(&self) -> Option<String>;
}

pub(crate) struct PlaybackController {
    backend: Box<dyn PlaybackBackend>,
    duration: Duration,
    chapters: Vec<AudiobookChapter>,
    position: Duration,
    rate: f64,
}

impl PlaybackController {
    pub(crate) fn new(duration: Duration, chapters: Vec<AudiobookChapter>, backend: impl PlaybackBackend + 'static) -> Self {
        Self { backend: Box::new(backend), duration, chapters, position: Duration::ZERO, rate: 1.0 }
    }

    pub(crate) fn position(&self) -> Duration {
        self.position
    }

    pub(crate) fn rate(&self) -> f64 {
        self.rate
    }

    pub(crate) fn is_playing(&self) -> bool {
        self.backend.is_playing()
    }

    pub(crate) fn active_chapter_index(&self) -> usize {
        active_chapter_index(&self.chapters, self.position)
    }

    pub(crate) fn refresh(&mut self) -> Option<String> {
        self.position = self.backend.position().min(self.duration);
        self.backend.take_error()
    }

    pub(crate) fn toggle(&mut self) -> Result<(), String> {
        self.backend.toggle()
    }

    pub(crate) fn seek_to(&mut self, position: Duration) -> Result<(), String> {
        let position = position.min(self.duration);
        self.backend.seek_to(position)?;
        self.position = position;
        Ok(())
    }

    pub(crate) fn seek_by(&mut self, seconds: f64) -> Result<(), String> {
        self.seek_to(relative_seek_position(self.position, seconds, self.duration))
    }

    pub(crate) fn previous_chapter(&mut self) -> Result<(), String> {
        self.seek_to(previous_chapter_position(&self.chapters, self.position))
    }

    pub(crate) fn next_chapter(&mut self) -> Result<(), String> {
        self.seek_to(next_chapter_position(&self.chapters, self.position, self.duration))
    }

    pub(crate) fn set_rate(&mut self, rate: f64) -> Result<(), String> {
        if !rate.is_finite() {
            return Err("playback rate must be finite".to_owned());
        }
        self.backend.set_tempo(rate as f32)?;
        self.rate = rate;
        Ok(())
    }
}

fn active_chapter_index(chapters: &[AudiobookChapter], position: Duration) -> usize {
    chapters.iter().rposition(|chapter| chapter.start <= position).unwrap_or(0)
}

fn previous_chapter_position(chapters: &[AudiobookChapter], position: Duration) -> Duration {
    let index = active_chapter_index(chapters, position);
    let chapter_start = chapters.get(index).map(|chapter| chapter.start).unwrap_or_default();
    if position.saturating_sub(chapter_start) > Duration::from_secs(3) { chapter_start } else { chapters.get(index.saturating_sub(1)).map(|chapter| chapter.start).unwrap_or_default() }
}

fn next_chapter_position(chapters: &[AudiobookChapter], position: Duration, duration: Duration) -> Duration {
    chapters.get(active_chapter_index(chapters, position) + 1).map(|chapter| chapter.start).unwrap_or(duration)
}

fn relative_seek_position(position: Duration, seconds: f64, duration: Duration) -> Duration {
    if !seconds.is_finite() {
        return position.min(duration);
    }
    Duration::from_secs_f64((position.as_secs_f64() + seconds).clamp(0.0, duration.as_secs_f64()))
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use super::*;

    #[derive(Default)]
    struct FakeState {
        position: Cell<Duration>,
        playing: Cell<bool>,
        seeks: RefCell<Vec<Duration>>,
        rates: RefCell<Vec<f32>>,
        toggle_error: RefCell<Option<String>>,
        seek_error: RefCell<Option<String>>,
        rate_error: RefCell<Option<String>>,
        playback_error: RefCell<Option<String>>,
    }

    struct FakeBackend(Rc<FakeState>);

    impl PlaybackBackend for FakeBackend {
        fn position(&self) -> Duration {
            self.0.position.get()
        }

        fn is_playing(&self) -> bool {
            self.0.playing.get()
        }

        fn toggle(&self) -> Result<(), String> {
            if let Some(error) = self.0.toggle_error.borrow().clone() {
                return Err(error);
            }
            self.0.playing.set(!self.0.playing.get());
            Ok(())
        }

        fn seek_to(&self, position: Duration) -> Result<(), String> {
            if let Some(error) = self.0.seek_error.borrow().clone() {
                return Err(error);
            }
            self.0.seeks.borrow_mut().push(position);
            self.0.position.set(position);
            Ok(())
        }

        fn set_tempo(&mut self, tempo: f32) -> Result<(), String> {
            if let Some(error) = self.0.rate_error.borrow().clone() {
                return Err(error);
            }
            self.0.rates.borrow_mut().push(tempo);
            Ok(())
        }

        fn take_error(&self) -> Option<String> {
            self.0.playback_error.borrow_mut().take()
        }
    }

    fn chapter(title: &str, start: u64, end: u64) -> AudiobookChapter {
        AudiobookChapter { title: title.to_owned(), start: Duration::from_secs(start), end: Duration::from_secs(end) }
    }

    fn controller() -> (PlaybackController, Rc<FakeState>) {
        let state = Rc::new(FakeState::default());
        let chapters = vec![chapter("One", 0, 10), chapter("Two", 10, 30), chapter("Three", 30, 60)];
        (PlaybackController::new(Duration::from_secs(60), chapters, FakeBackend(state.clone())), state)
    }

    #[test]
    fn repeated_chapter_selection_keeps_latest_selection() {
        let (mut controller, state) = controller();

        for target in [30, 10, 30, 0, 10] {
            controller.seek_to(Duration::from_secs(target)).unwrap();
        }

        assert_eq!(controller.position(), Duration::from_secs(10));
        assert_eq!(controller.active_chapter_index(), 1);
        assert_eq!(*state.seeks.borrow(), [30, 10, 30, 0, 10].map(Duration::from_secs));
    }

    #[test]
    fn failed_seek_does_not_change_controller_position() {
        let (mut controller, state) = controller();
        controller.seek_to(Duration::from_secs(10)).unwrap();
        *state.seek_error.borrow_mut() = Some("seek failed".to_owned());

        assert_eq!(controller.seek_to(Duration::from_secs(30)), Err("seek failed".to_owned()));
        assert_eq!(controller.position(), Duration::from_secs(10));
        assert_eq!(controller.active_chapter_index(), 1);
    }

    #[test]
    fn refresh_clamps_backend_position_and_forwards_errors_once() {
        let (mut controller, state) = controller();
        state.position.set(Duration::from_secs(90));
        *state.playback_error.borrow_mut() = Some("decoder failed".to_owned());

        assert_eq!(controller.refresh(), Some("decoder failed".to_owned()));
        assert_eq!(controller.position(), Duration::from_secs(60));
        assert_eq!(controller.refresh(), None);
    }

    #[test]
    fn navigation_and_relative_seeking_use_book_boundaries() {
        let (mut controller, state) = controller();

        controller.seek_to(Duration::from_secs(35)).unwrap();
        controller.previous_chapter().unwrap();
        controller.previous_chapter().unwrap();
        controller.next_chapter().unwrap();
        controller.seek_by(100.0).unwrap();

        assert_eq!(*state.seeks.borrow(), [35, 30, 10, 30, 60].map(Duration::from_secs));
        assert_eq!(controller.position(), Duration::from_secs(60));
    }

    #[test]
    fn failed_rate_change_preserves_displayed_rate() {
        let (mut controller, state) = controller();
        controller.set_rate(1.5).unwrap();
        *state.rate_error.borrow_mut() = Some("rate failed".to_owned());

        assert_eq!(controller.set_rate(2.0), Err("rate failed".to_owned()));
        assert_eq!(controller.rate(), 1.5);
        assert_eq!(*state.rates.borrow(), [1.5]);
    }

    #[test]
    fn toggle_state_is_observable_without_ui() {
        let (mut controller, _) = controller();

        assert!(!controller.is_playing());
        controller.toggle().unwrap();
        assert!(controller.is_playing());
        controller.toggle().unwrap();
        assert!(!controller.is_playing());
    }

    #[test]
    fn navigation_helpers_cover_empty_and_non_finite_inputs() {
        assert_eq!(active_chapter_index(&[], Duration::from_secs(10)), 0);
        assert_eq!(previous_chapter_position(&[], Duration::from_secs(10)), Duration::ZERO);
        assert_eq!(next_chapter_position(&[], Duration::from_secs(10), Duration::from_secs(60)), Duration::from_secs(60));
        assert_eq!(relative_seek_position(Duration::from_secs(20), f64::NAN, Duration::from_secs(60)), Duration::from_secs(20));
    }

    #[test]
    fn controller_starts_with_a_consistent_model() {
        let (controller, state) = controller();

        assert_eq!(controller.position(), Duration::ZERO);
        assert_eq!(controller.rate(), 1.0);
        assert_eq!(controller.active_chapter_index(), 0);
        assert!(!controller.is_playing());
        assert!(state.seeks.borrow().is_empty());
        assert!(state.rates.borrow().is_empty());
    }

    #[test]
    fn absolute_seeks_are_clamped_to_the_book_duration() {
        let (mut controller, state) = controller();

        controller.seek_to(Duration::from_secs(600)).unwrap();

        assert_eq!(controller.position(), Duration::from_secs(60));
        assert_eq!(*state.seeks.borrow(), [Duration::from_secs(60)]);
    }

    #[test]
    fn chapter_boundaries_select_the_chapter_that_starts_there() {
        let (mut controller, _) = controller();

        for (second, expected_index) in [(0, 0), (9, 0), (10, 1), (29, 1), (30, 2), (60, 2)] {
            controller.seek_to(Duration::from_secs(second)).unwrap();
            assert_eq!(controller.active_chapter_index(), expected_index, "at {second}s");
        }
    }

    #[test]
    fn relative_seeking_clamps_in_both_directions() {
        let (mut controller, state) = controller();
        controller.seek_to(Duration::from_secs(20)).unwrap();

        controller.seek_by(-100.0).unwrap();
        controller.seek_by(1_000.0).unwrap();

        assert_eq!(*state.seeks.borrow(), [20, 0, 60].map(Duration::from_secs));
    }

    #[test]
    fn non_finite_relative_seeks_leave_the_position_unchanged() {
        let (mut controller, state) = controller();
        controller.seek_to(Duration::from_secs(20)).unwrap();

        for offset in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            controller.seek_by(offset).unwrap();
        }

        assert_eq!(controller.position(), Duration::from_secs(20));
        assert_eq!(*state.seeks.borrow(), [20, 20, 20, 20].map(Duration::from_secs));
    }

    #[test]
    fn previous_chapter_uses_the_three_second_restart_threshold() {
        let (mut controller, state) = controller();

        controller.seek_to(Duration::from_secs(13)).unwrap();
        controller.previous_chapter().unwrap();
        controller.seek_to(Duration::from_millis(13_001)).unwrap();
        controller.previous_chapter().unwrap();

        assert_eq!(*state.seeks.borrow(), [Duration::from_secs(13), Duration::ZERO, Duration::from_millis(13_001), Duration::from_secs(10)]);
    }

    #[test]
    fn next_chapter_at_the_end_stays_at_the_end() {
        let (mut controller, state) = controller();
        controller.seek_to(Duration::from_secs(60)).unwrap();

        controller.next_chapter().unwrap();
        controller.next_chapter().unwrap();

        assert_eq!(*state.seeks.borrow(), [60, 60, 60].map(Duration::from_secs));
        assert_eq!(controller.position(), Duration::from_secs(60));
    }

    #[test]
    fn failed_toggle_does_not_change_playback_state() {
        let (mut controller, state) = controller();
        *state.toggle_error.borrow_mut() = Some("output unavailable".to_owned());

        assert_eq!(controller.toggle(), Err("output unavailable".to_owned()));
        assert!(!controller.is_playing());
    }

    #[test]
    fn successful_rate_changes_are_applied_in_order() {
        let (mut controller, state) = controller();

        for rate in [0.5, 1.0, 1.25, 2.0] {
            controller.set_rate(rate).unwrap();
        }

        assert_eq!(controller.rate(), 2.0);
        assert_eq!(*state.rates.borrow(), [0.5, 1.0, 1.25, 2.0]);
    }

    #[test]
    fn non_finite_rates_are_rejected_before_reaching_the_backend() {
        let (mut controller, state) = controller();

        for rate in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(controller.set_rate(rate), Err("playback rate must be finite".to_owned()));
        }

        assert_eq!(controller.rate(), 1.0);
        assert!(state.rates.borrow().is_empty());
    }

    #[test]
    fn refresh_tracks_backward_moves_and_chapter_changes() {
        let (mut controller, state) = controller();

        state.position.set(Duration::from_secs(35));
        controller.refresh();
        assert_eq!(controller.active_chapter_index(), 2);

        state.position.set(Duration::from_secs(5));
        controller.refresh();
        assert_eq!(controller.position(), Duration::from_secs(5));
        assert_eq!(controller.active_chapter_index(), 0);
    }

    #[test]
    fn zero_duration_book_cannot_seek_past_zero() {
        let state = Rc::new(FakeState::default());
        let mut controller = PlaybackController::new(Duration::ZERO, Vec::new(), FakeBackend(state.clone()));

        controller.seek_to(Duration::MAX).unwrap();
        controller.seek_by(10.0).unwrap();
        controller.next_chapter().unwrap();

        assert_eq!(controller.position(), Duration::ZERO);
        assert_eq!(*state.seeks.borrow(), [Duration::ZERO; 3]);
    }
}
