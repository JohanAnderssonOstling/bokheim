//! The player dock along the bottom of a window, shared by audiobook playback
//! and reader narration.
//!
//! Wide docks keep the transport centred between what is playing on the left
//! and the remaining tools on the right. Compact narration docks distribute
//! all controls evenly across one row.

use gpui::prelude::*;
use gpui::{App, DispatchPhase, Div, HitboxBehavior, SharedString, TouchEvent, TouchPhase, Window, canvas, px, rems};

use crate::{BrowserTheme, FlexItem, SPACE_MD, SPACE_SM, SPACE_XXS};

/// Dock and cover size in rems, scaling with the interface font.
pub const PLAYER_DOCK_HEIGHT_REM: f32 = 5.0;
pub const PLAYER_DOCK_COMPACT_ROW_HEIGHT_REM: f32 = 4.0;
pub const PLAYER_DOCK_TEXT_SIZE_REM: f32 = 1.0;
pub const PLAYER_DOCK_CAPTION_SIZE_REM: f32 = 0.75;
/// The progress line is drawn thin but has to be easy to hit, so its hit area
/// is taller than the line inside it.
pub const PLAYER_DOCK_TRACK_HIT_HEIGHT_REM: f32 = 1.0;
const PROGRESS_LINE_HEIGHT: f32 = 3.0;

/// Attach inside a positioned scrub track. Only an unobscured track can begin
/// a touch scrub; its owner tracks the contact id through release/cancellation,
/// including when the finger moves outside the track.
pub fn playback_scrub_touch_listener(listener: impl Fn(&TouchEvent, &mut Window, &mut App) + 'static) -> impl IntoElement {
    canvas(
        |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
        move |_, hitbox, window, _| {
            window.on_touch(move |event, phase, window, cx| {
                if phase == DispatchPhase::Capture && (event.phase != TouchPhase::Started || hitbox.is_under_pointer(window)) {
                    listener(event, window, cx);
                }
            });
        },
    )
    .absolute()
    .size_full()
}

/// Total dock height in pixels, including its fixed top border.
pub fn player_dock_height_px(window: &Window) -> f32 {
    let height = if crate::uses_mobile_reader_controls(window) { PLAYER_DOCK_COMPACT_ROW_HEIGHT_REM } else { PLAYER_DOCK_HEIGHT_REM };
    f32::from(rems(height).to_pixels(window.rem_size())) + 1.0
}

/// Information or voice selection, playback controls, and secondary tools.
/// Compact docks use one row; overflow scrolls instead of shrinking targets.
pub fn player_dock(leading: impl IntoElement, transport: impl IntoElement, trailing: impl IntoElement, compact: bool, theme: BrowserTheme) -> Div {
    let content = if compact {
        crate::row(0.0)
            .id("player-dock-controls-row")
            .w_full()
            .h(rems(PLAYER_DOCK_COMPACT_ROW_HEIGHT_REM))
            .gap(rems(SPACE_SM / 16.0))
            .px(rems(SPACE_SM / 16.0))
            .overflow_x_scroll()
            .child(crate::row(0.0).flex_none().h_full().child(leading))
            .child(crate::row(0.0).flex_grow(1.0).flex_shrink_0().h_full().justify_center().child(transport))
            .child(crate::row(0.0).flex_none().h_full().justify_end().child(trailing))
            .into_any_element()
    } else {
        crate::row(0.0)
            .gap(rems(SPACE_MD / 16.0))
            .fill()
            .h(rems(PLAYER_DOCK_HEIGHT_REM))
            .px(rems(SPACE_MD / 16.0))
            .child(crate::row(0.0).gap(rems(SPACE_SM / 16.0)).fill().h_full().child(leading))
            .child(transport)
            .child(crate::row(0.0).gap(rems(SPACE_SM / 16.0)).fill().h_full().justify_end().child(trailing))
            .into_any_element()
    };
    crate::bottom_bar(theme).occlude().relative().child(content)
}

/// Equal spacing between every compact control and at both ends of the row.
pub fn player_dock_evenly(controls: impl IntoIterator<Item = impl IntoElement>, theme: BrowserTheme) -> Div {
    crate::bottom_bar(theme).occlude().relative().child(crate::row(0.0).w_full().h(rems(PLAYER_DOCK_COMPACT_ROW_HEIGHT_REM)).justify_evenly().children(controls))
}

/// The transport buttons, with an optional caption over them.
pub fn player_dock_transport(caption: Option<SharedString>, buttons: impl IntoIterator<Item = impl IntoElement>, theme: BrowserTheme) -> Div {
    crate::column(0.0)
        .gap(rems(SPACE_XXS / 16.0))
        .flex_none()
        .items_center()
        .children(caption.map(|caption| crate::caption_line(caption, theme).text_size(rems(PLAYER_DOCK_CAPTION_SIZE_REM)).line_height_auto()))
        .child(crate::row(0.0).gap(rems(SPACE_MD / 16.0)).children(buttons))
}

/// A row of dock tools.
pub fn player_dock_tools() -> Div {
    crate::row(0.0).gap(rems(SPACE_SM / 16.0))
}

/// The chapter's progress as a thin line, for the top edge of a dock.
pub fn player_dock_progress_line(fraction: f32, theme: BrowserTheme) -> Div {
    crate::progress_track(theme).h(px(PROGRESS_LINE_HEIGHT)).child(crate::progress_fill(fraction, theme))
}

/// The progress line inside its taller hit area along the dock's top edge.
/// The caller makes it scrubbable.
pub fn player_dock_track(fraction: f32, theme: BrowserTheme) -> Div {
    crate::top_edge().h(rems(PLAYER_DOCK_TRACK_HIT_HEIGHT_REM)).child(player_dock_progress_line(fraction, theme))
}
