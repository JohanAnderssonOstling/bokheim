use super::*;
use gpui::{EventEmitter, relative, rems};
use gpui_component::menu::DropdownMenu as _;
use std::rc::Rc;
use ui_components::{FlexItem as _, PLAYER_DOCK_CAPTION_SIZE_REM, PLAYER_DOCK_TEXT_SIZE_REM, PlaybackTransportAction, SPACE_SM, SPACE_XXS, TransportLayout};

pub enum DockAction {
    /// The wide dock's title was clicked: open the book's detail page. A
    /// phone opens its own full-screen player instead.
    Expand,
    Close,
}

/// The cover's square box, in every layout. Square because audiobook art is,
/// and fixed so the bar's height never shifts with the cover a given book
/// happens to have, or is missing one to draw at all. The docked bar and the
/// desktop dock are this tall.
const COVER_SIZE_REM: f32 = ui_components::PLAYER_DOCK_HEIGHT_REM;
/// How far a touch must travel sideways across the docked bar to change chapter.
const SWIPE_THRESHOLD: f32 = 40.0;

/// Anchor compact sheets to the window. The dock itself is only a short
/// footer, so using it as the sheet's containing block clips the menu.
fn page_sheet(sheet: gpui::AnyElement, window: &Window) -> gpui::AnyElement {
    let viewport = window.viewport_size();
    let usable_height = viewport.height - window.insets().safe_area.bottom;
    gpui::anchored().position(gpui::point(px(0.0), px(0.0))).anchor(gpui::Anchor::TopLeft).child(div().relative().w(viewport.width).h(usable_height).child(sheet)).into_any_element()
}

/// The phone's expanded player covers the whole window, bottom navigation
/// included, and keeps its controls clear of the system bars itself.
fn full_window(page: gpui::Div, window: &Window) -> gpui::AnyElement {
    let viewport = window.viewport_size();
    let safe_area = window.insets().safe_area;
    gpui::anchored().position(gpui::point(px(0.0), px(0.0))).anchor(gpui::Anchor::TopLeft).child(page.w(viewport.width).h(viewport.height).pt(safe_area.top).pb(safe_area.bottom)).into_any_element()
}

/// Compact projection of the retained playback entity. It has no decoder or
/// streaming resources of its own.
pub struct AudiobookDock {
    player: Entity<PlaybackSession>,
    /// Drawn as the controls inside the playing book's detail page rather
    /// than as the dock along the bottom of the window.
    inline: bool,
    speed_menu_open: bool,
    sleep_menu_open: bool,
    chapters_menu_open: bool,
    /// On a phone, whether the bar has been opened into the full-screen
    /// player.
    mobile_expanded: bool,
    /// The fraction of the current chapter the pointer/finger is dragging the
    /// position to. Present only during a drag, and what the progress bar
    /// reads while it is.
    scrub: Option<f32>,
    /// Where the progress track was last laid out, so a drag that has left it
    /// can still be turned into a position.
    progress_bounds: Rc<Cell<Bounds<Pixels>>>,
    /// The distance a touch swipe on the docked bar has travelled, while one is
    /// in progress.
    swipe: Option<gpui::Point<f32>>,
}

impl AudiobookDock {
    pub fn new(player: Entity<PlaybackSession>, cx: &mut Context<Self>) -> Self {
        Self::create(player, false, cx)
    }

    /// The same controls, for the playing book's detail page.
    pub fn inline(player: Entity<PlaybackSession>, cx: &mut Context<Self>) -> Self {
        Self::create(player, true, cx)
    }

    fn create(player: Entity<PlaybackSession>, inline: bool, cx: &mut Context<Self>) -> Self {
        cx.observe(&player, |_, _, cx| cx.notify()).detach();
        Self { player, inline, speed_menu_open: false, sleep_menu_open: false, chapters_menu_open: false, mobile_expanded: false, scrub: None, progress_bounds: Rc::new(Cell::new(Bounds::default())), swipe: None }
    }

    /// Collapses the full-screen player back to the docked bar, e.g.
    /// on Back. Returns whether it was open.
    pub fn collapse_mobile(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.mobile_expanded {
            return false;
        }
        self.mobile_expanded = false;
        self.speed_menu_open = false;
        self.sleep_menu_open = false;
        self.chapters_menu_open = false;
        cx.notify();
        true
    }

