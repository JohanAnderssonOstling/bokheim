use std::cell::Cell;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    App, Bounds, Context, Entity, EventEmitter, FileDialogFilter, FontWeight, Image, ImageFormat, IntoElement, MouseButton, ObjectFit, PathPromptOptions, Pixels, Render, SharedString, Styled, StyledImage, Window, div, img, prelude::*, px,
    relative,
};
mod audio_engine;
mod playback_controller;
pub use audiobook_library as providers;

use audio_engine::AudioEngine;
pub use audiobook_library::AudiobookChapter;
use audiobook_library::ChapterOrigin;
use gpui_component::{ElementExt, Icon, IconName};
use playback_controller::PlaybackController;
use ui_components::ScrollableElement;

const POSITION_SAVE_INTERVAL: Duration = Duration::from_secs(5);
pub type SavePlaybackPosition = Rc<dyn Fn(Duration, f32)>;

pub trait AudiobookReadSeek: Read + Seek + Send + Sync {}
impl<T: Read + Seek + Send + Sync> AudiobookReadSeek for T {}

pub struct AudiobookSource {
    pub display_path: PathBuf,
    reader: Box<dyn AudiobookReadSeek>,
}

impl AudiobookSource {
    pub fn new(reader: impl AudiobookReadSeek + 'static, display_path: PathBuf) -> Self {
        Self { display_path, reader: Box::new(reader) }
    }

    pub fn from_path(path: PathBuf) -> Result<Self, String> {
        let reader = File::open(&path).map_err(|error| format!("Could not open M4B: {error}"))?;
        Ok(Self::new(reader, path))
    }
}

#[derive(Clone)]
pub struct AudiobookPublication {
    pub path: PathBuf,
    pub title: String,
    pub author: Option<String>,
    pub narrator: Option<String>,
    pub duration: Duration,
    pub chapters: Vec<AudiobookChapter>,
    pub chapter_origin: ChapterOrigin,
    pub cover: Option<Arc<Image>>,
}

pub struct AudiobookPlayer {
    publication: Option<AudiobookPublication>,
    controller: Option<PlaybackController>,
    error: Option<String>,
    loading: bool,
    speed_menu_open: bool,
    show_file_picker: bool,
    save_position: Option<SavePlaybackPosition>,
    last_saved_position: Option<Duration>,
}

pub enum AudiobookDockEvent {
    Close,
}

/// Window-shell presentation for one persistent audiobook playback session.
///
/// The dock observes the player entity instead of owning a second copy of its
/// playback state. Expanding and collapsing therefore never interrupts audio.
pub struct AudiobookDock {
    player: Entity<AudiobookPlayer>,
    expanded: bool,
}

impl EventEmitter<AudiobookDockEvent> for AudiobookDock {}

impl AudiobookDock {
    fn new(player: Entity<AudiobookPlayer>, cx: &mut Context<Self>) -> Self {
        cx.observe(&player, |_, _, cx| cx.notify()).detach();
        Self { player, expanded: false }
    }

    fn toggle_expanded(&mut self, cx: &mut Context<Self>) {
        self.expanded = !self.expanded;
        cx.notify();
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        self.player.update(cx, |player, _| player.persist_position(true));
        cx.emit(AudiobookDockEvent::Close);
    }
}

