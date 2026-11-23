//! Account section visuals: who is signed in, the storage meter with what each
//! library holds of it, and the table of this device's libraries.
//!
//! Only the status dots and the upload reservation carry a semantic colour, and
//! only while something is actually waiting; everything else is quiet.

use gpui::prelude::*;
use ui_text::text;
use gpui::{Div, Hsla, SharedString, div, px, relative};

use crate::{BrowserTheme, SPACE_MD, SPACE_SM, SPACE_XS, SPACE_XXS, TEXT_SM, TEXT_XS};

/// Height of the account quota meter. A line under the figures rather than a
/// bar of its own: the breakdown below it carries the detail.
const METER_HEIGHT: f32 = 6.0;
/// The account button carries the initial in the section header.
const ACCOUNT_AVATAR_SIZE_REM: f32 = 2.5;
/// The swatch tying a breakdown line to its segment of the meter.
const SEGMENT_SWATCH_SIZE: f32 = 8.0;
/// A breakdown line's share and size columns, wide enough for "100%" and
/// "999.9 MB" so the figures line up down the list.
const BREAKDOWN_SHARE_WIDTH_REM: f32 = 3.0;
const BREAKDOWN_BYTES_WIDTH_REM: f32 = 4.5;
/// The height a breakdown line keeps, so the list reads at one rhythm.
const BREAKDOWN_ROW_HEIGHT_REM: f32 = 1.5;
/// Keeps the empty line the height a library row's name has.
const LIBRARY_NAME_HEIGHT_REM: f32 = 2.5;

/// The account initial opens actions from the section header.
pub fn account_avatar_button(initial: impl Into<SharedString>) -> crate::Button {
    crate::base_button("account-menu")
        .primary()
        .label(initial)
        .aria_label(text!(account::account))
        .tooltip(text!(account::account))
        .p_0()
        .size(gpui::rems(ACCOUNT_AVATAR_SIZE_REM.max(crate::settings_control_size_rem())))
        .text_size(gpui::rems(TEXT_SM))
}

/// The email stays beside the avatar, truncating to fit narrow headers.
pub fn account_header(email: impl Into<SharedString>, trigger: impl IntoElement, theme: BrowserTheme) -> Div {
    crate::browser::browser_settings_header(text!(account::account), theme)
        .min_w_0()
        .flex_nowrap()
        .child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().text_right().text_size(gpui::rems(TEXT_XS)).text_color(theme.text).child(email.into()))
        .child(div().flex_none().child(trigger))
}

/// Storage use followed by the account quota.
pub fn storage_summary(used: impl Into<SharedString>, of_quota: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    div()
        .w_full()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_baseline()
        .gap(px(SPACE_XS))
        .text_size(gpui::rems(TEXT_SM))
        .child(div().text_color(theme.text).child(used.into()))
        .child(div().text_color(theme.text_muted).child(of_quota.into()))
}

/// The quota meter. Each tuple is `(fraction, color)`; fractions are clamped to
/// the track.
pub fn storage_meter(segments: impl IntoIterator<Item = (f32, Hsla)>, theme: BrowserTheme) -> Div {
    let mut track = div().w_full().h(px(METER_HEIGHT)).flex().flex_row().overflow_hidden().border_1().border_color(theme.rule).bg(theme.page_bg);
    for (fraction, color) in segments {
        track = track.child(div().h_full().w(relative(fraction.clamp(0.0, 1.0))).bg(color));
    }
    track
}

/// What each library holds of the quota, one line apiece under the meter.
///
/// A legend wrapped along one line made the figures impossible to compare;
/// down a column, with the share and size in fixed columns, the largest
/// library is the one that reads largest.
pub fn storage_breakdown(rows: impl IntoIterator<Item = Div>) -> Div {
    div().w_full().flex().flex_col().gap(px(SPACE_XXS)).children(rows)
}

/// One line of [`storage_breakdown`]: the segment's colour, its name, its share
/// of the quota, and its size.
pub fn storage_breakdown_row(color: Hsla, name: impl Into<SharedString>, share: impl Into<SharedString>, bytes: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    div()
        .w_full()
        .min_h(gpui::rems(BREAKDOWN_ROW_HEIGHT_REM))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(SPACE_SM))
        .text_size(gpui::rems(TEXT_XS))
        // E-ink draws no meter, so a swatch would key nothing.
        .when(!cfg!(feature = "kobo"), |row| row.child(div().size(px(SEGMENT_SWATCH_SIZE)).flex_none().bg(color)))
        .child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().text_color(theme.text).child(name.into()))
        .child(div().w(gpui::rems(BREAKDOWN_SHARE_WIDTH_REM)).flex_none().flex().justify_end().text_color(theme.text_muted).child(share.into()))
        .child(div().w(gpui::rems(BREAKDOWN_BYTES_WIDTH_REM)).flex_none().flex().justify_end().text_color(theme.text_muted).child(bytes.into()))
}

/// Categorical quota colours from the active palette. The meter and its legend
/// use the same colour for each library, with brighter colours in dark mode.
/// The sequence repeats once all five colours have been used.
pub fn usage_segment_color(index: usize, theme: BrowserTheme) -> Hsla {
    theme.usage_segments[index % theme.usage_segments.len()]
}

/// The list with no libraries in it. A line of its own rather than an empty
/// table, which would read as something that failed to load.
pub fn library_usage_empty(text: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    div().w_full().py(px(SPACE_SM)).min_h(gpui::rems(LIBRARY_NAME_HEIGHT_REM)).flex().items_center().border_t_1().border_color(theme.rule).text_size(gpui::rems(TEXT_SM)).text_color(theme.text_muted).child(text.into())
}

/// The signed-out form: a title, the inputs, the actions, and whatever the last
/// attempt had to say, in one measure.
///
/// The inputs inside it carry their own placeholders and are never given a
/// label as well — a field named twice reads as two fields.
pub fn access_form() -> Div {
    div().w_full().flex().flex_col().gap(px(SPACE_SM))
}

/// The quiet line under an input that names a constraint the user cannot guess,
/// such as a minimum password length. Not a label: it says something the
/// placeholder does not.
pub fn form_hint(text: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    let text: SharedString = text.into();
    crate::supporting_text(text, theme)
}

/// Vertical rhythm for the storage block: summary, meter, breakdown.
pub fn storage_block() -> Div {
    div().w_full().flex().flex_col().gap(px(SPACE_SM)).pt(px(SPACE_XS))
}
