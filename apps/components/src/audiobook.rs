//! The full-screen audiobook player on phones. Play is the one dominant
//! control; secondary controls share one square size, and speed, sleep and
//! chapters sit on a quiet tier below.
use gpui::prelude::*;
use gpui::{Div, ElementId, Pixels, SharedString, div, px, rems};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::{Icon, IconName};

use crate::{BrowserTheme, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XS, TEXT_LG, TEXT_SM, TEXT_XS, touch_ui};

/// The scrub line is drawn a little heavier than the dock's, with a round
/// thumb — like play, the player's one other circle — inside a hit area a
/// thumb can find.
const SCRUB_HIT_HEIGHT: f32 = 24.0;
const SCRUB_LINE_HEIGHT: f32 = 4.0;
const SCRUB_THUMB_SIZE: f32 = 14.0;

/// Every secondary playback control shares this square hit area.
pub const PLAYBACK_SECONDARY_SIZE_REM: f32 = 2.5;
const INLINE_GLYPH_REM: f32 = 1.625;
/// Preferred play/pause circle size across desktop audiobook and narration layouts.
const DESKTOP_PLAY_SIZE_REM: f32 = 4.0;
const DESKTOP_DOCK_PLAY_SIZE_REM: f32 = 3.0;
const DESKTOP_DOCK_PLAY_ICON_SIZE_REM: f32 = 1.5;
const DESKTOP_PLAY_MIN_SIZE_PX: f32 = 48.0;
const DESKTOP_PLAY_MAX_SIZE_PX: f32 = 80.0;
/// The three transport control roles.
#[derive(Clone, Copy)]
pub enum TransportControl {
    Chapter,
    Skip,
    Play,
}

/// Transport actions shared by audiobook playback and reader narration.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PlaybackTransportAction {
    Previous,
    Back,
    PlayPause,
    Forward,
    Next,
}

#[derive(Clone, Copy)]
pub enum PlaybackSkipKind {
    Seconds,
    Elements,
}

/// Where a set of transport controls sits, which sets their sizes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TransportLayout {
    /// The phone's full-screen player.
    Player,
    /// A book's detail page.
    Inline,
    /// The compact audiobook bar on phones.
    MiniPlayer,
    /// The dock along the bottom of a window: audiobooks and narration.
    Dock,
}

/// The transport: chapter skips (when `navigation_labels` names them), the
/// back and forward skips, and play.
pub fn playback_transport_buttons(id: &str, playing: bool, layout: TransportLayout, navigation_labels: Option<[&str; 2]>, skips: PlaybackSkipKind, theme: BrowserTheme) -> Vec<(PlaybackTransportAction, Button)> {
    use PlaybackTransportAction::*;
    let button = |suffix: &str, label: &str, icon: Icon, control| transport_button(SharedString::from(format!("{id}-{suffix}")), SharedString::from(label.to_owned()), icon, control, layout, theme);
    let (back_label, forward_label, back_icon, forward_icon) = match skips {
        PlaybackSkipKind::Seconds => ("Back 15 seconds", "Forward 30 seconds", skip_back_icon(), skip_forward_icon()),
        PlaybackSkipKind::Elements => ("Previous passage", "Next passage", Icon::empty().path("icons/undo-2.svg"), Icon::empty().path("icons/redo-2.svg")),
    };
    let mut buttons = Vec::with_capacity(5);
    if let Some([previous, _]) = navigation_labels {
        buttons.push((Previous, button("previous-chapter", previous, IconName::ChevronLeft.into(), TransportControl::Chapter)));
    }
    buttons.push((Back, button("seek-back", back_label, back_icon, TransportControl::Skip)));
    buttons.push((PlayPause, playback_play_button(SharedString::from(format!("{id}-play")), playing, layout, theme)));
    buttons.push((Forward, button("seek-forward", forward_label, forward_icon, TransportControl::Skip)));
    if let Some([_, next]) = navigation_labels {
        buttons.push((Next, button("next-chapter", next, IconName::ChevronRight.into(), TransportControl::Chapter)));
    }
    buttons
}

/// Play, or pause while playing: a plain icon in the mini-player, and an
/// accent-filled circle in the other transports.
pub fn playback_play_button(id: impl Into<ElementId>, playing: bool, layout: TransportLayout, theme: BrowserTheme) -> Button {
    let button = transport_button(id, if playing { "Pause" } else { "Play" }, play_pause_icon(playing), TransportControl::Play, layout, theme);
    if layout == TransportLayout::MiniPlayer { button } else { crate::selectable_button(button, true, theme) }
}