    /// A horizontal swipe across the docked bar changes chapter when the finger
    /// lifts: leftwards to the next chapter, rightwards to the previous one.
    fn handle_swipe(&mut self, event: &gpui::ScrollWheelEvent, cx: &mut Context<Self>) {
        if let gpui::ScrollDelta::Pages(delta) = event.delta {
            if self.swipe.take().is_some() && delta.x != 0.0 {
                if delta.x < 0.0 { self.next_chapter(cx) } else { self.previous_chapter(cx) }
            }
            cx.stop_propagation();
            return;
        }
        let gpui::ScrollDelta::Pixels(delta) = event.delta else { return };
        let delta = gpui::point(f32::from(delta.x), f32::from(delta.y));
        match event.touch_phase {
            gpui::TouchPhase::Started => self.swipe = Some(delta),
            gpui::TouchPhase::Moved => {
                if let Some(swipe) = &mut self.swipe {
                    *swipe = gpui::point(swipe.x + delta.x, swipe.y + delta.y);
                }
            }
            gpui::TouchPhase::Ended => {
                let Some(swipe) = self.swipe.take() else { return };
                if swipe.x.abs() < SWIPE_THRESHOLD || swipe.x.abs() < swipe.y.abs() {
                    return;
                }
                if swipe.x < 0.0 { self.next_chapter(cx) } else { self.previous_chapter(cx) }
            }
            gpui::TouchPhase::Cancelled => self.swipe = None,
        }
        cx.stop_propagation();
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
        cx.notify();
    }

    /// Follows the pointer while a press is dragging. A pointer merely passing
    /// over the bar moves nothing: the position changes only by pressing.
    fn hover_scrub(&mut self, x: Pixels, cx: &mut Context<Self>) {
        if self.scrub.is_none() {
            return;
        }
        let Some(fraction) = self.track_fraction(x) else { return };
        if self.scrub != Some(fraction) {
            self.scrub = Some(fraction);
            cx.notify();
        }
    }

    /// Commits the drag: the seek happens once, where the pointer was released.
    fn finish_scrub(&mut self, cx: &mut Context<Self>) {
        let Some(fraction) = self.scrub.take() else { return };
        self.player.update(cx, |player, cx| player.seek_in_chapter(fraction, cx));
        cx.notify();
    }

    /// Chapter skip, the seconds skips and play, at the size `layout` gives them.
    fn transport(&self, playing: bool, layout: TransportLayout, theme: ui_components::BrowserTheme, cx: &Context<Self>) -> Vec<(PlaybackTransportAction, gpui::AnyElement)> {
        ui_components::playback_transport_buttons("now-playing", playing, layout, Some(["Previous chapter", "Next chapter"]), ui_components::PlaybackSkipKind::Seconds, theme)
            .into_iter()
            .map(|(action, button)| {
                let button = button
                    .on_click(cx.listener(move |dock, _, _, cx| match action {
                        PlaybackTransportAction::Previous => dock.previous_chapter(cx),
                        PlaybackTransportAction::Back => dock.skip(super::SKIP_BACK_SECONDS, cx),
                        PlaybackTransportAction::PlayPause => dock.toggle_playback(cx),
                        PlaybackTransportAction::Forward => dock.skip(super::SKIP_FORWARD_SECONDS, cx),
                        PlaybackTransportAction::Next => dock.next_chapter(cx),
                    }))
                    .into_any_element();
                (action, button)
            })
            .collect()
    }

    /// Makes a progress track scrubbable. Only one is on screen at a time, so
    /// they share an id and the recorded bounds.
    fn scrubbable(&self, track: gpui::Div, cx: &Context<Self>) -> gpui::Stateful<gpui::Div> {
        let progress_bounds = self.progress_bounds.clone();
        track
            .id("audiobook-dock-progress")
            .cursor_pointer()
            .on_prepaint(move |bounds, _, _| progress_bounds.set(bounds))
            // Pressing starts a scrub rather than seeking outright, so a
            // press-and-drag gesture is one action: the seek happens once,
            // wherever the finger/pointer has got to when it lifts.
            .on_mouse_down(MouseButton::Left, cx.listener(|dock, event: &gpui::MouseDownEvent, _, cx| dock.begin_scrub(event.position.x, cx)))
            .on_mouse_move(cx.listener(|dock, event: &MouseMoveEvent, _, cx| dock.hover_scrub(event.position.x, cx)))
            .on_mouse_up(MouseButton::Left, cx.listener(|dock, _, _, cx| dock.finish_scrub(cx)))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|dock, _, _, cx| dock.finish_scrub(cx)))
    }
}

