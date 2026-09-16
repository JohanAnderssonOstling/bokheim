use std::cell::Cell;
mod dock;
mod session;
pub use dock::{AudiobookDock, DockAction};
pub use session::{ActiveAudiobook, PlaybackSession};
#[cfg(not(target_arch = "wasm32"))]
use std::io::{Read, Seek, SeekFrom};
#[cfg(not(target_arch = "wasm32"))]
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    App, Bounds, Context, DispatchPhase, Entity, FocusHandle, Focusable, FontWeight, Image, ImageFormat, IntoElement, KeyBinding, MouseButton, MouseMoveEvent, MouseUpEvent, ObjectFit, Pixels, Render, ScrollStrategy, SharedString, Styled,
    StyledImage, UniformListScrollHandle, Window, actions, canvas, div, img, prelude::*, px, relative, uniform_list,
};
use web_time::Instant;

// Stage boundaries also appear on early returns, making failed opens visible
// in the Kobo launcher log without enabling per-sample audio logging.
struct OpeningTiming {
    phase: &'static str,
    started: Instant,
}
impl OpeningTiming {
    fn new(phase: &'static str) -> Self {
        eprintln!("AUDIOBOOK_TIMING phase={phase} event=start");
        Self { phase, started: Instant::now() }
    }
}
impl Drop for OpeningTiming {
    fn drop(&mut self) {
        eprintln!("AUDIOBOOK_TIMING phase={} event=end elapsed_ms={}", self.phase, self.started.elapsed().as_millis());
    }
}
#[cfg(any(target_os = "android", test))]
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
#[path = "android_audio.rs"]
pub mod android;
#[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
mod audio_engine;

#[cfg(target_os = "android")]
use android::AudioEngine;
#[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
use audio_engine::AudioEngine;
#[cfg(target_arch = "wasm32")]
mod web_audio;
use audiobook_library::AudiobookChapter;
use bokheim_backend::LibraryClient;
use bokheim_backend::ResolvedBook;
use gpui_component::{ElementExt, Icon, IconName};
use library_model::BookLocator;
#[cfg(target_arch = "wasm32")]
use web_audio::AudioEngine;

const POSITION_SAVE_INTERVAL: Duration = Duration::from_secs(5);
/// How far the transport's two skip controls — and the keys that mirror them —
/// move. Back is shorter than forward: it is used to hear something again.
const SKIP_BACK_SECONDS: f64 = -15.0;
const SKIP_FORWARD_SECONDS: f64 = 30.0;
/// The speeds the menu offers and the keyboard steps through.
const PLAYBACK_SPEEDS: [f64; 6] = [0.75, 1.0, 1.25, 1.5, 1.75, 2.0];
/// The sleep timer's fixed lengths, in minutes.
const SLEEP_MINUTES: [u64; 5] = [5, 15, 30, 45, 60];
/// The chapter list's two amount columns, and the transport's two time labels.
const COVER_WIDTH: f32 = 128.0;
const MENU_WIDTH: f32 = 176.0;

#[derive(Clone, Copy)]
struct MenuLayout {
    compact: bool,
    max_width: f32,
    max_height: f32,
}

impl MenuLayout {
    fn for_window(window: &Window) -> Self {
        let compact = ui_components::WindowWidthClass::for_window(window).is_compact();
        Self {
            compact,
            max_width: (f32::from(window.viewport_size().width) - if compact { 0.0 } else { 2.0 * ui_components::SPACE_MD }).max(1.0),
            max_height: (f32::from(window.viewport_size().height) - if compact { ui_components::SPACE_MD } else { 56.0 + 2.0 * ui_components::SPACE_MD }).max(1.0),
        }
    }
}
/// The progress track is drawn thin but has to be easy to hit, so its hit area
/// is taller than the bar inside it.
const TRACK_HIT_HEIGHT: f32 = 16.0;

/// The key context the player's keys are bound in. It is not the reader's:
/// nothing here pages, and space belongs to playback.
pub const KEY_CONTEXT: &str = "Audiobook";

actions!(audiobook, [TogglePlayback, SkipBack, SkipForward, PreviousChapter, NextChapter, IncreaseSpeed, DecreaseSpeed, ReturnToLibrary, DismissMenus]);

/// Binds the player's keys. Called once at startup, alongside the readers'.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("space", TogglePlayback, Some(KEY_CONTEXT)),
        // Left and right are the two skip buttons, so the keyboard and the
        // transport cannot drift apart; chapters are a row up.
        KeyBinding::new("left", SkipBack, Some(KEY_CONTEXT)),
        KeyBinding::new("right", SkipForward, Some(KEY_CONTEXT)),
        KeyBinding::new("up", PreviousChapter, Some(KEY_CONTEXT)),
        KeyBinding::new("down", NextChapter, Some(KEY_CONTEXT)),
        KeyBinding::new("]", IncreaseSpeed, Some(KEY_CONTEXT)),
        KeyBinding::new("[", DecreaseSpeed, Some(KEY_CONTEXT)),
        KeyBinding::new("backspace", ReturnToLibrary, Some(KEY_CONTEXT)),
        KeyBinding::new("escape", DismissMenus, Some(KEY_CONTEXT)),
    ]);
}

type PlaybackPosition = (Duration, f32);

// Browser media callbacks report fractional progress, whereas the library stores
// percentages. Reject invalid media values before they reach persistence.
#[cfg(any(target_arch = "wasm32", test))]
fn browser_position_update(seconds: f64, fraction: f64) -> Option<PlaybackPosition> {
    if !fraction.is_finite() {
        return None;
    }
    let position = Duration::try_from_secs_f64(seconds).ok()?;
    Some((position, (fraction.clamp(0.0, 1.0) * 100.0) as f32))
}

#[cfg(test)]
mod browser_position_tests {
    use super::*;

    #[test]
    fn browser_progress_uses_library_percentages() {
        assert_eq!(browser_position_update(150.25, 0.5), Some((Duration::from_millis(150250), 50.0)));
        assert_eq!(browser_position_update(300.0, 1.0), Some((Duration::from_secs(300), 100.0)));
        assert_eq!(browser_position_update(0.0, 0.0), Some((Duration::ZERO, 0.0)));
    }

    #[test]
    fn browser_progress_clamps_finite_rounding_overshoot() {
        assert_eq!(browser_position_update(1.0, 1.01).unwrap().1, 100.0);
        assert_eq!(browser_position_update(0.0, -0.01).unwrap().1, 0.0);
    }