impl AudiobookPlayer {
    pub fn new(initial_path: Option<PathBuf>, cx: &mut Context<Self>) -> Self {
        let mut player = Self { publication: None, controller: None, error: None, loading: false, speed_menu_open: false, show_file_picker: true, save_position: None, last_saved_position: None };

        if let Some(path) = initial_path {
            player.load_path(path);
        }

        let timer = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            loop {
                timer.timer(Duration::from_millis(250)).await;
                if this
                    .update(cx, |this, cx| {
                        this.refresh_playback_state();
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        player
    }

    pub fn new_library_book(path: PathBuf, cx: &mut Context<Self>) -> Self {
        let mut player = Self::new(Some(path), cx);
        player.show_file_picker = false;
        player
    }

    pub fn publication(&self) -> Option<&AudiobookPublication> {
        self.publication.as_ref()
    }

    pub fn position(&self) -> Duration {
        self.controller.as_ref().map(PlaybackController::position).unwrap_or_default()
    }

    pub fn load_path(&mut self, path: PathBuf) {
        match AudiobookSource::from_path(path) {
            Ok(source) => self.load_source(source),
            Err(error) => self.error = Some(error),
        }
    }

    pub fn load_source(&mut self, source: AudiobookSource) {
        self.load_source_at(source, Duration::ZERO);
    }

    fn load_source_at(&mut self, mut source: AudiobookSource, initial_position: Duration) {
        self.loading = true;
        self.error = None;

        let path = source.display_path.clone();
        match Self::open_publication(&path, &mut source.reader).and_then(|publication| {
            source.reader.seek(SeekFrom::Start(0)).map_err(|error| format!("Could not rewind M4B: {error}"))?;
            let playback = AudioEngine::open(source.reader).map_err(|error| format!("Could not open M4B audio: {error}"))?;
            Ok((publication, playback))
        }) {
            Ok((publication, playback)) => {
                self.speed_menu_open = false;
                let mut controller = PlaybackController::new(publication.duration, publication.chapters.clone(), playback);
                let initial_position = initial_position.min(publication.duration);
                if !initial_position.is_zero()
                    && let Err(error) = controller.seek_to(initial_position)
                {
                    self.error = Some(format!("Could not restore audiobook position: {error}"));
                }
                self.last_saved_position = Some(initial_position);
                self.controller = Some(controller);
                self.publication = Some(publication);
            }
            Err(error) => self.error = Some(error),
        }

        self.loading = false;
    }

    fn open_publication(path: &Path, reader: &mut (impl Read + Seek)) -> Result<AudiobookPublication, String> {
        if !path.extension().and_then(|value| value.to_str()).is_some_and(|value| value.eq_ignore_ascii_case("m4b")) {
            return Err("This standalone player only accepts .m4b files.".to_owned());
        }

        let tag = mp4ameta::Tag::read_from(reader).map_err(|error| format!("Could not read M4B metadata: {error}"))?;
        let duration = tag.duration();
        if duration.is_zero() {
            return Err("The M4B has no playable duration.".to_owned());
        }

        let title = tag
            .title()
            .or_else(|| tag.album())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .or_else(|| path.file_stem().and_then(|value| value.to_str()).map(str::to_owned))
            .unwrap_or_else(|| "Untitled audiobook".to_owned());
        let author = tag.album_artist().or_else(|| tag.artist()).map(str::to_owned);
        let narrator = tag.composer().map(str::to_owned);
        let cover = tag.artwork().map(|artwork| {
            let format = match artwork.fmt {
                mp4ameta::ImgFmt::Bmp => ImageFormat::Bmp,
                mp4ameta::ImgFmt::Jpeg => ImageFormat::Jpeg,
                mp4ameta::ImgFmt::Png => ImageFormat::Png,
            };
            Arc::new(Image::from_bytes(format, artwork.data.to_vec()))
        });

        let (chapters, chapter_origin) = publication_chapters(tag.chapters(), duration);

        Ok(AudiobookPublication { path: path.to_owned(), title, author, narrator, duration, chapters, chapter_origin, cover })
    }

    fn choose_file(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths_with_filters(
            PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some("Open M4B".into()) },
            vec![FileDialogFilter { name: "M4B audiobook".to_owned(), extensions: vec!["m4b".to_owned()] }],
        );
        cx.spawn(async move |this, cx| match receiver.await {
            Ok(Ok(Some(paths))) => {
                if let Some(path) = paths.into_iter().next() {
                    let _ = this.update(cx, |this, cx| {
                        this.load_path(path);
                        cx.notify();
                    });
                }
            }
            Ok(Err(error)) => {
                let _ = this.update(cx, |this, cx| {
                    this.error = Some(format!("Could not open the file picker: {error}"));
                    cx.notify();
                });
            }
            _ => {}
        })
        .detach();
    }

    fn refresh_playback_state(&mut self) {
        let error = self.controller.as_mut().and_then(PlaybackController::refresh);
        if let Some(error) = error {
            self.error = Some(format!("Audio decoding failed: {error}"));
        }
        self.persist_position(false);
    }

    fn toggle_playback(&mut self) {
        let Some(controller) = self.controller.as_mut() else { return };
        match controller.toggle() {
            Ok(()) => self.persist_position(true),
            Err(error) => self.error = Some(error),
        }
    }

    fn seek_by(&mut self, seconds: f64) {
        let Some(controller) = self.controller.as_mut() else { return };
        match controller.seek_by(seconds) {
            Ok(()) => self.persist_position(true),
            Err(error) => self.error = Some(error),
        }
    }

    fn seek_to(&mut self, position: Duration) {
        let Some(controller) = self.controller.as_mut() else { return };
        match controller.seek_to(position) {
            Ok(()) => self.persist_position(true),
            Err(error) => self.error = Some(error),
        }
    }

    fn previous_chapter(&mut self) {
        let Some(controller) = self.controller.as_mut() else { return };
        match controller.previous_chapter() {
            Ok(()) => self.persist_position(true),
            Err(error) => self.error = Some(error),
        }
    }

    fn next_chapter(&mut self) {
        let Some(controller) = self.controller.as_mut() else { return };
        match controller.next_chapter() {
            Ok(()) => self.persist_position(true),
            Err(error) => self.error = Some(error),
        }
    }

    fn set_rate(&mut self, rate: f64) {
        let Some(controller) = self.controller.as_mut() else { return };
        match controller.set_rate(rate) {
            Ok(()) => self.speed_menu_open = false,
            Err(error) => self.error = Some(error),
        }
    }

    fn toggle_speed_menu(&mut self) {
        self.speed_menu_open = !self.speed_menu_open;
    }

    fn is_playing(&self) -> bool {
        self.controller.as_ref().is_some_and(PlaybackController::is_playing)
    }

    fn active_chapter_index(&self) -> usize {
        self.controller.as_ref().map(PlaybackController::active_chapter_index).unwrap_or(0)
    }

    fn persist_position(&mut self, force: bool) {
        let (Some(save), Some(publication), Some(controller)) = (&self.save_position, &self.publication, &self.controller) else { return };
        let position = controller.position().min(publication.duration);
        if !force && !position_save_due(self.last_saved_position, position) {
            return;
        }
        let progress = playback_progress(position, publication.duration);
        save(position, progress);
        self.last_saved_position = Some(position);
    }
}

impl Drop for AudiobookPlayer {
    fn drop(&mut self) {
        self.persist_position(true);
    }
}

impl Render for AudiobookPlayer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = ui_components::browser_theme(cx);
        let target = cx.entity().downgrade();
        let open_target = target.clone();
        let play_target = target.clone();
        let back_target = target.clone();
        let forward_target = target.clone();
        let previous_chapter_target = target.clone();
        let next_chapter_target = target.clone();
        let rate_target = target.clone();

        let open_button = div()
            .id("open-m4b")
            .cursor_pointer()
            .px(px(12.0))
            .py(px(7.0))
            .rounded(px(0.0))
            .border_1()
            .border_color(theme.border)
            .bg(theme.surface)
            .text_color(theme.accent)
            .font_weight(FontWeight::SEMIBOLD)
            .child(if self.publication.is_some() { "Open another M4B" } else { "Open M4B" })
            .on_click(move |_, _, cx| {
                let _ = open_target.update(cx, |this, cx| this.choose_file(cx));
            });

        let Some(book) = self.publication.clone() else {
            return div().size_full().bg(theme.page_bg).text_color(theme.text).flex().items_center().justify_center().p(px(32.0)).child(
                div()
                    .w_full()
                    .max_w(px(520.0))
                    .p(px(34.0))
                    .rounded(px(0.0))
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.raised_bg)
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(14.0))
                    .child(div().text_size(px(12.0)).text_color(theme.accent).font_weight(FontWeight::BOLD).child("AUDIOBOOK READER"))
                    .child(div().text_size(px(30.0)).font_weight(FontWeight::SEMIBOLD).child("Open an M4B audiobook"))
                    .child(div().max_w(px(420.0)).text_center().text_color(theme.text_muted).line_height(relative(1.5)).child("Playback, embedded chapters, and pitch-preserving speed controls are available in the native reader."))
                    .child(open_button)
                    .when_some(self.error.clone(), |panel, error| panel.child(div().text_color(theme.danger).child(error))),
            );
        };

        let current_rate = self.controller.as_ref().map(PlaybackController::rate).unwrap_or(1.0);
        let active_chapter = self.active_chapter_index();
        let playing = self.is_playing();
        let position = self.position().min(book.duration);
        let chapter = &book.chapters[active_chapter];
        let chapter_start = chapter.start;
        let chapter_end = chapter.end;
        let chapter_duration = chapter_end.saturating_sub(chapter_start);
        let chapter_position = position.saturating_sub(chapter_start).min(chapter_duration);
        let progress = playback_progress(chapter_position, chapter_duration) / 100.0;
        let progress_target = target.clone();
        let progress_bounds = Rc::new(Cell::new(Bounds::<Pixels>::default()));
        let painted_progress_bounds = progress_bounds.clone();
        let clicked_progress_bounds = progress_bounds;
        let progress_track = div()
            .id("chapter-progress")
            .relative()
            .flex_1()
            .h(px(16.0))
            .cursor_pointer()
            .flex()
            .items_center()
            .on_prepaint(move |bounds, _, _| painted_progress_bounds.set(bounds))
            .on_click(move |event, _, cx| {
                let Some(mouse_position) = event.mouse_position() else { return };
                let bounds = clicked_progress_bounds.get();
                let width = f32::from(bounds.size.width);
                if width <= 0.0 {
                    return;
                }
                let fraction = f32::from(mouse_position.x - bounds.left()) / width;
                let position = chapter_position_at_fraction(chapter_start, chapter_end, fraction);
                let _ = progress_target.update(cx, |this, cx| {
                    this.seek_to(position);
                    cx.notify();
                });
            })
            .child(
                div().relative().w_full().h(px(6.0)).rounded(px(ui_components::RADIUS_SM)).bg(theme.raised_bg).child(div().absolute().left_0().top_0().h_full().w(relative(progress)).rounded(px(ui_components::RADIUS_SM)).bg(theme.accent)),
            );
        let progress_row = div()
            .w_full()
            .flex()
            .items_center()
            .gap(px(12.0))
            .text_size(px(12.0))
            .text_color(theme.text_muted)
            .child(div().w(px(58.0)).text_right().child(format_time(chapter_position)))
            .child(progress_track)
            .child(div().w(px(58.0)).child(format_time(chapter_duration)));

        let cover_art = if let Some(cover) = book.cover.clone() {
            div().w(px(128.0)).h(px(192.0)).flex_none().overflow_hidden().rounded(px(0.0)).border_1().border_color(theme.border).child(img(cover).size_full().object_fit(ObjectFit::Cover))
        } else {
            div()
                .w(px(128.0))
                .h(px(192.0))
                .flex_none()
                .rounded(px(0.0))
                .border_1()
                .border_color(theme.border)
                .bg(theme.accent_soft)
                .flex()
                .flex_col()
                .justify_between()
                .p(px(12.0))
                .child(div().text_size(px(10.0)).font_weight(FontWeight::BOLD).child("M4B"))
                .child(div().text_size(px(15.0)).font_weight(FontWeight::SEMIBOLD).line_height(relative(1.08)).child(book.title.clone()))
        };

        let speed_menu = playback_speed_menu(target.clone(), current_rate, theme);

        let mut chapter_list = div().flex().flex_col().gap(px(2.0)).child(
            div()
                .flex()
                .items_center()
                .gap(px(16.0))
                .px(px(14.0))
                .pb(px(6.0))
                .text_size(px(11.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.text_muted)
                .child(div().flex_1().min_w_0().child("Title"))
                .child(div().w(px(82.0)).text_right().child("Duration"))
                .child(div().w(px(82.0)).text_right().child("Start")),
        );
        for (index, chapter) in book.chapters.iter().enumerate() {
            let chapter_target = target.clone();
            let start = chapter.start;
            let duration = chapter.end.saturating_sub(chapter.start);
            chapter_list = chapter_list.child(
                div()
                    .id(("chapter", index))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(16.0))
                    .px(px(14.0))
                    .py(px(10.0))
                    .rounded(px(0.0))
                    .hover(|style| style.bg(theme.accent_soft))
                    .when(index == active_chapter, |row| row.bg(theme.accent_soft))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .font_weight(if index == active_chapter { FontWeight::SEMIBOLD } else { FontWeight::NORMAL })
                            .text_color(if index == active_chapter { theme.text } else { theme.text_muted })
                            .child(chapter_display_title(&chapter.title)),
                    )
                    .child(div().w(px(82.0)).text_right().text_size(px(12.0)).text_color(theme.text_muted).child(format_time(duration)))
                    .child(div().w(px(82.0)).text_right().text_size(px(12.0)).text_color(theme.text_muted).child(format_time(chapter.start)))
                    .on_click(move |_, _, cx| {
                        let _ = chapter_target.update(cx, |this, cx| {
                            this.seek_to(start);
                            cx.notify();
                        });
                    }),
            );
        }

        div()
            .size_full()
            .min_h_0()
            .relative()
            .bg(theme.page_bg)
            .text_color(theme.text)
            .flex()
            .flex_col()
            .child(
                div().flex_none().bg(theme.surface).border_b_1().border_color(theme.border).p(px(16.0)).flex().items_center().gap(px(20.0)).child(cover_art).child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .gap(px(7.0))
                        .child(div().text_size(px(28.0)).font_weight(FontWeight::SEMIBOLD).line_height(relative(1.08)).child(book.title.clone()))
                        .when_some(book.author.clone(), |details, author| details.child(div().text_color(theme.text_muted).child(author)))
                        .when_some(book.narrator.clone(), |details, narrator| details.child(div().text_color(theme.text_muted).child(format!("Narrated by {narrator}"))))
                        .when(self.show_file_picker, |details| details.child(div().mt(px(10.0)).child(open_button))),
                ),
            )
            .child(div().id("chapter-scroll").flex_1().min_h_0().overflow_y_scrollbar().px(px(10.0)).py(px(10.0)).child(chapter_list))
            .child(
                div()
                    .flex_none()
                    .relative()
                    .w_full()
                    .px(px(24.0))
                    .pt(px(15.0))
                    .pb(px(17.0))
                    .bg(theme.surface)
                    .border_t_1()
                    .border_color(theme.border)
                    .child(
                        div()
                            .relative()
                            .w_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap(px(6.0))
                            .child(transport_control("previous-chapter", chapter_icon(false, theme), theme).on_click(move |_, _, cx| {
                                let _ = previous_chapter_target.update(cx, |this, cx| {
                                    this.previous_chapter();
                                    cx.notify();
                                });
                            }))
                            .child(transport_control("seek-back", skip_icon(false, "15"), theme).on_click(move |_, _, cx| {
                                let _ = back_target.update(cx, |this, cx| {
                                    this.seek_by(-15.0);
                                    cx.notify();
                                });
                            }))
                            .child(
                                transport_control("toggle-play", Icon::new(if playing { IconName::Pause } else { IconName::Play }).size(px(22.0)), theme)
                                    .size(px(56.0))
                                    .bg(theme.accent)
                                    .text_color(theme.page_bg)
                                    .hover(|style| style.bg(theme.accent).text_color(theme.page_bg))
                                    .on_click(move |_, _, cx| {
                                        let _ = play_target.update(cx, |this, cx| {
                                            this.toggle_playback();
                                            cx.notify();
                                        });
                                    }),
                            )
                            .child(transport_control("seek-forward", skip_icon(true, "30"), theme).on_click(move |_, _, cx| {
                                let _ = forward_target.update(cx, |this, cx| {
                                    this.seek_by(30.0);
                                    cx.notify();
                                });
                            }))
                            .child(transport_control("next-chapter", chapter_icon(true, theme), theme).on_click(move |_, _, cx| {
                                let _ = next_chapter_target.update(cx, |this, cx| {
                                    this.next_chapter();
                                    cx.notify();
                                });
                            }))
                            .child(div().absolute().right_0().bottom(px(4.0)).when(self.speed_menu_open, |wrapper| wrapper.child(speed_menu)).child(speed_control(format_playback_rate(current_rate), self.speed_menu_open, theme).on_click(
                                move |_, _, cx| {
                                    let _ = rate_target.update(cx, |this, cx| {
                                        this.toggle_speed_menu();
                                        cx.notify();
                                    });
                                },
                            ))),
                    )
                    .child(div().mt(px(8.0)).child(progress_row)),
            )
            .when_some(self.error.clone(), |column, error| column.child(div().absolute().left(px(16.0)).right(px(16.0)).top(px(16.0)).p(px(10.0)).rounded(px(0.0)).bg(theme.accent_soft).text_color(theme.danger).child(error)))
    }
}