impl EventEmitter<DockAction> for AudiobookDock {}

/// A window-wide mouse listener installed for the duration of a scrub, so a
/// drag that strays off the (thin) progress bar still tracks. Releasing off
/// the bar is the track's own `on_mouse_up_out`.
fn dock_scrub_capture(cx: &Context<AudiobookDock>) -> impl IntoElement {
    let hover = cx.listener(|dock, event: &MouseMoveEvent, _, cx| dock.hover_scrub(event.position.x, cx));
    canvas(
        |_, _, _| (),
        move |_, _: (), window, _| {
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                if phase == DispatchPhase::Bubble {
                    hover(event, window, cx);
                }
            });
        },
    )
    .absolute()
    .size_full()
}

impl AudiobookDock {
    fn toggle_playback(&mut self, cx: &mut Context<Self>) {
        self.player.update(cx, |player, cx| player.request_toggle(cx));
    }
    fn skip(&mut self, seconds: f64, cx: &mut Context<Self>) {
        self.player.update(cx, |player, cx| player.skip(seconds, cx));
    }
    fn previous_chapter(&mut self, cx: &mut Context<Self>) {
        self.player.update(cx, |player, cx| player.previous_chapter(cx));
    }
    fn next_chapter(&mut self, cx: &mut Context<Self>) {
        self.player.update(cx, |player, cx| player.next_chapter(cx));
    }
    /// A choice from a compact sheet: the dropdowns talk to the player
    /// directly, but a sheet is the dock's to close.
    fn set_rate(&mut self, rate: f64, cx: &mut Context<Self>) {
        self.player.update(cx, |player, cx| player.set_rate(rate, cx));
        self.speed_menu_open = false;
        cx.notify();
    }
    fn choose_sleep(&mut self, choice: super::SleepChoice, cx: &mut Context<Self>) {
        self.player.update(cx, |player, cx| player.choose_sleep(choice, cx));
        self.sleep_menu_open = false;
        cx.notify();
    }
    fn choose_chapter(&mut self, index: usize, cx: &mut Context<Self>) {
        self.player.update(cx, |player, cx| player.go_to_chapter(index, cx));
        self.chapters_menu_open = false;
        cx.notify();
    }
    fn toggle_sleep_menu(&mut self, cx: &mut Context<Self>) {
        self.sleep_menu_open = !self.sleep_menu_open;
        self.speed_menu_open = false;
        self.chapters_menu_open = false;
        cx.notify();
    }
    fn toggle_speed_menu(&mut self, cx: &mut Context<Self>) {
        self.speed_menu_open = !self.speed_menu_open;
        self.sleep_menu_open = false;
        self.chapters_menu_open = false;
        cx.notify();
    }
    fn toggle_chapters_menu(&mut self, cx: &mut Context<Self>) {
        self.chapters_menu_open = !self.chapters_menu_open;
        self.speed_menu_open = false;
        self.sleep_menu_open = false;
        cx.notify();
    }
}

/// Title over the current chapter, both truncated: the same block in every
/// layout.
fn title_block(title: String, chapter: String, theme: ui_components::BrowserTheme) -> gpui::Div {
    ui_components::column(0.0)
        .fill()
        .child(ui_components::single_line(title).text_size(rems(PLAYER_DOCK_TEXT_SIZE_REM)).line_height(relative(1.25)))
        .child(ui_components::caption_line(chapter, theme).text_size(rems(PLAYER_DOCK_CAPTION_SIZE_REM)).line_height(relative(1.25)))
}

/// The cover's square box; empty when the book has no cover.
fn cover_box(cover: Option<Arc<Image>>, theme: ui_components::BrowserTheme) -> gpui::Div {
    div().size(rems(COVER_SIZE_REM)).flex_none().children(cover.map(|cover| ui_components::book_cover_image("audiobook-dock-cover", cover, 1.0, false, theme)))
}

impl Render for AudiobookDock {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = ui_components::browser_theme(cx);
        let player = self.player.read(cx);
        let cover = player.book.as_ref().and_then(|book| book.cover.clone());
        let compact = ui_components::WindowWidthClass::for_window(window).is_compact();