    #[test]
    fn invalid_browser_media_values_are_not_saved() {
        for seconds in [-1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, f64::MAX] {
            assert_eq!(browser_position_update(seconds, 0.5), None);
        }
        for fraction in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(browser_position_update(1.0, fraction), None);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
trait ReadSeek: Read + Seek + Send + Sync {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Read + Seek + Send + Sync> ReadSeek for T {}

#[cfg(not(target_arch = "wasm32"))]
struct AudiobookSource {
    stream_control: Option<bokheim_backend::AudioStreamControl>,
    cached_metadata: Option<bokheim_backend::PlaybackMetadata>,
    display_path: PathBuf,
    #[cfg(target_os = "android")]
    playback_file: Option<std::fs::File>,
    reader: Box<dyn ReadSeek>,
}

#[cfg(not(target_arch = "wasm32"))]
impl AudiobookSource {
    fn new(reader: impl Read + Seek + Send + Sync + 'static, display_path: PathBuf) -> Self {
        Self {
            stream_control: None,
            cached_metadata: None,
            display_path,
            reader: Box::new(reader),
            #[cfg(target_os = "android")]
            playback_file: None,
        }
    }
}

#[derive(Clone)]
struct AudiobookPublication {
    title: String,
    author: Option<String>,
    narrator: Option<String>,
    duration: Duration,
    chapters: Vec<AudiobookChapter>,
    cover: Option<Arc<Image>>,
}

type CloseReader = Rc<dyn Fn(&mut Window, &mut App)>;
/// Persists the narration speed. It belongs to the reader's settings rather
/// than to this session: it describes how someone listens, not what to.
type SaveSpeed = Rc<dyn Fn(f64, &mut App)>;

/// What the sleep timer is waiting for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SleepPlan {
    /// A wall clock: it runs down whether or not anything is playing, so a
    /// pause to answer the door does not silently extend the night.
    Until(Instant),
    /// The end of the chapter that was playing when it was set.
    EndOfChapter(usize),
}

/// A whole-page audiobook reading session.
pub struct AudiobookReader {
    playback: Entity<PlaybackSession>,
    speed_menu_open: bool,
    sleep_menu_open: bool,
    progress_hover: Option<f32>,
    /// The fraction of the current chapter the pointer is dragging the position
    /// to. Present only during a drag, and what the transport reads while it is.
    scrub: Option<f32>,
    /// Where the progress track was last laid out, so a drag that has left it
    /// can still be turned into a position.
    progress_bounds: Rc<Cell<Bounds<Pixels>>>,
    focus: FocusHandle,
    chapter_scroll: UniformListScrollHandle,
    /// The chapter the list was last scrolled to, so it follows playback
    /// without fighting the reader who is scrolling it.
    revealed_chapter: Option<usize>,
    close_reader: CloseReader,
}

impl AudiobookReader {
    fn new(locator: BookLocator, library: LibraryClient, resolved: ResolvedBook, initial_target: Option<String>, speed: f64, save_speed: SaveSpeed, close_reader: CloseReader, cx: &mut Context<Self>) -> Self {
        cx.set_volume_button_capture(false);
        let playback = cx.new(|cx| PlaybackSession::open(locator, library, resolved, initial_target, speed, save_speed, cx));
        Self::from_session(playback, close_reader, cx)
    }

    pub fn from_session(playback: Entity<PlaybackSession>, close_reader: CloseReader, cx: &mut Context<Self>) -> Self {
        cx.observe(&playback, |reader, _, cx| {
            reader.reveal_active_chapter(cx);
            cx.notify();
        })
        .detach();
        Self {
            playback,
            speed_menu_open: false,
            sleep_menu_open: false,
            progress_hover: None,
            scrub: None,
            progress_bounds: Rc::new(Cell::new(Bounds::default())),
            focus: cx.focus_handle(),
            chapter_scroll: UniformListScrollHandle::new(),
            revealed_chapter: None,
            close_reader,
        }
    }

    pub fn playback_session(&self) -> Entity<PlaybackSession> {
        self.playback.clone()
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.persist_position(true, cx);
        (self.close_reader)(window, cx);
    }

    pub fn stop_playback(&mut self, cx: &mut App) {
        self.playback.update(cx, |session, cx| {
            session.stop_playback();
            cx.notify();
        });
    }

    /// What the sleep control reports, or `None` when nothing is armed.
    fn sleep_label(&self, cx: &App) -> Option<SharedString> {
        self.playback.read(cx).sleep_label()
    }

    fn set_sleep(&mut self, plan: Option<SleepPlan>, cx: &mut App) {
        self.playback.update(cx, |session, cx| {
            session.set_sleep(plan);
            cx.notify();
        });
        self.sleep_menu_open = false;
    }

    fn toggle_sleep_menu(&mut self) {
        self.sleep_menu_open = !self.sleep_menu_open;
        self.speed_menu_open = false;
    }

    /// Keeps the chapter list showing the chapter being played, without
    /// scrolling on every tick: only a change of chapter moves the list.
    fn reveal_active_chapter(&mut self, cx: &App) {
        let active = self.active_chapter_index(cx);
        if self.revealed_chapter == Some(active) {
            return;
        }
        self.revealed_chapter = Some(active);
        self.chapter_scroll.scroll_to_item(active, ScrollStrategy::Center);
    }

    /// The fraction of the current chapter at a window x coordinate.
    fn track_fraction(&self, x: Pixels) -> Option<f32> {
        let bounds = self.progress_bounds.get();
        let width = f32::from(bounds.size.width);
        (width > 0.0).then(|| (f32::from(x - bounds.left()) / width).clamp(0.0, 1.0))
    }

    fn begin_scrub(&mut self, x: Pixels, cx: &mut Context<Self>) {
        let Some(fraction) = self.track_fraction(x) else { return };
        self.scrub = Some(fraction);
        self.progress_hover = Some(fraction);
        cx.notify();
    }

    fn scrub_to(&mut self, x: Pixels, cx: &mut Context<Self>) {
        if self.scrub.is_none() {
            return;
        }
        let Some(fraction) = self.track_fraction(x) else { return };
        self.scrub = Some(fraction);
        self.progress_hover = Some(fraction);
        cx.notify();
    }

    /// Reports where the pointer is while it is not dragging.
    fn hover_scrub(&mut self, x: Pixels, cx: &mut Context<Self>) {
        let Some(fraction) = self.track_fraction(x) else { return };
        if self.scrub.is_some() {
            self.scrub = Some(fraction);
        }
        if self.progress_hover != Some(fraction) {
            self.progress_hover = Some(fraction);
            cx.notify();
        }
    }

    /// Commits the drag: the seek happens once, where the pointer was released.
    fn finish_scrub(&mut self, cx: &mut Context<Self>) {
        let Some(fraction) = self.scrub.take() else { return };
        if let Some(publication) = &self.playback.read(cx).publication {
            let chapter = &publication.chapters[active_chapter_index(&publication.chapters, self.playback.read(cx).position)];
            let target = chapter_position_at_fraction(chapter.start, chapter.end, fraction);
            self.seek_to(target, cx);
        }
        cx.notify();
    }

    fn toggle_playback(&mut self, cx: &mut App) {
        self.playback.update(cx, |session, cx| session.request_toggle(cx));
    }

    fn seek_by(&mut self, seconds: f64, cx: &mut App) {
        self.playback.update(cx, |session, cx| {
            session.seek_by(seconds);
            cx.notify();
        });
    }

    fn seek_to(&mut self, position: Duration, cx: &mut App) {
        self.playback.update(cx, |session, cx| {
            session.seek_to(position);
            cx.notify();
        });
    }

    fn previous_chapter(&mut self, cx: &mut App) {
        self.playback.update(cx, |session, cx| {
            session.previous_chapter();
            cx.notify();
        });
    }

    fn next_chapter(&mut self, cx: &mut App) {
        self.playback.update(cx, |session, cx| {
            session.next_chapter();
            cx.notify();
        });
    }

    fn set_rate(&mut self, rate: f64, cx: &mut App) {
        self.playback.update(cx, |session, cx| {
            session.set_rate(rate, cx);
            cx.notify();
        });
        self.speed_menu_open = false;
    }

    /// Moves to the next or previous offered speed.
    fn step_speed(&mut self, delta: isize, cx: &mut App) {
        let rate = stepped_speed(self.playback.read(cx).rate, delta);
        if !same_rate(rate, self.playback.read(cx).rate) {
            self.set_rate(rate, cx);
        }
    }

    fn toggle_speed_menu(&mut self) {
        self.speed_menu_open = !self.speed_menu_open;
        self.sleep_menu_open = false;
    }

    fn on_toggle_playback(&mut self, _: &TogglePlayback, _: &mut Window, cx: &mut Context<Self>) {
        self.toggle_playback(cx);
        cx.notify();
    }

    fn on_skip_back(&mut self, _: &SkipBack, _: &mut Window, cx: &mut Context<Self>) {
        self.seek_by(SKIP_BACK_SECONDS, cx);
        cx.notify();
    }

    fn on_skip_forward(&mut self, _: &SkipForward, _: &mut Window, cx: &mut Context<Self>) {
        self.seek_by(SKIP_FORWARD_SECONDS, cx);
        cx.notify();
    }

    fn on_previous_chapter(&mut self, _: &PreviousChapter, _: &mut Window, cx: &mut Context<Self>) {
        self.previous_chapter(cx);
        self.reveal_active_chapter(cx);
        cx.notify();
    }

    fn on_next_chapter(&mut self, _: &NextChapter, _: &mut Window, cx: &mut Context<Self>) {
        self.next_chapter(cx);
        self.reveal_active_chapter(cx);
        cx.notify();
    }

    fn on_increase_speed(&mut self, _: &IncreaseSpeed, _: &mut Window, cx: &mut Context<Self>) {
        self.step_speed(1, cx);
        cx.notify();
    }

    fn on_decrease_speed(&mut self, _: &DecreaseSpeed, _: &mut Window, cx: &mut Context<Self>) {
        self.step_speed(-1, cx);
        cx.notify();
    }

    fn on_return_to_library(&mut self, _: &ReturnToLibrary, window: &mut Window, cx: &mut Context<Self>) {
        self.close(window, cx);
    }

    /// Escape closes whichever menu is open, and otherwise means whatever the
    /// window around the player takes it to mean.
    fn on_dismiss_menus(&mut self, _: &DismissMenus, _: &mut Window, cx: &mut Context<Self>) {
        if !self.speed_menu_open && !self.sleep_menu_open {
            cx.propagate();
            return;
        }
        self.speed_menu_open = false;
        self.sleep_menu_open = false;
        cx.notify();
    }

    fn is_playing(&self, cx: &App) -> bool {
        self.playback.read(cx).transport_playing()
    }

    fn active_chapter_index(&self, cx: &App) -> usize {
        self.playback.read(cx).active_chapter_index()
    }

    // Android service callbacks are the sole persistence producer. A UI timer
    // could otherwise enqueue an older position after a newer service event.
    #[cfg(any(target_os = "android", target_arch = "wasm32"))]
    fn persist_position(&mut self, _force: bool, cx: &mut App) {
        self.playback.update(cx, |session, _| session.persist_position(_force))
    }

    #[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
    fn persist_position(&mut self, force: bool, cx: &mut App) {
        self.playback.update(cx, |session, _| session.persist_position(force))
    }
}

trait PlaybackTarget: 'static {
    fn toggle_playback(&mut self, cx: &mut App);
    fn seek_by(&mut self, seconds: f64, cx: &mut App);
    fn previous_chapter(&mut self, cx: &mut App);
    fn next_chapter(&mut self, cx: &mut App);
    fn set_rate(&mut self, rate: f64, cx: &mut App);
    fn set_sleep(&mut self, plan: Option<SleepPlan>, cx: &mut App);
    fn toggle_sleep_menu(&mut self);
    fn toggle_speed_menu(&mut self);
    fn active_chapter_index(&self, cx: &App) -> usize;
    fn sleep_label(&self, cx: &App) -> Option<SharedString>;
}

impl PlaybackTarget for AudiobookReader {
    fn toggle_playback(&mut self, cx: &mut App) {
        self.toggle_playback(cx);
    }
    fn seek_by(&mut self, seconds: f64, cx: &mut App) {
        self.seek_by(seconds, cx);
    }
    fn previous_chapter(&mut self, cx: &mut App) {
        self.previous_chapter(cx);
    }
    fn next_chapter(&mut self, cx: &mut App) {
        self.next_chapter(cx);
    }
    fn set_rate(&mut self, rate: f64, cx: &mut App) {
        self.set_rate(rate, cx);
    }
    fn set_sleep(&mut self, plan: Option<SleepPlan>, cx: &mut App) {
        self.set_sleep(plan, cx);
    }
    fn toggle_sleep_menu(&mut self) {
        self.toggle_sleep_menu();
    }
    fn toggle_speed_menu(&mut self) {
        self.toggle_speed_menu();
    }
    fn active_chapter_index(&self, cx: &App) -> usize {
        self.active_chapter_index(cx)
    }
    fn sleep_label(&self, cx: &App) -> Option<SharedString> {
        self.sleep_label(cx)
    }
}

impl AudiobookReader {
    /// The player body: what is playing, the chapters, and the transport.
    fn render_expanded(&mut self, window: &Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = ui_components::browser_theme(cx);
        let target = cx.entity().downgrade();

        let Some(book) = self.playback.read(cx).publication.clone() else {
            let mut opening = ui_components::reader_document_message("Opening audiobook…", theme).flex_col().gap(px(ui_components::SPACE_SM));
            if let Some(error) = self.playback.read(cx).error.clone() {
                opening = opening.child(div().text_color(theme.danger).child(error));
            }
            return opening.into_any_element();
        };

        let active_chapter = self.active_chapter_index(cx);
        let chapter = &book.chapters[active_chapter];
        // While the track is being dragged the whole row reads from the drag,
        // so the elapsed time, the fill and the tooltip cannot disagree.
        let position = match self.scrub {
            Some(fraction) => chapter_position_at_fraction(chapter.start, chapter.end, fraction),
            None => self.playback.read(cx).position.min(book.duration),
        };
        let progress_row = chapter_progress_row(target.clone(), self.progress_bounds.clone(), chapter.start, chapter.end, position, self.progress_hover, theme);
        let controls = playback_transport(target.clone(), self.is_playing(cx), self.playback.read(cx).rate, self.speed_menu_open, self.sleep_label(cx), self.sleep_menu_open, MenuLayout::for_window(window), theme);

        let cover_art = if let Some(cover) = book.cover.clone() {
            cover_frame(theme).child(img(cover).size_full().object_fit(ObjectFit::Cover))
        } else {
            cover_frame(theme)
                .bg(theme.accent_wash)
                .flex()
                .flex_col()
                .justify_between()
                .p(px(ui_components::SPACE_SM))
                .child(div().text_size(gpui::rems(ui_components::TEXT_XS / 16.0)).font_weight(FontWeight::BOLD).child("M4B"))
                .child(div().text_size(gpui::rems(ui_components::TEXT_SM / 16.0)).font_weight(FontWeight::SEMIBOLD).line_height(relative(1.08)).child(book.title.clone()))
        };

        let chapter_header = div()
            .flex()
            .items_center()
            .gap(px(ui_components::SPACE_MD))
            .px(px(ui_components::SPACE_SM))
            .pb(px(ui_components::SPACE_XS))
            .text_size(gpui::rems(ui_components::TEXT_XS / 16.0))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme.text_muted)
            .child(div().flex_1().min_w_0().child("Title"))
            .child(div().w(gpui::rems(ui_components::CONTENTS_AMOUNT_WIDTH_REM)).flex_none().text_right().child("Duration"));
        let chapter_book = book.clone();
        let chapter_target = target.clone();
        // One contents list, drawn by the same component the reader's sidebar
        // and the library's detail page use. A chapter is an entry with no
        // children, a number and an amount; the only thing particular to a
        // player is what a click does — seek what is already playing, rather
        // than open anything.
        let chapter_list = uniform_list("audiobook-chapters", book.chapters.len(), move |visible_range, _, _| {
            visible_range
                .map(|index| {
                    let chapter = &chapter_book.chapters[index];
                    let start = chapter.start;
                    let duration = chapter.end.saturating_sub(chapter.start);
                    let target = chapter_target.clone();
                    let row = ui_components::ContentsRow { marked: index == active_chapter, ..Default::default() };
                    ui_components::contents_row(
                        ("chapter", index),
                        row,
                        ui_components::contents_disclosure(("chapter-disclosure", index), row, theme),
                        div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(chapter_display_title(&chapter.title)),
                        Some(index + 1),
                        Some(format_time(duration).into()),
                        theme,
                    )
                    .on_click(move |_, _, cx| {
                        let _ = target.update(cx, |this, cx| {
                            this.seek_to(start, cx);
                            this.reveal_active_chapter(cx);
                            cx.notify();
                        });
                    })
                })
                .collect()
        })
        .track_scroll(&self.chapter_scroll)
        .flex_1()
        .min_h_0();

