//! Account section visuals: who is signed in, the storage meter with what each
//! library holds of it, and the table of this device's libraries.
//!
//! Only the status dots and the upload reservation carry a semantic colour, and
//! only while something is actually waiting; everything else is quiet.

use gpui::prelude::*;
use gpui::{AnyElement, Div, FontWeight, Hsla, SharedString, div, px, relative};

use crate::{BrowserTheme, SPACE_MD, SPACE_SM, SPACE_XS, SPACE_XXS, TEXT_SM, TEXT_XS};

/// Height of the account quota meter. A line under the figures rather than a
/// bar of its own: the breakdown below it carries the detail.
const METER_HEIGHT: f32 = 6.0;
/// The initial beside the address.
const ACCOUNT_AVATAR_SIZE_REM: f32 = 2.5;
/// The swatch tying a breakdown line to its segment of the meter.
const SEGMENT_SWATCH_SIZE: f32 = 8.0;
/// A breakdown line's share and size columns, wide enough for "100%" and
/// "999.9 MB" so the figures line up down the list.
const BREAKDOWN_SHARE_WIDTH_REM: f32 = 3.0;
const BREAKDOWN_BYTES_WIDTH_REM: f32 = 4.5;
/// The height a breakdown line keeps, so the list reads at one rhythm.
const BREAKDOWN_ROW_HEIGHT_REM: f32 = 1.5;
/// The library table's fixed columns. The name takes what is left.
const LIBRARY_SIZE_COLUMN_WIDTH_REM: f32 = 6.5;
const LIBRARY_STATUS_COLUMN_WIDTH_REM: f32 = 9.0;
const LIBRARY_CLOUD_COLUMN_WIDTH_REM: f32 = 3.0;
/// The column gap in the library table.
const LIBRARY_COLUMN_GAP: f32 = 12.0;
/// Keeps the empty line the height a library row's name has.
const LIBRARY_NAME_HEIGHT_REM: f32 = 2.5;

/// Who is signed in, as the first line of the account section.
///
/// The address with the two things that can be done to the account beside it.
/// They used to hang off a chip in the header, in a menu; as links they are
/// findable without opening anything, and they are quiet enough not to make
/// leaving the account the most prominent thing about having one.
pub fn account_identity(initial: impl Into<SharedString>, email: impl Into<SharedString>, actions: impl IntoIterator<Item = impl IntoElement>, theme: BrowserTheme) -> Div {
    div()
        .w_full()
        .py(px(SPACE_SM))
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap(px(SPACE_MD))
        .child(
            div()
                .w(gpui::rems(ACCOUNT_AVATAR_SIZE_REM))
                .h(gpui::rems(ACCOUNT_AVATAR_SIZE_REM))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .bg(theme.accent)
                .text_color(theme.accent_text)
                .text_size(gpui::rems(TEXT_SM))
                .child(initial.into()),
        )
        .child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().text_size(gpui::rems(TEXT_SM)).text_color(theme.text).child(email.into()))
        .child(div().flex_none().max_w_full().flex().flex_row().flex_wrap().items_center().gap(px(SPACE_MD)).children(actions))
}

/// The quota as one line: what is used against the quota, and what is left
/// opposite it.
pub fn storage_summary(used: impl Into<SharedString>, of_quota: impl Into<SharedString>, available: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    div()
        .w_full()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_baseline()
        .justify_between()
        .gap(px(SPACE_MD))
        .text_size(gpui::rems(TEXT_SM))
        .child(div().flex().flex_row().items_baseline().gap(px(SPACE_XS)).child(div().text_color(theme.text).child(used.into())).child(div().text_color(theme.text_muted).child(of_quota.into())))
        .child(div().text_color(theme.text_muted).child(available.into()))
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

/// The library table's column headings.
///
/// Every library states the same things, and a reader comparing two of them is
/// reading down a column, so the columns are named once rather than labelled in
/// every row. A compact window has no room for the columns and draws no heading.
pub fn library_table_header(theme: BrowserTheme) -> Div {
    let heading = |label: &'static str| div().text_size(gpui::rems(TEXT_XS)).text_color(theme.text_muted).font_features(crate::small_caps()).child(label);
    library_table_line(theme)
        .py(px(SPACE_XS))
        .child(heading("Library").flex_1().min_w_0())
        .child(heading("On this device").w(gpui::rems(LIBRARY_SIZE_COLUMN_WIDTH_REM)).flex_none())
        .child(heading("Status").w(gpui::rems(LIBRARY_STATUS_COLUMN_WIDTH_REM)).flex_none())
        .child(heading("Cloud").w(gpui::rems(LIBRARY_CLOUD_COLUMN_WIDTH_REM)).flex_none())
}

/// One library: its name and folder, what it occupies here, where it stands
/// with the server, and whether it is stored in the cloud.
///
/// Nothing is folded away. The folder and the cloud switch were behind a
/// details button, which hid the one control on the row and made every library
/// a two-step read.
///
/// A compact window keeps the status and cloud columns and moves the size
/// under the name, where it does not need a column of its own.
pub fn library_table_row(name: impl Into<SharedString>, path: Option<AnyElement>, size: impl Into<SharedString>, status: (impl Into<SharedString>, Hsla), cloud: impl IntoElement, compact: bool, theme: BrowserTheme) -> Div {
    let size = div().text_size(gpui::rems(TEXT_XS)).text_color(theme.text_muted).child(size.into());
    let title = div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .items_start()
        .gap(px(SPACE_XS))
        .child(div().w_full().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().font_weight(FontWeight::SEMIBOLD).text_size(gpui::rems(TEXT_SM)).text_color(theme.text).child(name.into()))
        .children(path.map(|path| div().max_w_full().min_w_0().child(path)));
    let (status, status_color) = status;
    let status = div()
        .min_w_0()
        .flex()
        .items_center()
        .gap(px(6.0))
        .text_size(gpui::rems(TEXT_XS))
        .text_color(theme.text)
        .child(div().size(px(SEGMENT_SWATCH_SIZE)).flex_none().rounded(px(SEGMENT_SWATCH_SIZE / 2.0)).bg(status_color))
        .child(div().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(status.into()));
    let cloud = div().flex_none().flex().items_center().child(cloud);
    if compact {
        return library_table_line(theme).py(px(12.0)).child(title.child(size)).child(status.flex_none()).child(cloud);
    }
    library_table_line(theme)
        .py(px(12.0))
        .child(title)
        .child(size.w(gpui::rems(LIBRARY_SIZE_COLUMN_WIDTH_REM)).flex_none())
        .child(status.w(gpui::rems(LIBRARY_STATUS_COLUMN_WIDTH_REM)).flex_none())
        .child(cloud.w(gpui::rems(LIBRARY_CLOUD_COLUMN_WIDTH_REM)))
}

/// A table line, ruled above like every row in Settings.
fn library_table_line(theme: BrowserTheme) -> Div {
    div().w_full().flex().flex_row().items_center().gap(px(LIBRARY_COLUMN_GAP)).border_t_1().border_color(theme.rule)
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
    div().text_size(gpui::rems(TEXT_XS)).text_color(theme.text_muted).child(text.into())
}

/// Vertical rhythm for the storage block: summary, meter, breakdown.
pub fn storage_block() -> Div {
    div().w_full().flex().flex_col().gap(px(SPACE_SM)).pt(px(SPACE_XS))
}