impl TransportControl {
    /// Button and icon size, in rem.
    fn size(self, layout: TransportLayout) -> (f32, f32) {
        let (size, icon_size) = match (self, layout) {
            (TransportControl::Chapter, TransportLayout::Player) => (PLAYBACK_SECONDARY_SIZE_REM, 1.375),
            (TransportControl::Skip, TransportLayout::Player) => (PLAYBACK_SECONDARY_SIZE_REM, 2.0),
            (TransportControl::Chapter | TransportControl::Skip, TransportLayout::Inline) => (PLAYBACK_SECONDARY_SIZE_REM, INLINE_GLYPH_REM),
            (TransportControl::Play, TransportLayout::Player | TransportLayout::Inline) => (4.5, 1.875),
            (TransportControl::Chapter | TransportControl::Skip, TransportLayout::Dock | TransportLayout::MiniPlayer) => (PLAYBACK_SECONDARY_SIZE_REM, 1.25),
            (TransportControl::Play, TransportLayout::MiniPlayer) => (3.0, 1.5),
            (TransportControl::Play, TransportLayout::Dock) => (4.0, 2.0),
        };
        match (self, layout, touch_ui()) {
            (Self::Play, TransportLayout::MiniPlayer, _) => (size, icon_size),
            (Self::Play, TransportLayout::Dock, false) => (DESKTOP_DOCK_PLAY_SIZE_REM, DESKTOP_DOCK_PLAY_ICON_SIZE_REM),
            (Self::Play, _, false) => (DESKTOP_PLAY_SIZE_REM, icon_size),
            _ => (size, icon_size),
        }
    }
}

/// The player's controls inside a book's detail page, under its headline:
/// the times and chapter over the scrubber, then one row of controls.
pub fn now_playing_inline() -> Div {
    div().w_full().min_w_0().flex().flex_col().mt(px(SPACE_SM))
}

/// The skip icons, with their seconds written inside the arrow.
pub fn skip_back_icon() -> Icon {
    Icon::empty().path("icons/skip-back-15.svg")
}

pub fn skip_forward_icon() -> Icon {
    Icon::empty().path("icons/skip-forward-30.svg")
}

/// Play, or pause while playing, as a solid glyph for the filled play button.
pub fn play_pause_icon(playing: bool) -> Icon {
    Icon::empty().path(if playing { "icons/pause-fill.svg" } else { "icons/play-fill.svg" })
}

/// Over the detail page's scrubber: elapsed at the start, the chapter in the
/// middle, the chapter's remaining time at the end.
pub fn now_playing_inline_times(elapsed: impl Into<SharedString>, chapter: impl Into<SharedString>, remaining: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    crate::row(SPACE_SM).flex_none().child(crate::caption_line(elapsed, theme).flex_none()).child(crate::single_line(chapter).flex_1().text_center().text_size(rems(TEXT_XS))).child(crate::caption_line(remaining, theme).flex_none())
}

/// A detail-page control row with equal space between controls and at its edges.
pub fn now_playing_inline_controls() -> Div {
    div().w_full().min_w_0().flex().items_center().justify_evenly().mt(rems(SPACE_SM / 16.0))
}

/// Speed beside the detail page's transport: the rate set as large as the
/// glyphs beside it, in the text colour until it holds a choice of the
/// reader's (a speed other than 1×), then accent.
pub fn now_playing_speed_button(id: impl Into<ElementId>, rate: impl Into<SharedString>, set: bool, theme: BrowserTheme) -> Button {
    inline_secondary_button(id, "Playback speed", set, theme).child(div().text_size(rems(TEXT_SM)).child(rate.into()))
}

/// Sleep beside the detail page's transport: the alarm glyph, or the time
/// left in accent once a timer is running.
pub fn now_playing_sleep_button(id: impl Into<ElementId>, running: Option<SharedString>, theme: BrowserTheme) -> Button {
    let button = inline_secondary_button(id, "Sleep timer", running.is_some(), theme);
    match running {
        Some(left) => button.child(div().text_size(rems(TEXT_SM)).whitespace_nowrap().child(left)),
        None => button.child(Icon::empty().path("icons/alarm.svg").size(rems(INLINE_GLYPH_REM))),
    }
}

/// Give icon and value controls the same fixed square, without extra padding.
pub fn playback_secondary_button(button: Button) -> Button {
    button.p_0().flex_none().size(rems(PLAYBACK_SECONDARY_SIZE_REM))
}

/// A secondary control in the detail page's evenly spaced row.
fn inline_secondary_button(id: impl Into<ElementId>, tooltip: impl Into<SharedString>, set: bool, theme: BrowserTheme) -> Button {
    playback_secondary_button(crate::base_button(id).ghost()).text_color(if set { theme.text_accent } else { theme.text }).tooltip(tooltip)
}

/// The page itself, covering the window and everything on it.
pub fn now_playing_page(theme: BrowserTheme) -> Div {
    div().size_full().flex().flex_col().occlude().bg(theme.page_bg).text_color(theme.text)
}

/// The page's topbar: collapse on the leading side, close on the trailing one.
pub fn now_playing_top_bar(theme: BrowserTheme) -> Div {
    crate::topbar(theme).justify_between().px(px(SPACE_SM))
}

/// Everything under the top bar: the cover and titles at the top, the
/// controls gathered at the bottom where a thumb reaches them.
pub fn now_playing_body() -> Div {
    div().flex_1().min_h_0().flex().flex_col().px(px(SPACE_LG)).pb(px(SPACE_MD))
}