        let position = super::shown_position(player.position);
        let (chapter_start, chapter_position, chapter_duration) = player
            .book
            .as_ref()
            .and_then(|book| book.chapters.get(player.active_chapter_index()))
            .map(|chapter| {
                let duration = chapter.end().saturating_sub(chapter.start());
                (chapter.start(), position.saturating_sub(chapter.start()).min(duration), duration)
            })
            .unwrap_or_default();
        let progress_fraction = if chapter_duration.is_zero() { 0.0 } else { (chapter_position.as_secs_f64() / chapter_duration.as_secs_f64()) as f32 };
        let title = player.book.as_ref().map(|book| book.title.clone()).unwrap_or_else(|| "Opening audiobook…".into());
        let current_chapter = player.book.as_ref().and_then(|book| book.chapters.get(player.active_chapter_index())).map(|chapter| super::chapter_display_title(&chapter.title)).unwrap_or_else(|| "No chapter".into());
        let playing = player.transport_playing();
        // While a press is scrubbing, the bar and the time readout follow the
        // drag instead of playback, to the second, so what the digits say
        // matches where the fill is. Otherwise they show whole minutes.
        let display_fraction = self.scrub.unwrap_or(progress_fraction);
        let display_position = self.scrub.map(|fraction| super::chapter_position_at_fraction(Duration::ZERO, chapter_duration, fraction)).unwrap_or(chapter_position);
        let display_book_position = if self.scrub.is_some() { chapter_start.saturating_add(display_position) } else { position };
        let book_remaining = player.book.as_ref().filter(|book| !book.duration.is_zero()).map(|book| super::format_time_left(book.duration.saturating_sub(display_book_position)));
        let sleep_label = player.sleep_label();
        let rate = player.rate;
        // The scrubber's ends in the phone player and the detail page: elapsed
        // and still to come in this chapter.
        let chapter_remaining = chapter_duration.saturating_sub(display_position);
        let (elapsed, remaining) = match self.scrub {
            Some(_) => (super::format_time(display_position), format!("−{}", super::format_time(chapter_remaining))),
            None => (super::format_duration_short(display_position), format!("−{}", super::format_duration_short(chapter_remaining))),
        };

        // The playing book's detail page holds these controls in place of its
        // actions; the page itself already shows the cover, title and
        // chapters, and the dock is hidden while it is open.
        if self.inline {
            let player_entity = self.player.clone();
            let choose_sleep = move |choice, cx: &mut App| player_entity.update(cx, |player, cx| player.choose_sleep(choice, cx));
            let menu_label = sleep_label.clone();
            let sleep = ui_components::now_playing_sleep_button("now-playing-sleep", sleep_label.clone(), theme)
                .dropdown_menu_with_anchor(gpui::Anchor::TopRight, move |menu, _, _| super::sleep_menu(menu_label.as_deref()).dropdown(menu, choose_sleep.clone()));
            let player_entity = self.player.clone();
            let set_rate = move |rate, cx: &mut App| player_entity.update(cx, |player, cx| player.set_rate(rate, cx));
            let speed = ui_components::now_playing_speed_button("now-playing-speed", super::format_playback_rate(rate), !super::same_rate(rate, 1.0), theme)
                .dropdown_menu_with_anchor(gpui::Anchor::TopLeft, move |menu, _, _| super::speed_menu(rate).dropdown(menu, set_rate.clone()));
            let transport = self.transport(playing, TransportLayout::Inline, theme, cx);
            let controls = if ui_components::TOUCH_UI {
                ui_components::now_playing_inline_controls().justify_between().child(speed).children(transport.into_iter().map(|(_, button)| button)).child(sleep)
            } else {
                let (chapters, playback): (Vec<_>, Vec<_>) = transport.into_iter().partition(|(action, _)| matches!(action, PlaybackTransportAction::Previous | PlaybackTransportAction::Next));
                ui_components::column(0.0)
                    .child(ui_components::now_playing_inline_controls().children(playback.into_iter().map(|(_, button)| button)))
                    .child(ui_components::now_playing_inline_controls().child(speed).children(chapters.into_iter().map(|(_, button)| button)).child(sleep))
            };
            return ui_components::now_playing_inline()
                .relative()
                .child(ui_components::now_playing_inline_times(elapsed, current_chapter, remaining, theme))
                .child(self.scrubbable(ui_components::now_playing_scrub_track(display_fraction, theme), cx))
                .child(controls)
                .when(self.scrub.is_some(), |controls| controls.child(dock_scrub_capture(cx)))
                .into_any_element();
        }