        div()
            .size_full()
            .min_h_0()
            .relative()
            .flex()
            .flex_col()
            .child(
                div().flex_none().bg(theme.page_bg).border_b_1().border_color(theme.rule).p(px(ui_components::SPACE_MD)).flex().items_center().gap(px(ui_components::SPACE_MD)).child(cover_art).child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .gap(px(ui_components::SPACE_XS))
                        .child(div().text_size(gpui::rems(ui_components::TITLE_MD / 16.0)).font_weight(FontWeight::SEMIBOLD).line_height(relative(1.08)).child(book.title.clone()))
                        .when_some(book.author.clone(), |details, author| details.child(div().text_size(gpui::rems(ui_components::TEXT_SM / 16.0)).text_color(theme.text_muted).child(author)))
                        .when_some(book.narrator.clone(), |details, narrator| details.child(div().text_size(gpui::rems(ui_components::TEXT_SM / 16.0)).text_color(theme.text_muted).child(format!("Narrated by {narrator}")))),
                ),
            )
            .child(div().flex_1().min_h_0().flex().flex_col().px(px(ui_components::SPACE_SM)).py(px(ui_components::SPACE_SM)).child(chapter_header).child(chapter_list))
            // The transport is a column: the position is read first and the
            // controls act on it. A row — which is what a bare `div` is — put
            // the two side by side and left the track whatever width the
            // buttons did not take.
            .child(
                div()
                    .flex_none()
                    .relative()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(px(ui_components::SPACE_SM))
                    .px(px(ui_components::SPACE_MD))
                    .py(px(ui_components::SPACE_MD))
                    .bg(theme.page_bg)
                    .border_t_1()
                    .border_color(theme.rule)
                    .child(progress_row)
                    .child(controls),
            )
            .when(self.playback.read(cx).is_buffering(), |column| column.child(div().text_color(theme.text_muted).child("Buffering…")))
            .when_some(self.playback.read(cx).error.clone(), |column, error| {
                let dismiss = cx.entity();
                column.child(
                    div()
                        .absolute()
                        .left(px(ui_components::SPACE_MD))
                        .right(px(ui_components::SPACE_MD))
                        .top(px(ui_components::SPACE_MD))
                        .p(px(ui_components::SPACE_SM))
                        .bg(theme.danger_surface)
                        .text_color(theme.danger)
                        .text_size(gpui::rems(ui_components::TEXT_SM / 16.0))
                        .flex()
                        .items_center()
                        .gap(px(ui_components::SPACE_SM))
                        .child(div().flex_1().min_w_0().child(error))
                        .child(div().id("dismiss-audiobook-error").cursor_pointer().flex_none().child(Icon::new(IconName::Close).size(px(14.0))).on_click(move |_, _, cx| {
                            dismiss.update(cx, |this, cx| {
                                this.playback.update(cx, |session, cx| {
                                    session.error = None;
                                    cx.notify();
                                });
                                cx.notify();
                            });
                        })),
                )
            })
            .into_any_element()
    }

    /// Keep the top bar for returning to the library.
    fn toolbar(&self, window: &Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = ui_components::browser_theme(cx);
        let library = cx.entity();
        if ui_components::uses_mobile_navigation(window) {
            let title = self.playback.read(cx).publication.as_ref().map(|book| book.title.clone()).unwrap_or_else(|| "Audiobook".into());
            let back = ui_components::mobile_back_button(theme).on_click(move |_, window, cx| library.update(cx, |this, cx| this.close(window, cx)));
            return ui_components::mobile_navigation_bar(title, back, theme).into_any_element();
        }
        let left = div().flex_1().min_w_0().flex().items_center().gap(px(ui_components::SPACE_SM)).child(ui_components::reader_control_button_icon("audiobook-library", "Return to library", IconName::ArrowLeft, theme).on_click(
            move |_, window, cx| {
                library.update(cx, |this, cx| this.close(window, cx));
            },
        ));
        ui_components::reader_toolbar(theme).border_b_1().border_color(theme.rule).child(left).into_any_element()
    }
}