impl Render for AudiobookDock {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = ui_components::browser_theme(cx);
        let dock = cx.entity().downgrade();
        let close_dock = dock.clone();
        let expand_dock = dock;

        if self.expanded {
            return div()
                .absolute()
                .inset_0()
                .size_full()
                .min_h_0()
                .flex()
                .flex_col()
                .bg(theme.page_bg)
                .child(
                    div()
                        .h(px(40.0))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_end()
                        .gap(px(8.0))
                        .px(px(12.0))
                        .border_b_1()
                        .border_color(theme.border)
                        .child(div().id("audiobook-collapse").cursor_pointer().size(px(32.0)).flex().items_center().justify_center().child(Icon::new(IconName::ChevronDown).size(px(16.0))).on_click(move |_, _, cx| {
                            let _ = expand_dock.update(cx, |dock, cx| dock.toggle_expanded(cx));
                        }))
                        .child(div().id("audiobook-close-expanded").cursor_pointer().size(px(32.0)).flex().items_center().justify_center().child(Icon::new(IconName::Close).size(px(16.0))).on_click(move |_, _, cx| {
                            let _ = close_dock.update(cx, |dock, cx| dock.close(cx));
                        })),
                )
                .child(div().flex_1().min_h_0().child(self.player.clone()))
                .into_any_element();
        }