        // The dock along the bottom of a phone. Tapping it opens the
        // full-screen player over the window, which leaves the bar in place
        // underneath.
        let docked_bar = if compact {
            let summary = title_block(title.clone(), current_chapter.clone(), theme).id("audiobook-dock-expand").cursor_pointer().on_click(cx.listener(|dock, _, _, cx| {
                dock.mobile_expanded = true;
                cx.notify();
            }));
            let play = ui_components::playback_play_button("dock-play", playing, TransportLayout::Dock, theme).on_click(cx.listener(|dock, _, _, cx| dock.toggle_playback(cx)));
            let bar = ui_components::bottom_bar(theme)
                .id("audiobook-dock")
                .occlude()
                .relative()
                .h(rems(COVER_SIZE_REM))
                .on_scroll_wheel(cx.listener(|dock, event, _, cx| dock.handle_swipe(event, cx)))
                .child(ui_components::row(0.0).gap(rems(SPACE_SM / 16.0)).fill().pr(rems(SPACE_SM / 16.0)).child(cover_box(cover.clone(), theme)).child(summary).child(play))
                .child(ui_components::top_edge().child(ui_components::player_dock_progress_line(progress_fraction, theme)));
            if !self.mobile_expanded {
                return bar.into_any_element();
            }
            Some(bar)
        } else {
            None
        };

        if let Some(bar) = docked_bar {
            let author = player.book.as_ref().and_then(|book| book.author.clone()).map(SharedString::from);
            let active_chapter = player.active_chapter_index();
            // Square, as wide as the page allows, but never so tall that it
            // pushes the controls off a short screen.
            let viewport = window.viewport_size();
            let cover_side = px((f32::from(viewport.width) - 2.0 * ui_components::SPACE_LG).min(f32::from(viewport.height) * 0.42).max(0.0));

            let collapse = ui_components::outlined_icon_button("now-playing-collapse", "Collapse player", IconName::ChevronDown, theme).on_click(cx.listener(|dock, _, _, cx| {
                dock.collapse_mobile(cx);
            }));
            let close = ui_components::outlined_icon_button("now-playing-close", "Close player", IconName::Close, theme).on_click(cx.listener(|_, _, _, cx| cx.emit(DockAction::Close)));

            let transport = ui_components::now_playing_transport().children(self.transport(playing, TransportLayout::Player, theme, cx).into_iter().map(|(_, button)| button));

            let has_chapters = player.book.as_ref().is_some_and(|book| book.chapters.len() > 1);
            let tools = ui_components::now_playing_tools()
                .child(ui_components::now_playing_tool_button("now-playing-speed", super::format_playback_rate(rate), None, !super::same_rate(rate, 1.0), theme).on_click(cx.listener(|dock, _, _, cx| dock.toggle_speed_menu(cx))))
                .child(
                    ui_components::now_playing_tool_button("now-playing-sleep", sleep_label.clone().unwrap_or_else(|| "Sleep".into()), Some(Icon::empty().path("icons/alarm.svg")), sleep_label.is_some(), theme)
                        .on_click(cx.listener(|dock, _, _, cx| dock.toggle_sleep_menu(cx))),
                )
                .when(has_chapters, |tools| {
                    tools.child(ui_components::now_playing_tool_button("now-playing-chapters", "Chapters", Some(IconName::Menu.into()), false, theme).on_click(cx.listener(|dock, _, _, cx| dock.toggle_chapters_menu(cx))))
                });

            let body = ui_components::now_playing_body()
                .children(cover.map(|cover| ui_components::now_playing_cover(cover_side).child(ui_components::book_cover_image("now-playing-cover", cover, 1.0, false, theme))))
                .child(ui_components::now_playing_titles(title, author, theme))
                .child(ui_components::now_playing_spacer())
                .child(ui_components::now_playing_chapter(current_chapter))
                .child(self.scrubbable(ui_components::now_playing_scrub_track(display_fraction, theme), cx))
                .child(ui_components::now_playing_times(elapsed, book_remaining.map(SharedString::from), remaining, theme))
                .child(transport)
                .child(tools);
            let page = ui_components::now_playing_page(theme).child(ui_components::now_playing_top_bar(theme).child(collapse).child(close)).child(body);

            // The menus open over the player, so they sit a layer above it.
            let sleep_sheet = self.sleep_menu_open.then(|| {
                gpui::deferred(page_sheet(super::sleep_menu(sleep_label.as_deref()).sheet(|choice| cx.listener(move |dock, _, _, cx| dock.choose_sleep(choice, cx)), cx.listener(|dock, _, _, cx| dock.toggle_sleep_menu(cx)), theme), window))
                    .with_priority(2)
            });
            let speed_sheet = self.speed_menu_open.then(|| {
                gpui::deferred(page_sheet(super::speed_menu(rate).sheet(|rate| cx.listener(move |dock, _, _, cx| dock.set_rate(rate, cx)), cx.listener(|dock, _, _, cx| dock.toggle_speed_menu(cx)), theme), window)).with_priority(2)
            });
            let chapters_sheet = player.book.as_ref().filter(|_| self.chapters_menu_open).map(|book| {
                let menu = super::chapters_menu(&book.chapters, active_chapter);
                gpui::deferred(page_sheet(menu.sheet(|index| cx.listener(move |dock, _, _, cx| dock.choose_chapter(index, cx)), cx.listener(|dock, _, _, cx| dock.toggle_chapters_menu(cx)), theme), window)).with_priority(2)
            });

            return bar
                .child(gpui::deferred(full_window(page, window)).with_priority(1))
                .children(sleep_sheet)
                .children(speed_sheet)
                .children(chapters_sheet)
                .when(self.scrub.is_some(), |bar| bar.child(dock_scrub_capture(cx)))
                .into_any_element();
        }