impl Focusable for AudiobookReader {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for AudiobookReader {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = ui_components::browser_theme(cx);
        let toolbar = self.toolbar(window, cx);
        let body = self.render_expanded(window, cx);
        let layout = MenuLayout::for_window(window);
        div()
            .relative()
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(theme.page_bg)
            .text_color(theme.text)
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::on_toggle_playback))
            .on_action(cx.listener(Self::on_skip_back))
            .on_action(cx.listener(Self::on_skip_forward))
            .on_action(cx.listener(Self::on_previous_chapter))
            .on_action(cx.listener(Self::on_next_chapter))
            .on_action(cx.listener(Self::on_increase_speed))
            .on_action(cx.listener(Self::on_decrease_speed))
            .on_action(cx.listener(Self::on_return_to_library))
            .on_action(cx.listener(Self::on_dismiss_menus))
            .child(toolbar)
            .child(div().flex_1().min_h_0().child(body))
            .when(layout.compact && (self.sleep_menu_open || self.speed_menu_open), |page| {
                let target = cx.entity().downgrade();
                let sheet = if self.sleep_menu_open { sleep_menu(target.clone(), self.sleep_label(cx), layout, theme) } else { playback_speed_menu(target.clone(), self.playback.read(cx).rate, layout, theme) };
                page.child(
                    gpui::deferred(
                        div()
                            .id("audiobook-menu-backdrop")
                            .absolute()
                            .inset_0()
                            .bg(gpui::rgba(0x00000040))
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                cx.stop_propagation();
                                let _ = target.update(cx, |reader, cx| {
                                    reader.sleep_menu_open = false;
                                    reader.speed_menu_open = false;
                                    cx.notify();
                                });
                            })
                            .child(sheet),
                    )
                    .with_priority(1),
                )
            })
            // A drag that has left the 6px track still has to be followed, and
            // a hitbox under the pointer would otherwise swallow the moves, so
            // while scrubbing the whole window listens.
            .when(self.scrub.is_some(), |page| page.child(scrub_capture(cx.entity().downgrade())))
            .into_any_element()
    }
}