        let (publication, error, playing, position, current_rate, speed_menu_open) = {
            let player = self.player.read(cx);
            (player.publication.clone(), player.error.clone(), player.is_playing(), player.position(), player.controller.as_ref().map(PlaybackController::rate).unwrap_or(1.0), player.speed_menu_open)
        };

        let Some(book) = publication else {
            return div()
                .w_full()
                .h(px(72.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_between()
                .px(px(20.0))
                .border_t_1()
                .border_color(theme.border)
                .bg(theme.surface)
                .child(error.unwrap_or_else(|| "Opening audiobook…".to_owned()))
                .child(div().id("audiobook-close-loading").cursor_pointer().size(px(36.0)).flex().items_center().justify_center().child(Icon::new(IconName::Close).size(px(16.0))).on_click(move |_, _, cx| {
                    let _ = close_dock.update(cx, |dock, cx| dock.close(cx));
                }))
                .into_any_element();
        };

        let position = position.min(book.duration);
        let active_chapter = self.player.read(cx).active_chapter_index();
        let chapter = &book.chapters[active_chapter];
        let chapter_title = chapter_display_title(&chapter.title);
        let chapter_start = chapter.start;
        let chapter_end = chapter.end;
        let chapter_duration = chapter_end.saturating_sub(chapter_start);
        let chapter_position = position.saturating_sub(chapter_start).min(chapter_duration);
        let progress = playback_progress(chapter_position, chapter_duration) / 100.0;
        let player_target = self.player.downgrade();
        let play_target = player_target.clone();
        let back_target = player_target.clone();
        let forward_target = player_target.clone();
        let previous_target = player_target.clone();
        let next_target = player_target.clone();
        let rate_target = player_target.clone();
        let progress_target = self.player.downgrade();
        let progress_bounds = Rc::new(Cell::new(Bounds::<Pixels>::default()));
        let painted_progress_bounds = progress_bounds.clone();
        let clicked_progress_bounds = progress_bounds;

        let cover = match book.cover.clone() {
            Some(cover) => div().w(px(48.0)).h(px(64.0)).flex_none().overflow_hidden().border_1().border_color(theme.border).child(img(cover).size_full().object_fit(ObjectFit::Cover)),
            None => div()
                .w(px(48.0))
                .h(px(64.0))
                .flex_none()
                .overflow_hidden()
                .border_1()
                .border_color(theme.border)
                .p(px(5.0))
                .flex()
                .items_end()
                .text_size(px(9.0))
                .font_weight(FontWeight::SEMIBOLD)
                .line_height(relative(1.05))
                .child(book.title.clone()),
        };

        let progress_track = div()
            .id("audiobook-dock-progress")
            .relative()
            .flex_1()
            .min_w_0()
            .h(px(16.0))
            .cursor_pointer()
            .flex()
            .items_center()
            .on_prepaint(move |bounds, _, _| painted_progress_bounds.set(bounds))
            .on_click(move |event, _, cx| {
                let Some(mouse_position) = event.mouse_position() else { return };
                let bounds = clicked_progress_bounds.get();
                let width = f32::from(bounds.size.width);
                if width <= 0.0 {
                    return;
                }
                let fraction = f32::from(mouse_position.x - bounds.left()) / width;
                let target = chapter_position_at_fraction(chapter_start, chapter_end, fraction);
                let _ = progress_target.update(cx, |player, cx| {
                    player.seek_to(target);
                    cx.notify();
                });
            })
            .child(
                div().relative().w_full().h(px(6.0)).rounded(px(ui_components::RADIUS_SM)).bg(theme.raised_bg).child(div().absolute().left_0().top_0().h_full().w(relative(progress)).rounded(px(ui_components::RADIUS_SM)).bg(theme.accent)),
            );

        let speed_menu = playback_speed_menu(player_target, current_rate, theme);

        div()
            .w_full()
            .h(px(96.0))
            .relative()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(16.0))
            .px(px(12.0))
            .py(px(8.0))
            .border_t_1()
            .border_color(theme.border)
            .bg(theme.surface)
            .text_color(theme.text)
            .child(
                div()
                    .id("audiobook-book-details")
                    .w(px(240.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .cursor_pointer()
                    .on_click(move |_, _, cx| {
                        let _ = expand_dock.update(cx, |dock, cx| dock.toggle_expanded(cx));
                    })
                    .child(cover)
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(px(3.0))
                            .child(div().overflow_hidden().whitespace_nowrap().text_ellipsis().font_weight(FontWeight::SEMIBOLD).child(book.title.clone()))
                            .child(div().overflow_hidden().whitespace_nowrap().text_ellipsis().text_size(px(12.0)).text_color(theme.text_muted).child(chapter_title)),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .justify_center()
                    .gap(px(4.0))
                    .child(
                        div()
                            .relative()
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap(px(2.0))
                            .child(transport_control("dock-previous-chapter", chapter_icon(false, theme), theme).on_click(move |_, _, cx| {
                                let _ = previous_target.update(cx, |player, cx| {
                                    player.previous_chapter();
                                    cx.notify();
                                });
                            }))
                            .child(transport_control("dock-seek-back", skip_icon(false, "15"), theme).on_click(move |_, _, cx| {
                                let _ = back_target.update(cx, |player, cx| {
                                    player.seek_by(-15.0);
                                    cx.notify();
                                });
                            }))
                            .child(transport_control("dock-toggle-play", Icon::new(if playing { IconName::Pause } else { IconName::Play }).size(px(20.0)), theme).bg(theme.accent).text_color(theme.page_bg).on_click(move |_, _, cx| {
                                let _ = play_target.update(cx, |player, cx| {
                                    player.toggle_playback();
                                    cx.notify();
                                });
                            }))
                            .child(transport_control("dock-seek-forward", skip_icon(true, "30"), theme).on_click(move |_, _, cx| {
                                let _ = forward_target.update(cx, |player, cx| {
                                    player.seek_by(30.0);
                                    cx.notify();
                                });
                            }))
                            .child(transport_control("dock-next-chapter", chapter_icon(true, theme), theme).on_click(move |_, _, cx| {
                                let _ = next_target.update(cx, |player, cx| {
                                    player.next_chapter();
                                    cx.notify();
                                });
                            }))
                            .child(div().relative().ml(px(6.0)).when(speed_menu_open, |wrapper| wrapper.child(speed_menu)).child(speed_control(format_playback_rate(current_rate), speed_menu_open, theme).on_click(move |_, _, cx| {
                                let _ = rate_target.update(cx, |player, cx| {
                                    player.toggle_speed_menu();
                                    cx.notify();
                                });
                            }))),
                    )
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap(px(12.0))
                            .text_size(px(12.0))
                            .text_color(theme.text_muted)
                            .child(div().w(px(58.0)).flex_none().text_right().child(format_time(chapter_position)))
                            .child(progress_track)
                            .child(div().w(px(58.0)).flex_none().child(format_time(chapter_duration))),
                    ),
            )
            .child(div().flex_none().flex().items_center().gap(px(2.0)).child(
                div().id("audiobook-close").cursor_pointer().size(px(36.0)).flex_none().flex().items_center().justify_center().child(Icon::new(IconName::Close).size(px(16.0))).on_click(move |_, _, cx| {
                    let _ = close_dock.update(cx, |dock, cx| dock.close(cx));
                }),
            ))
            .when_some(error, |dock, error| dock.child(div().absolute().left(px(16.0)).bottom(px(104.0)).p(px(8.0)).bg(theme.raised_bg).text_color(theme.danger).child(error)))
            .into_any_element()
    }
}

fn speed_control(label: impl Into<SharedString>, open: bool, theme: ui_components::BrowserTheme) -> gpui::Stateful<gpui::Div> {
    div()
        .id("playback-rate")
        .cursor_pointer()
        .h(px(32.0))
        .min_w(px(48.0))
        .px(px(12.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(ui_components::RADIUS_SM))
        .border_1()
        .border_color(if open { theme.accent } else { theme.border })
        .bg(if open { theme.accent_soft } else { gpui::Hsla::transparent_black() })
        .text_color(if open { theme.accent } else { theme.text_muted })
        .hover(|style| style.border_color(theme.accent).bg(theme.accent_soft).text_color(theme.accent))
        .text_size(px(12.0))
        .font_weight(FontWeight::BOLD)
        .child(label.into())
}

fn playback_speed_menu(target: gpui::WeakEntity<AudiobookPlayer>, current_rate: f64, theme: ui_components::BrowserTheme) -> gpui::Div {
    let mut menu = div()
        .absolute()
        .right_0()
        .bottom(px(42.0))
        .w(px(176.0))
        .p(px(8.0))
        .rounded(px(0.0))
        .border_1()
        .border_color(theme.border)
        .bg(theme.raised_bg)
        .shadow_lg()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .child(div().px(px(10.0)).pt(px(5.0)).pb(px(7.0)).text_size(px(12.0)).font_weight(FontWeight::SEMIBOLD).text_color(theme.text).child("Playback speed"))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
    for rate in [0.75, 1.0, 1.25, 1.5, 1.75, 2.0] {
        let option_target = target.clone();
        menu = menu.child(
            div()
                .id(format!("speed-{rate}"))
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_between()
                .px(px(12.0))
                .py(px(9.0))
                .rounded(px(0.0))
                .text_size(px(13.0))
                .hover(|style| style.bg(theme.accent_soft))
                .when((rate - current_rate).abs() < f64::EPSILON, |row| row.bg(theme.accent_soft).text_color(theme.accent))
                .child(format_playback_rate(rate))
                .when((rate - current_rate).abs() < f64::EPSILON, |row| row.child(Icon::new(IconName::Check).size(px(15.0))))
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    let _ = option_target.update(cx, |player, cx| {
                        player.set_rate(rate);
                        cx.notify();
                    });
                }),
        );
    }
    menu
}