        let chapter_time = match self.scrub {
            Some(_) => format!("{} / {}", super::format_time(display_position), super::format_time(chapter_duration)),
            None => format!("{} / {}", super::format_duration_short(display_position), super::format_duration_short(chapter_duration)),
        };
        // The line is drawn thin, inside a taller hit area.
        let scrub_track = self.scrubbable(ui_components::player_dock_track(display_fraction, theme), cx);

        let transport = self.transport(playing, TransportLayout::Dock, theme, cx).into_iter().map(|(_, button)| button);
        let close = ui_components::outlined_icon_button("dock-close", "Close player", IconName::Close, theme).on_click(cx.listener(|_, _, _, cx| cx.emit(DockAction::Close)));
        // The alarm glyph, or the time left once a timer is running.
        let sleep = match sleep_label.clone() {
            Some(label) => ui_components::player_dock_text_button("dock-sleep-timer", label, theme).tooltip("Sleep timer"),
            None => ui_components::outlined_icon_button("dock-sleep-timer", "Sleep timer", Icon::empty().path("icons/alarm.svg"), theme),
        };
        let sleep = ui_components::selectable_button(sleep, self.sleep_menu_open || sleep_label.is_some(), theme);
        let speed = ui_components::player_dock_text_button("dock-playback-rate", super::format_playback_rate(rate), theme).tooltip("Playback speed");
        let speed = ui_components::selectable_button(speed, self.speed_menu_open, theme);

        let book = title_block(title, current_chapter, theme)
            .child(ui_components::caption_line(chapter_time, theme).text_size(rems(PLAYER_DOCK_CAPTION_SIZE_REM)).line_height(relative(1.25)))
            .id("audiobook-dock-expand")
            .cursor_pointer()
            .on_click(cx.listener(|_, _, _, cx| cx.emit(DockAction::Expand)));
        let transport = ui_components::player_dock_transport(None, transport, theme);

        let player = self.player.clone();
        let choose_sleep = move |choice, cx: &mut App| player.update(cx, |player, cx| player.choose_sleep(choice, cx));
        let sleep = sleep.dropdown_menu_with_anchor(gpui::Anchor::BottomLeft, move |menu, _, _| super::sleep_menu(sleep_label.as_deref()).dropdown(menu, choose_sleep.clone()));
        let player = self.player.clone();
        let set_rate = move |rate, cx: &mut App| player.update(cx, |player, cx| player.set_rate(rate, cx));
        let speed = speed.dropdown_menu_with_anchor(gpui::Anchor::BottomRight, move |menu, _, _| super::speed_menu(rate).dropdown(menu, set_rate.clone()));
        let tools = ui_components::column(0.0)
            .gap(rems(SPACE_XXS / 16.0))
            .items_end()
            .children(book_remaining.map(|left| ui_components::caption_line(left, theme).text_size(rems(PLAYER_DOCK_CAPTION_SIZE_REM)).line_height(relative(1.25))))
            .child(ui_components::player_dock_tools().child(sleep).child(speed).child(close));

        ui_components::player_dock(div().flex().items_center().gap(rems(SPACE_SM / 16.0)).h_full().min_w_0().flex_1().child(cover_box(cover, theme)).child(book), transport, tools, theme)
            .id("audiobook-dock")
            .child(scrub_track)
            .when(self.scrub.is_some(), |dock| dock.child(dock_scrub_capture(cx)))
            .into_any_element()
    }
}