/// Window-wide mouse listeners installed for the duration of a scrub.
fn scrub_capture(target: gpui::WeakEntity<AudiobookReader>) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |_, _: (), window, _| {
            let moved = target.clone();
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble {
                    let _ = moved.update(cx, |this, cx| this.scrub_to(event.position.x, cx));
                }
            });
            let released = target.clone();
            window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble && event.button == MouseButton::Left {
                    let _ = released.update(cx, |this, cx| this.finish_scrub(cx));
                }
            });
        },
    )
    .absolute()
    .size_full()
}

fn cover_frame(theme: ui_components::BrowserTheme) -> gpui::Div {
    div().w(px(COVER_WIDTH)).h(px(COVER_WIDTH * ui_components::BOOK_COVER_ASPECT_RATIO.recip())).flex_none().overflow_hidden().border_1().border_color(theme.border)
}

fn playback_transport(
    target: gpui::WeakEntity<impl PlaybackTarget>, playing: bool, current_rate: f64, speed_menu_open: bool, sleep_label: Option<SharedString>, sleep_menu_open: bool, layout: MenuLayout, theme: ui_components::BrowserTheme,
) -> gpui::Div {
    let previous_target = target.clone();
    let back_target = target.clone();
    let play_target = target.clone();
    let forward_target = target.clone();
    let next_target = target.clone();
    let rate_target = target.clone();
    let sleep_target = target.clone();
    let play = transport_control_base("toggle-play", Icon::new(if playing { IconName::Pause } else { IconName::Play }).size(px(22.0)), theme)
        .bg(theme.accent)
        .text_color(theme.accent_text)
        .h(px(56.0))
        .hover(|style| style.bg(theme.accent_hover).text_color(theme.accent_text))
        .on_click(move |_, _, cx| {
            let _ = play_target.update(cx, |dock, cx| {
                dock.toggle_playback(cx);
                cx.notify();
            });
        });

    let sleep_armed = sleep_label.is_some();
    let sleep_content = match sleep_label.clone() {
        Some(label) => div().child(label).into_any_element(),
        None => Icon::empty().path("icons/alarm.svg").size(px(18.0)).into_any_element(),
    };
    let sleep = div().relative().flex_1().min_w_0().when(sleep_menu_open && !layout.compact, |wrapper| wrapper.child(gpui::deferred(sleep_menu(target.clone(), sleep_label.clone(), layout, theme)).with_priority(1))).child(
        menu_trigger("sleep-timer", sleep_content, sleep_menu_open || sleep_armed, theme).on_click(move |_, _, cx| {
            let _ = sleep_target.update(cx, |dock, cx| {
                dock.toggle_sleep_menu();
                cx.notify();
            });
        }),
    );
    let speed = div().relative().flex_1().min_w_0().when(speed_menu_open && !layout.compact, |wrapper| wrapper.child(gpui::deferred(playback_speed_menu(rate_target.clone(), current_rate, layout, theme)).with_priority(1))).child(
        menu_trigger("playback-rate", format_playback_rate(current_rate), speed_menu_open, theme).on_click(move |_, _, cx| {
            let _ = rate_target.update(cx, |dock, cx| {
                dock.toggle_speed_menu();
                cx.notify();
            });
        }),
    );

    let transport = div()
        .w_full()
        .flex_none()
        .flex()
        .items_center()
        .child(sleep)
        .child(transport_control("previous-chapter", chapter_icon(false, theme), theme).on_click(move |_, _, cx| {
            let _ = previous_target.update(cx, |dock, cx| {
                dock.previous_chapter(cx);
                cx.notify();
            });
        }))
        .child(transport_control("seek-back", skip_icon(false, "15"), theme).on_click(move |_, _, cx| {
            let _ = back_target.update(cx, |dock, cx| {
                dock.seek_by(SKIP_BACK_SECONDS, cx);
                cx.notify();
            });
        }))
        .child(play)
        .child(transport_control("seek-forward", skip_icon(true, "30"), theme).on_click(move |_, _, cx| {
            let _ = forward_target.update(cx, |dock, cx| {
                dock.seek_by(SKIP_FORWARD_SECONDS, cx);
                cx.notify();
            });
        }))
        .child(transport_control("next-chapter", chapter_icon(true, theme), theme).on_click(move |_, _, cx| {
            let _ = next_target.update(cx, |dock, cx| {
                dock.next_chapter(cx);
                cx.notify();
            });
        }));

    transport.child(speed)
}

fn chapter_progress_row(
    target: gpui::WeakEntity<AudiobookReader>, bounds: Rc<Cell<Bounds<Pixels>>>, chapter_start: Duration, chapter_end: Duration, position: Duration, hover_fraction: Option<f32>, theme: ui_components::BrowserTheme,
) -> gpui::Div {
    let chapter_duration = chapter_end.saturating_sub(chapter_start);
    let chapter_position = position.saturating_sub(chapter_start).min(chapter_duration);
    let progress = playback_progress(chapter_position, chapter_duration) / 100.0;
    let painted_bounds = bounds.clone();
    let pressed_target = target.clone();
    let moved_target = target.clone();
    let released_target = target.clone();
    let released_outside_target = target.clone();
    let hover_ended_target = target;
    let progress_track = div()
        .id("chapter-progress")
        .relative()
        .w_full()
        .flex_none()
        .min_w_0()
        .h(px(TRACK_HIT_HEIGHT))
        .cursor_pointer()
        .flex()
        .items_center()
        .on_prepaint(move |bounds, _, _| painted_bounds.set(bounds))
        // Pressing starts a scrub rather than seeking outright, so a press and
        // a drag are the same gesture: the seek happens when the button is
        // released, wherever the pointer has got to by then.
        .on_mouse_down(MouseButton::Left, move |event, _, cx| {
            let _ = pressed_target.update(cx, |dock, cx| dock.begin_scrub(event.position.x, cx));
        })
        .on_mouse_move(move |event, _, cx| {
            let _ = moved_target.update(cx, |dock, cx| dock.hover_scrub(event.position.x, cx));
        })
        .on_mouse_up(MouseButton::Left, move |_, _, cx| {
            let _ = released_target.update(cx, |dock, cx| dock.finish_scrub(cx));
        })
        .on_mouse_up_out(MouseButton::Left, move |_, _, cx| {
            let _ = released_outside_target.update(cx, |dock, cx| dock.finish_scrub(cx));
        })
        .on_hover(move |hovered, _, cx| {
            if !hovered {
                let _ = hover_ended_target.update(cx, |dock, cx| {
                    if dock.progress_hover.take().is_some() {
                        cx.notify();
                    }
                });
            }
        })
        .child(div().relative().w_full().h(px(ui_components::PROGRESS_TRACK_HEIGHT)).bg(theme.rule).child(div().absolute().left_0().top_0().h_full().w(relative(progress)).bg(theme.accent)))
        .when_some(hover_fraction, |track, fraction| {
            let hover_position = chapter_position_at_fraction(chapter_start, chapter_end, fraction).saturating_sub(chapter_start);
            track.child(
                div()
                    .absolute()
                    .left(relative(fraction.clamp(0.08, 0.92)))
                    .bottom(px(18.0))
                    .ml(px(-28.0))
                    .min_w(px(56.0))
                    .px(px(ui_components::SPACE_XS))
                    .py(px(ui_components::SPACE_XXS))
                    .bg(theme.text)
                    .text_color(theme.page_bg)
                    .text_center()
                    .text_size(gpui::rems(ui_components::TEXT_XS / 16.0))
                    .child(format_time(hover_position)),
            )
        });
    div()
        .w_full()
        .min_w_0()
        .flex()
        .flex_col()
        .text_size(gpui::rems(ui_components::TEXT_XS / 16.0))
        .text_color(theme.text_muted)
        .child(div().w_full().flex().justify_between().child(format_time(chapter_position)).child(format_time(chapter_duration)))
        .child(progress_track)
}

