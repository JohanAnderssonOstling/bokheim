//! The full-screen audiobook player on phones. Play is the one dominant
//! control; the seconds skips stand beside it, chapter skip is small and
//! muted, and speed, sleep and chapters sit on a quiet tier below.
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

/// The detail page's secondary controls — speed, chapter skip, seconds skip,
/// sleep — are one size: this glyph in a hit area of `INLINE_HIT_REM`.
const INLINE_GLYPH_REM: f32 = 1.625;
const INLINE_HIT_REM: f32 = 3.0;
/// Preferred play/pause circle size across desktop audiobook and narration layouts.
const DESKTOP_PLAY_SIZE_REM: f32 = 4.0;
const DESKTOP_DOCK_PLAY_SIZE_REM: f32 = 3.0;
const DESKTOP_DOCK_PLAY_ICON_SIZE_REM: f32 = 1.5;
const DESKTOP_PLAY_MIN_SIZE_PX: f32 = 48.0;
const DESKTOP_PLAY_MAX_SIZE_PX: f32 = 80.0;
/// The row is laid out on the glyphs, not the buttons: each hit area overhangs
/// its glyph by this much on either side, taken back with a negative margin,
/// so the spacing between neighbours is the spacing between what is seen and
/// the outermost glyphs sit on the column's edges.
const INLINE_OVERHANG_REM: f32 = (INLINE_HIT_REM - INLINE_GLYPH_REM) / 2.0;
/// A chevron's ink is the middle third of its box; its box is trimmed to the
/// ink by this much more on either side, or the gaps beside it look wider.
const CHEVRON_TRIM_REM: f32 = INLINE_GLYPH_REM * 0.32;

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
    /// A book's detail page, laid out on the glyphs.
    Inline,
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

/// Play, or pause while playing: round, and the one control filled with the
/// accent wherever a transport is.
pub fn playback_play_button(id: impl Into<ElementId>, playing: bool, layout: TransportLayout, theme: BrowserTheme) -> Button {
    crate::selectable_button(transport_button(id, if playing { "Pause" } else { "Play" }, play_pause_icon(playing), TransportControl::Play, layout, theme), true, theme)
}

impl TransportControl {
    /// Button and icon size, in rem.
    fn size(self, layout: TransportLayout) -> (f32, f32) {
        let (size, icon_size) = match (self, layout) {
            (TransportControl::Chapter, TransportLayout::Player) => (2.75, 1.375),
            (TransportControl::Skip, TransportLayout::Player) => (3.5, 2.0),
            (TransportControl::Chapter | TransportControl::Skip, TransportLayout::Inline) => (INLINE_HIT_REM, INLINE_GLYPH_REM),
            (TransportControl::Play, TransportLayout::Player | TransportLayout::Inline) => (4.5, 1.875),
            (TransportControl::Chapter | TransportControl::Skip, TransportLayout::Dock) => (2.5, 1.25),
            (TransportControl::Play, TransportLayout::Dock) => (4.0, 2.0),
        };
        match (self, layout, touch_ui()) {
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
    inline_secondary_button(id, "Playback speed", set, theme).child(div().text_size(rems(TEXT_LG)).child(rate.into()))
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

/// A secondary control in the detail page's row: a hit area overhanging its
/// glyph, laid out on the glyph (see `INLINE_OVERHANG_REM`).
fn inline_secondary_button(id: impl Into<ElementId>, tooltip: impl Into<SharedString>, set: bool, theme: BrowserTheme) -> Button {
    crate::base_button(id).ghost().h(rems(INLINE_HIT_REM)).min_w(rems(INLINE_HIT_REM)).px(rems(INLINE_OVERHANG_REM)).mx(rems(-INLINE_OVERHANG_REM)).text_color(if set { theme.text_accent } else { theme.text }).tooltip(tooltip)
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
    crate::row(SPACE_SM).flex_none().justify_center().mt(px(SPACE_LG))
}

/// One transport control; play is made accent by `playback_play_button`, and
/// is round. In the phone player chapter skip is muted, the lesser control. On
/// the detail page every control but play is one size and colour, laid out on
/// its glyph (see `INLINE_OVERHANG_REM`).
fn transport_button(id: impl Into<ElementId>, label: impl Into<SharedString>, icon: impl Into<Icon>, control: TransportControl, layout: TransportLayout, theme: BrowserTheme) -> Button {
    let (size, icon_size) = control.size(layout);
    let color = if matches!(control, TransportControl::Chapter) && layout == TransportLayout::Player { theme.text_muted } else { theme.text };
    // Set both dimensions and prevent flex shrinking to preserve a square icon.
    let icon = icon.into().size(rems(icon_size)).flex_none();
    let button = crate::base_button(id).ghost().text_color(color).bg(theme.page_bg).p_0().flex_none().size(rems(size)).tooltip(label).child(icon);
    match (control, layout) {
        (TransportControl::Play, _) => button.rounded_full().when(!touch_ui(), |button| button.min_w(px(DESKTOP_PLAY_MIN_SIZE_PX)).min_h(px(DESKTOP_PLAY_MIN_SIZE_PX)).max_w(px(DESKTOP_PLAY_MAX_SIZE_PX)).max_h(px(DESKTOP_PLAY_MAX_SIZE_PX))),
        (TransportControl::Chapter, TransportLayout::Inline) => button.mx(rems(-(INLINE_OVERHANG_REM + CHEVRON_TRIM_REM))),
        (TransportControl::Skip, TransportLayout::Inline) => button.mx(rems(-INLINE_OVERHANG_REM)),
        _ => button,
    }
}

/// Spread across the bottom of the phone's player.
pub fn now_playing_tools() -> Div {
    crate::spread_row(SPACE_SM).flex_none().mt(px(SPACE_LG))
}

/// A quiet text control on the bottom tier: muted at its default, accent once
/// it holds a choice of the reader's (a speed other than 1×, a running timer).
pub fn now_playing_tool_button(id: impl Into<ElementId>, label: impl Into<SharedString>, icon: Option<Icon>, set: bool, theme: BrowserTheme) -> Button {
    let button = crate::base_button(id).ghost().label(label).h(rems(2.5)).px(px(SPACE_MD)).text_color(if set { theme.text_accent } else { theme.text_muted });
    match icon {
        Some(icon) => button.icon(icon),
        None => button,
    }
}