fn format_playback_rate(rate: f64) -> String {
    let formatted = format!("{rate:.2}");
    format!("{}×", formatted.trim_end_matches('0').trim_end_matches('.'))
}

fn transport_control(id: impl Into<SharedString>, content: impl IntoElement, theme: ui_components::BrowserTheme) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id.into())
        .size(px(40.0))
        .flex_none()
        .cursor_pointer()
        .rounded(px(ui_components::RADIUS_SM))
        .flex()
        .items_center()
        .justify_center()
        .bg(gpui::rgba(0x00000000))
        .text_color(theme.text_muted)
        .font_weight(FontWeight::BOLD)
        .hover(|style| style.bg(theme.accent_soft).text_color(theme.text))
        .child(content)
}

fn chapter_icon(forward: bool, theme: ui_components::BrowserTheme) -> gpui::Div {
    let chevron = Icon::new(if forward { IconName::ChevronRight } else { IconName::ChevronLeft }).size(px(19.0));
    let bar = div().w(px(2.0)).h(px(17.0)).rounded(px(ui_components::RADIUS_SM)).bg(theme.text_muted);
    let icon = div().flex().items_center().gap(px(-4.0));
    if forward { icon.child(chevron).child(bar) } else { icon.child(bar).child(chevron) }
}

fn skip_icon(forward: bool, seconds: &'static str) -> gpui::Div {
    let chevron = Icon::new(if forward { IconName::ChevronRight } else { IconName::ChevronLeft }).size(px(14.0));
    let label = div().text_size(px(10.0)).child(seconds);
    let icon = div().flex().items_center().gap(px(-2.0));
    if forward { icon.child(label).child(chevron) } else { icon.child(chevron).child(label) }
}