/// The button that opens one of the transport's two menus. Lit while its menu
/// is open, and — for the sleep timer — while it has something to report.
fn menu_trigger(id: &'static str, label: impl IntoElement, active: bool, theme: ui_components::BrowserTheme) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .cursor_pointer()
        .h(px(56.0))
        .w_full()
        .min_w_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(if active { theme.accent } else { gpui::Hsla::transparent_black() })
        .text_color(if active { theme.accent_text } else { theme.text_muted })
        .hover(|style| style.bg(theme.accent_wash).text_color(theme.accent))
        .text_size(gpui::rems(ui_components::TEXT_MD))
        .font_weight(FontWeight::BOLD)
        .child(label)
}

fn menu_panel(theme: ui_components::BrowserTheme, layout: MenuLayout, title: &'static str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(title)
        .absolute()
        .bottom(px(56.0))
        .w(gpui::rems(MENU_WIDTH / 16.0))
        .max_w(px(layout.max_width))
        .max_h(px(layout.max_height))
        .overflow_y_scroll()
        .whitespace_normal()
        .when(layout.compact, |panel| panel.w_full().bottom_0())
        .p(px(ui_components::SPACE_XS))
        .border_1()
        .border_color(theme.border)
        .bg(theme.page_bg)
        .shadow_lg()
        .flex()
        .flex_col()
        .gap(px(ui_components::SPACE_XXS))
        .child(
            div()
                .flex_none()
                .min_w_0()
                .px(px(ui_components::SPACE_SM))
                .pt(px(ui_components::SPACE_XS))
                .pb(px(ui_components::SPACE_XS))
                .text_size(gpui::rems(ui_components::TEXT_MD))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.text)
                .child(title),
        )
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}

fn menu_option(id: String, label: String, selected: bool, theme: ui_components::BrowserTheme) -> gpui::Stateful<gpui::Div> {
    div()
        .id(gpui::SharedString::from(id))
        .flex_none()
        .min_w_0()
        .cursor_pointer()
        .flex()
        .items_center()
        .justify_between()
        .px(px(ui_components::SPACE_SM))
        .py(px(ui_components::SPACE_SM))
        .text_size(gpui::rems(ui_components::TEXT_MD))
        .hover(|style| style.bg(theme.accent_wash))
        .when(selected, |row| row.bg(theme.accent).text_color(theme.accent_text))
        .child(div().flex_1().min_w_0().whitespace_normal().child(label))
        .when(selected, |row| row.child(Icon::new(IconName::Check).size(px(15.0))))
}

fn playback_speed_menu<T: PlaybackTarget>(target: gpui::WeakEntity<T>, current_rate: f64, layout: MenuLayout, theme: ui_components::BrowserTheme) -> gpui::Stateful<gpui::Div> {
    let mut menu = menu_panel(theme, layout, "Playback speed").right_0();
    for rate in PLAYBACK_SPEEDS {
        let option_target = target.clone();
        menu = menu.child(menu_option(format!("speed-{rate}"), format_playback_rate(rate), same_rate(rate, current_rate), theme).on_click(move |_, _, cx| {
            cx.stop_propagation();
            let _ = option_target.update(cx, |dock, cx| {
                dock.set_rate(rate, cx);
                cx.notify();
            });
        }));
    }
    menu
}

fn sleep_menu<T: PlaybackTarget>(target: gpui::WeakEntity<T>, remaining: Option<SharedString>, layout: MenuLayout, theme: ui_components::BrowserTheme) -> gpui::Stateful<gpui::Div> {
    let armed = remaining.is_some();
    let mut menu = menu_panel(theme, layout, "Sleep timer").left_0().when_some(remaining, |menu, remaining| {
        let label = if remaining.as_ref() == "Chapter" { "Until end of chapter".to_owned() } else { format!("{remaining} remaining") };
        menu.child(div().flex_none().min_w_0().px(px(ui_components::SPACE_SM)).py(px(ui_components::SPACE_SM)).text_size(gpui::rems(ui_components::TEXT_MD / 16.0)).font_weight(FontWeight::SEMIBOLD).child(label))
    });
    for minutes in SLEEP_MINUTES {
        let option_target = target.clone();
        menu = menu.child(menu_option(format!("sleep-{minutes}"), format!("{minutes} minutes"), false, theme).on_click(move |_, _, cx| {
            cx.stop_propagation();
            let _ = option_target.update(cx, |dock, cx| {
                dock.set_sleep(Some(SleepPlan::Until(Instant::now() + Duration::from_secs(minutes * 60))), cx);
                cx.notify();
            });
        }));
    }
    let chapter_target = target.clone();
    menu = menu.child(menu_option("sleep-chapter".to_owned(), "End of chapter".to_owned(), false, theme).on_click(move |_, _, cx| {
        cx.stop_propagation();
        let _ = chapter_target.update(cx, |dock, cx| {
            let chapter = dock.active_chapter_index(cx);
            dock.set_sleep(Some(SleepPlan::EndOfChapter(chapter)), cx);
            cx.notify();
        });
    }));
    if armed {
        menu = menu.child(menu_option("sleep-off".to_owned(), "Off".to_owned(), false, theme).on_click(move |_, _, cx| {
            cx.stop_propagation();
            let _ = target.update(cx, |dock, cx| {
                dock.set_sleep(None, cx);
                cx.notify();
            });
        }));
    }
    menu
}

fn format_playback_rate(rate: f64) -> String {
    let formatted = format!("{rate:.2}");
    format!("{}×", formatted.trim_end_matches('0').trim_end_matches('.'))
}

fn transport_control(id: impl Into<SharedString>, content: impl IntoElement, theme: ui_components::BrowserTheme) -> gpui::Stateful<gpui::Div> {
    transport_control_base(id, content, theme).hover(|style| style.bg(theme.accent_wash).text_color(theme.text))
}