/// The square the cover art fills, centred.
pub fn now_playing_cover(side: Pixels) -> Div {
    div().flex_none().self_center().mt(px(SPACE_SM)).size(side)
}

pub fn now_playing_titles(title: impl Into<SharedString>, author: Option<SharedString>, theme: BrowserTheme) -> Div {
    div()
        .flex_none()
        .mt(px(SPACE_LG))
        .flex()
        .flex_col()
        .items_center()
        .gap(px(SPACE_XS))
        .child(crate::single_line(title).max_w_full().font_family(theme.display_font_family()).text_size(rems(TEXT_LG)))
        .children(author.map(|author| crate::muted_line(author, theme).max_w_full()))
}

/// Pushes the controls down to the bottom of the body.
pub fn now_playing_spacer() -> Div {
    div().flex_1().min_h(px(SPACE_MD))
}

pub fn now_playing_chapter(chapter: impl Into<SharedString>) -> Div {
    crate::single_line(chapter).flex_none().text_center().text_size(rems(TEXT_XS)).mb(px(SPACE_XS))
}

/// The chapter's progress as a line with a square thumb where playback is.
pub fn now_playing_scrub_track(fraction: f32, theme: BrowserTheme) -> Div {
    let fraction = fraction.clamp(0.0, 1.0);
    div()
        .relative()
        .flex_none()
        .w_full()
        .h(px(SCRUB_HIT_HEIGHT))
        .flex()
        .items_center()
        .cursor_pointer()
        .child(crate::progress_track(theme).h(px(SCRUB_LINE_HEIGHT)).child(crate::progress_fill(fraction, theme)))
        .child(div().absolute().top(px((SCRUB_HIT_HEIGHT - SCRUB_THUMB_SIZE) / 2.0)).left(gpui::relative(fraction)).ml(px(-SCRUB_THUMB_SIZE / 2.0)).size(px(SCRUB_THUMB_SIZE)).rounded_full().bg(theme.accent))
}

/// Elapsed at the start, the book's time left in the middle, the chapter's
/// remaining time at the end.
pub fn now_playing_times(elapsed: impl Into<SharedString>, book_left: Option<SharedString>, remaining: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    crate::spread_row(SPACE_SM).flex_none().child(crate::caption_line(elapsed, theme).flex_none()).children(book_left.map(|left| crate::caption_line(left, theme))).child(crate::caption_line(remaining, theme).flex_none())
}

/// The phone's player, set apart below the times.
pub fn now_playing_transport() -> Div {
    crate::row(0.0).w_full().flex_none().justify_evenly().mt(px(SPACE_LG))
}

/// One transport control; `playback_play_button` accents play outside the
/// mini-player. In the phone player chapter skip is muted. Every secondary
/// control has the same square hit area.
fn transport_button(id: impl Into<ElementId>, label: impl Into<SharedString>, icon: impl Into<Icon>, control: TransportControl, layout: TransportLayout, theme: BrowserTheme) -> Button {
    let (size, icon_size) = control.size(layout);
    let color = match (control, layout) {
        (TransportControl::Play, TransportLayout::MiniPlayer) => theme.accent,
        (TransportControl::Chapter, TransportLayout::Player) => theme.text_muted,
        _ => theme.text,
    };
    // Set both dimensions and prevent flex shrinking to preserve a square icon.
    let icon = icon.into().size(rems(icon_size)).flex_none();
    let button = crate::base_button(id).ghost().text_color(color).bg(theme.page_bg).p_0().flex_none().size(rems(size)).tooltip(label).child(icon);
    match (control, layout) {
        (TransportControl::Play, TransportLayout::MiniPlayer) => button,
        (TransportControl::Play, _) => button.rounded_full().when(!touch_ui(), |button| button.min_w(px(DESKTOP_PLAY_MIN_SIZE_PX)).min_h(px(DESKTOP_PLAY_MIN_SIZE_PX)).max_w(px(DESKTOP_PLAY_MAX_SIZE_PX)).max_h(px(DESKTOP_PLAY_MAX_SIZE_PX))),
        _ => button,
    }
}

/// Spread across the bottom of the phone's player.
pub fn now_playing_tools() -> Div {
    crate::row(0.0).w_full().flex_none().justify_evenly().mt(px(SPACE_LG))
}

/// A quiet text control on the bottom tier: muted at its default, accent once
/// it holds a choice of the reader's (a speed other than 1×, a running timer).
pub fn now_playing_tool_button(id: impl Into<ElementId>, label: impl Into<SharedString>, icon: Option<Icon>, set: bool, theme: BrowserTheme) -> Button {
    let label = label.into();
    let button = playback_secondary_button(crate::base_button(id).ghost()).text_color(if set { theme.text_accent } else { theme.text_muted }).tooltip(label.clone());
    match icon {
        Some(icon) if !set => button.child(icon.size(rems(1.0))),
        _ => button.child(div().text_size(rems(TEXT_SM)).whitespace_nowrap().child(label)),
    }
}