#[cfg(test)]
fn metadata_lines(book: &AudiobookPublication) -> Vec<String> {
    let mut values = Vec::new();
    if let Some(author) = &book.author {
        values.push(author.clone());
    }
    if let Some(narrator) = &book.narrator {
        values.push(format!("Narrated by {narrator}"));
    }
    values
}

fn position_save_due(previous: Option<Duration>, position: Duration) -> bool {
    previous.is_none_or(|previous| previous.abs_diff(position) >= POSITION_SAVE_INTERVAL)
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

fn publication_chapters(embedded: &[mp4ameta::Chapter], duration: Duration) -> (Vec<AudiobookChapter>, ChapterOrigin) {
    if embedded.is_empty() {
        return (vec![AudiobookChapter { title: "Full audiobook".to_owned(), start: Duration::ZERO, end: duration }], ChapterOrigin::FullAudiobookFallback);
    }

    let mut normalized = embedded
        .iter()
        .map(|chapter| {
            let start = chapter.start.min(duration);
            let title = (!chapter.title.trim().is_empty()).then(|| chapter.title.trim().to_owned());
            (start, title)
        })
        .collect::<Vec<_>>();
    normalized.sort_by_key(|(start, _)| *start);
    normalized.dedup_by(|current, previous| current.0 == previous.0);
    let chapters = normalized
        .iter()
        .enumerate()
        .map(|(index, (start, title))| {
            let end = normalized.get(index + 1).map(|(next, _)| *next).unwrap_or(duration);
            AudiobookChapter { title: title.clone().unwrap_or_else(|| format!("Chapter {}", index + 1)), start: *start, end }
        })
        .collect();
    (chapters, ChapterOrigin::EmbeddedM4b)
}

pub fn open_player(initial_path: Option<PathBuf>, cx: &mut App) -> Entity<AudiobookPlayer> {
    cx.new(|cx| AudiobookPlayer::new(initial_path, cx))
}

pub fn open_library_player(path: PathBuf, cx: &mut App) -> Entity<AudiobookPlayer> {
    cx.new(|cx| AudiobookPlayer::new_library_book(path, cx))
}

pub fn open_library_source(source: AudiobookSource, initial_position: Duration, save_position: SavePlaybackPosition, cx: &mut App) -> Entity<AudiobookPlayer> {
    cx.new(|cx| {
        let mut player = AudiobookPlayer::new(None, cx);
        player.show_file_picker = false;
        player.save_position = Some(save_position);
        player.load_source_at(source, initial_position);
        player
    })
}

pub fn open_library_dock(source: AudiobookSource, initial_position: Duration, save_position: SavePlaybackPosition, cx: &mut App) -> Entity<AudiobookDock> {
    let player = open_library_source(source, initial_position, save_position, cx);
    cx.new(|cx| AudiobookDock::new(player, cx))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mp4ameta::Chapter;

    fn chapter(title: &str, start: u64, end: u64) -> AudiobookChapter {
        AudiobookChapter { title: title.to_owned(), start: Duration::from_secs(start), end: Duration::from_secs(end) }
    }

    #[test]
    fn empty_embedded_chapters_create_full_book_fallback() {
        let duration = Duration::from_secs(90);
        let (chapters, origin) = publication_chapters(&[], duration);

        assert_eq!(origin, ChapterOrigin::FullAudiobookFallback);
        assert_eq!(chapters, vec![chapter("Full audiobook", 0, 90)]);
    }

    #[test]
    fn embedded_chapters_are_sorted_trimmed_and_bounded() {
        let embedded = vec![Chapter::new(Duration::from_secs(50), "  Last  "), Chapter::new(Duration::from_secs(10), "  "), Chapter::new(Duration::from_secs(120), "Past end")];
        let (chapters, origin) = publication_chapters(&embedded, Duration::from_secs(100));

        assert_eq!(origin, ChapterOrigin::EmbeddedM4b);
        assert_eq!(chapters, vec![chapter("Chapter 1", 10, 50), chapter("Last", 50, 100), chapter("Past end", 100, 100)]);
    }

    #[test]
    fn duplicate_and_out_of_range_chapter_starts_are_collapsed() {
        let embedded =
            vec![Chapter::new(Duration::from_secs(10), "First at ten"), Chapter::new(Duration::from_secs(10), "Duplicate"), Chapter::new(Duration::from_secs(200), "Past end"), Chapter::new(Duration::from_secs(300), "Also past end")];

        let (chapters, _) = publication_chapters(&embedded, Duration::from_secs(100));

        assert_eq!(chapters, vec![chapter("First at ten", 10, 100), chapter("Past end", 100, 100)]);
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
    fn main_metadata_only_shows_people_without_slash_separators() {
        let book = AudiobookPublication {
            path: PathBuf::from("hidden-file-name.m4b"),
            title: "Book".to_owned(),
            author: Some("Author".to_owned()),
            narrator: Some("Narrator".to_owned()),
            duration: Duration::from_secs(60),
            chapters: vec![chapter("One", 0, 60)],
            chapter_origin: ChapterOrigin::EmbeddedM4b,
            cover: None,
        };

        assert_eq!(metadata_lines(&book), ["Author", "Narrated by Narrator"]);
        assert!(metadata_lines(&book).iter().all(|line| !line.contains('/')));
        assert!(metadata_lines(&book).iter().all(|line| !line.contains("chapter")));
        assert!(metadata_lines(&book).iter().all(|line| !line.contains("M4B")));
    }

    #[test]
    fn playback_position_persistence_is_bounded_and_throttled() {
        assert!(position_save_due(None, Duration::ZERO));
        assert!(!position_save_due(Some(Duration::from_secs(10)), Duration::from_secs(14)));
        assert!(position_save_due(Some(Duration::from_secs(10)), Duration::from_secs(15)));
        assert!(position_save_due(Some(Duration::from_secs(10)), Duration::from_secs(5)));
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