fn transport_control_base(id: impl Into<SharedString>, content: impl IntoElement, theme: ui_components::BrowserTheme) -> gpui::Stateful<gpui::Div> {
    div().id(id.into()).flex_1().h(px(56.0)).min_w_0().cursor_pointer().flex().items_center().justify_center().bg(gpui::Hsla::transparent_black()).text_color(theme.text_muted).font_weight(FontWeight::BOLD).child(content)
}

fn chapter_icon(forward: bool, theme: ui_components::BrowserTheme) -> gpui::Div {
    let chevron = Icon::new(if forward { IconName::ChevronRight } else { IconName::ChevronLeft }).size(px(19.0));
    let bar = div().w(px(2.0)).h(px(17.0)).bg(theme.text_muted);
    let icon = div().flex().items_center();
    if forward { icon.child(chevron).child(bar) } else { icon.child(bar).child(chevron) }
}

fn skip_icon(forward: bool, seconds: &'static str) -> gpui::Div {
    div()
        .relative()
        .size(px(32.0))
        .child(Icon::new(if forward { IconName::Redo2 } else { IconName::Undo2 }).size(px(32.0)))
        .child(div().absolute().inset_0().flex().items_center().justify_center().text_size(gpui::rems(ui_components::TEXT_XS / 16.0)).child(seconds))
}

fn active_chapter_index(chapters: &[AudiobookChapter], position: Duration) -> usize {
    chapters.iter().rposition(|chapter| chapter.start <= position).unwrap_or(0)
}

fn clamp_position(position: Duration, duration: Duration) -> Duration {
    position.min(duration)
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

fn tempo_for(rate: f64) -> Result<f32, String> {
    if !rate.is_finite() {
        return Err("playback rate must be finite".to_owned());
    }
    Ok(rate as f32)
}

fn position_save_due(previous: Option<Duration>, position: Duration) -> bool {
    previous.is_none_or(|previous| previous.abs_diff(position) >= POSITION_SAVE_INTERVAL)
}

fn position_save_required(previous: Option<Duration>, position: Duration, force: bool) -> bool {
    previous != Some(position) && (force || position_save_due(previous, position))
}

fn playback_progress(position: Duration, duration: Duration) -> f32 {
    if duration.is_zero() {
        return 0.0;
    }
    (position.as_secs_f64() / duration.as_secs_f64() * 100.0).clamp(0.0, 100.0) as f32
}

fn chapter_position_at_fraction(start: Duration, end: Duration, fraction: f32) -> Duration {
    if !fraction.is_finite() {
        return start;
    }
    start + end.saturating_sub(start).mul_f32(fraction.clamp(0.0, 1.0))
}

/// The offered speed one step from `current`, in the direction of `delta`.
///
/// A rate that is not one of the offered steps — restored from a build that
/// offered others — moves to the neighbouring step rather than being ignored.
fn stepped_speed(current: f64, delta: isize) -> f64 {
    let last = PLAYBACK_SPEEDS.len() as isize - 1;
    let index = PLAYBACK_SPEEDS.iter().position(|speed| same_rate(*speed, current)).map(|index| index as isize).unwrap_or_else(|| {
        let above = PLAYBACK_SPEEDS.iter().position(|speed| *speed > current).map(|index| index as isize).unwrap_or(last + 1);
        if delta > 0 { above - 1 } else { above }
    });
    PLAYBACK_SPEEDS[(index + delta).clamp(0, last) as usize]
}

/// Whether a sleep plan has come due.
#[cfg(any(not(target_os = "android"), test))]
fn sleep_remaining(plan: SleepPlan, position: Duration, chapters: &[AudiobookChapter], now: Instant, rate: f64) -> Duration {
    match plan {
        SleepPlan::Until(deadline) => deadline.saturating_duration_since(now),
        SleepPlan::EndOfChapter(index) => chapters.get(index).map_or(Duration::ZERO, |chapter| chapter.end.saturating_sub(position).div_f64(rate.max(0.01))),
    }
}

#[cfg(any(not(target_os = "android"), test))]
fn sleep_gain(remaining: Duration) -> f32 {
    (remaining.as_secs_f32() / 30.0).clamp(0.0, 1.0)
}

#[cfg(test)]
fn sleep_elapsed(plan: SleepPlan, position: Duration, chapters: &[AudiobookChapter], now: Instant) -> bool {
    match plan {
        SleepPlan::Until(deadline) => now >= deadline,
        // A chapter the book no longer has cannot be waited for, so the timer
        // is treated as due rather than left armed forever.
        SleepPlan::EndOfChapter(chapter) => chapters.get(chapter).is_none_or(|chapter| position >= chapter.end),
    }
}

/// Whether two speeds are the same offered step. Playback rates are a short
/// list of round numbers, so exact comparison is what is meant.
fn same_rate(left: f64, right: f64) -> bool {
    (left - right).abs() < f64::EPSILON
}

fn format_time(duration: Duration) -> String {
    let seconds = duration.as_secs();
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let seconds = seconds % 60;
    if hours > 0 { format!("{hours}:{minutes:02}:{seconds:02}") } else { format!("{minutes}:{seconds:02}") }
}

fn chapter_display_title(title: &str) -> String {
    let title = title.trim();
    for separator in [" - ", " – ", " — "] {
        if let Some((name, suffix)) = title.rsplit_once(separator)
            && is_timestamp(suffix)
        {
            return name.trim().to_owned();
        }
    }
    title.to_owned()
}

fn is_timestamp(value: &str) -> bool {
    let parts = value.split(':').collect::<Vec<_>>();
    if !(2..=3).contains(&parts.len()) || parts.iter().any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit())) {
        return false;
    }
    parts[parts.len() - 2].parse::<u64>().is_ok_and(|minutes| minutes < 60) && parts[parts.len() - 1].parse::<u64>().is_ok_and(|seconds| seconds < 60)
}

/// Opens a book and puts the keyboard on it, so the player can be operated
/// without reaching for the pointer first.
pub fn open_reader(
    locator: BookLocator, library: LibraryClient, resolved: ResolvedBook, initial_target: Option<String>, speed: f64, save_speed: SaveSpeed, close_reader: CloseReader, window: &mut Window, cx: &mut App,
) -> Entity<AudiobookReader> {
    let reader = cx.new(|cx| AudiobookReader::new(locator, library, resolved, initial_target, speed, save_speed, close_reader, cx));
    reader.read(cx).focus.clone().focus(window, cx);
    reader
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chapter(title: &str, start: u64, end: u64) -> AudiobookChapter {
        AudiobookChapter { title: title.to_owned(), start: Duration::from_secs(start), end: Duration::from_secs(end) }
    }

    fn chapters() -> Vec<AudiobookChapter> {
        vec![chapter("One", 0, 10), chapter("Two", 10, 30), chapter("Three", 30, 60)]
    }

    #[test]
    fn sleep_fade_uses_final_thirty_seconds_and_playback_rate() {
        let now = Instant::now();
        let plan = SleepPlan::Until(now + Duration::from_secs(60));
        assert_eq!(sleep_gain(sleep_remaining(plan, Duration::ZERO, &chapters(), now, 1.0)), 1.0);
        assert_eq!(sleep_gain(sleep_remaining(plan, Duration::ZERO, &chapters(), now + Duration::from_secs(45), 1.0)), 0.5);
        assert_eq!(sleep_gain(sleep_remaining(plan, Duration::ZERO, &chapters(), now + Duration::from_secs(60), 1.0)), 0.0);
        assert_eq!(sleep_gain(sleep_remaining(SleepPlan::EndOfChapter(1), Duration::ZERO, &chapters(), now, 2.0)), 0.5);
    }

    #[test]
    fn speed_steps_move_between_the_offered_rates_and_stop_at_the_ends() {
        assert_eq!(stepped_speed(1.0, 1), 1.25);
        assert_eq!(stepped_speed(1.0, -1), 0.75);
        assert_eq!(stepped_speed(0.75, -1), 0.75);
        assert_eq!(stepped_speed(2.0, 1), 2.0);
    }

    #[test]
    fn a_speed_between_the_offered_rates_steps_to_its_neighbour() {
        assert_eq!(stepped_speed(1.1, 1), 1.25);
        assert_eq!(stepped_speed(1.1, -1), 1.0);
    }

    #[test]
    fn a_sleep_deadline_comes_due_on_the_clock() {
        let now = Instant::now();
        let plan = SleepPlan::Until(now + Duration::from_secs(60));
        assert!(!sleep_elapsed(plan, Duration::ZERO, &chapters(), now));
        assert!(sleep_elapsed(plan, Duration::ZERO, &chapters(), now + Duration::from_secs(61)));
    }

    #[test]
    fn an_end_of_chapter_sleep_waits_for_that_chapter_only() {
        let now = Instant::now();
        let plan = SleepPlan::EndOfChapter(1);
        assert!(!sleep_elapsed(plan, Duration::from_secs(20), &chapters(), now));
        assert!(sleep_elapsed(plan, Duration::from_secs(30), &chapters(), now));
        // A chapter the book does not have cannot be waited for.
        assert!(sleep_elapsed(SleepPlan::EndOfChapter(9), Duration::ZERO, &chapters(), now));
    }

    #[test]
    fn positions_are_clamped_to_the_book_duration() {
        assert_eq!(clamp_position(Duration::from_secs(600), Duration::from_secs(60)), Duration::from_secs(60));
        assert_eq!(clamp_position(Duration::MAX, Duration::ZERO), Duration::ZERO);
    }

    #[test]
    fn chapter_boundaries_select_the_chapter_that_starts_there() {
        let chapters = chapters();
        for (second, expected_index) in [(0, 0), (9, 0), (10, 1), (29, 1), (30, 2), (60, 2)] {
            assert_eq!(active_chapter_index(&chapters, Duration::from_secs(second)), expected_index, "at {second}s");
        }
    }

    #[test]
    fn navigation_targets_respect_chapters_and_book_boundaries() {
        let chapters = chapters();
        let duration = Duration::from_secs(60);

        assert_eq!(previous_chapter_position(&chapters, Duration::from_secs(35)), Duration::from_secs(30));
        assert_eq!(previous_chapter_position(&chapters, Duration::from_secs(30)), Duration::from_secs(10));
        assert_eq!(next_chapter_position(&chapters, Duration::from_secs(10), duration), Duration::from_secs(30));
        assert_eq!(next_chapter_position(&chapters, duration, duration), duration);
    }

    #[test]
    fn relative_seeking_clamps_and_ignores_non_finite_offsets() {
        let position = Duration::from_secs(20);
        let duration = Duration::from_secs(60);

        assert_eq!(relative_seek_position(position, -100.0, duration), Duration::ZERO);
        assert_eq!(relative_seek_position(position, 1_000.0, duration), duration);
        for offset in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(relative_seek_position(position, offset, duration), position);
        }
    }

    #[test]
    fn previous_chapter_uses_the_three_second_restart_threshold() {
        let chapters = chapters();
        assert_eq!(previous_chapter_position(&chapters, Duration::from_secs(13)), Duration::ZERO);
        assert_eq!(previous_chapter_position(&chapters, Duration::from_millis(13_001)), Duration::from_secs(10));
    }

    #[test]
    fn empty_chapter_navigation_stays_inside_the_book() {
        assert_eq!(active_chapter_index(&[], Duration::from_secs(10)), 0);
        assert_eq!(previous_chapter_position(&[], Duration::from_secs(10)), Duration::ZERO);
        assert_eq!(next_chapter_position(&[], Duration::from_secs(10), Duration::from_secs(60)), Duration::from_secs(60));
    }

    #[test]
    fn playback_rates_must_be_finite() {
        for rate in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(tempo_for(rate), Err("playback rate must be finite".to_owned()));
        }
        assert_eq!(tempo_for(1.5), Ok(1.5));
    }

    #[test]
    fn time_format_covers_minute_and_hour_forms() {
        assert_eq!(format_time(Duration::from_secs(9)), "0:09");
        assert_eq!(format_time(Duration::from_secs(3_725)), "1:02:05");
    }

    #[test]
    fn chapter_display_title_removes_only_a_trailing_duration() {
        assert_eq!(chapter_display_title("Chapter 001 - 00:30:37"), "Chapter 001");
        assert_eq!(chapter_display_title("Chapter 002 – 30:37"), "Chapter 002");
        assert_eq!(chapter_display_title("A title - with words"), "A title - with words");
        assert_eq!(chapter_display_title("A title - 72:99"), "A title - 72:99");
        assert_eq!(chapter_display_title("  Plain title  "), "Plain title");
    }

    #[test]
    fn playback_position_persistence_is_bounded_and_throttled() {
        assert!(position_save_due(None, Duration::ZERO));
        assert!(!position_save_due(Some(Duration::from_secs(10)), Duration::from_secs(14)));
        assert!(position_save_due(Some(Duration::from_secs(10)), Duration::from_secs(15)));
        assert!(position_save_due(Some(Duration::from_secs(10)), Duration::from_secs(5)));
        assert!(!position_save_required(Some(Duration::from_secs(10)), Duration::from_secs(10), true));
        assert!(position_save_required(Some(Duration::from_secs(10)), Duration::from_secs(11), true));
        assert!(!position_save_required(Some(Duration::from_secs(10)), Duration::from_secs(11), false));
        assert_eq!(playback_progress(Duration::from_secs(30), Duration::from_secs(120)), 25.0);
        assert_eq!(playback_progress(Duration::from_secs(200), Duration::from_secs(120)), 100.0);
        assert_eq!(playback_progress(Duration::from_secs(10), Duration::ZERO), 0.0);
    }

    #[test]
    fn chapter_progress_seek_stays_inside_the_active_chapter() {
        let start = Duration::from_secs(120);
        let end = Duration::from_secs(240);
        assert_eq!(chapter_position_at_fraction(start, end, -1.0), start);
        assert_eq!(chapter_position_at_fraction(start, end, 0.25), Duration::from_secs(150));
        assert_eq!(chapter_position_at_fraction(start, end, 2.0), end);
        assert_eq!(chapter_position_at_fraction(start, end, f32::NAN), start);
        assert_eq!(chapter_position_at_fraction(end, start, 0.5), end);
    }

    #[test]
    fn playback_rate_labels_are_compact() {
        assert_eq!(format_playback_rate(0.75), "0.75×");
        assert_eq!(format_playback_rate(1.0), "1×");
        assert_eq!(format_playback_rate(1.5), "1.5×");
        assert_eq!(format_playback_rate(2.0), "2×");
    }
}
