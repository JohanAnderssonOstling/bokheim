//! Account section visuals: the identity block, the storage meter, the library
//! usage rows that read against it, and the activity list under them.
//!
//! The section opens with who is signed in, and the sync state is one line
//! inside that block rather than a banner above it. Only that line and the
//! activity dots carry a semantic colour, and only while something is actually
//! waiting; everything else is quiet.

use gpui::prelude::*;
use gpui::{Div, FontWeight, Hsla, SharedString, div, px, relative};

use crate::{BrowserTheme, SPACE_MD, SPACE_SM, SPACE_XS, SPACE_XXS, TEXT_SM, TEXT_XS, TITLE_LG, TITLE_MD};

/// Width of the quota meter and of the per-library comparison bars. Both are
/// read as lengths against each other, so they share a scale.
const METER_HEIGHT: f32 = 10.0;
const USAGE_BAR_WIDTH: f32 = 96.0;
const USAGE_BAR_HEIGHT: f32 = 4.0;
const USAGE_AMOUNT_WIDTH: f32 = 76.0;
const AVATAR_SIZE: f32 = 40.0;
const EVENT_DOT_SIZE: f32 = 8.0;
/// Half the gap between the dot and the cap height of the line beside it.
const EVENT_DOT_OFFSET: f32 = 6.0;
const PROGRESS_HEIGHT: f32 = 3.0;

/// The account itself: who is signed in, one line on how the sync is doing, and
/// the two actions that change either.
///
/// The page opens with identity rather than with a verdict, so the sync state
/// is a line inside this block instead of a banner above it. It keeps a colour
/// only while something is actually waiting; a settled account is not news.
pub fn account_identity(initial: impl Into<SharedString>, email: impl Into<SharedString>, status: impl Into<SharedString>, settled: bool, actions: impl IntoElement, theme: BrowserTheme) -> Div {
    let status_color = if settled { theme.text_muted } else { theme.warning };
    div()
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .flex_wrap()
        .gap(px(SPACE_MD))
        .px(px(SPACE_MD))
        .py(px(SPACE_MD))
        .border_1()
        .border_color(theme.border)
        .bg(theme.raised_bg)
        .child(
            div()
                .w(px(AVATAR_SIZE))
                .h(px(AVATAR_SIZE))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(AVATAR_SIZE / 2.0))
                .bg(theme.accent)
                .text_color(theme.page_bg)
                .text_size(gpui::rems(TITLE_MD / 16.0))
                .font_weight(FontWeight::SEMIBOLD)
                .child(initial.into()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(SPACE_XXS))
                .child(div().line_clamp(1).text_size(gpui::rems(TITLE_MD / 16.0)).font_weight(FontWeight::SEMIBOLD).child(email.into()))
                .child(div().text_size(gpui::rems(TEXT_XS / 16.0)).text_color(status_color).child(status.into())),
        )
        .child(actions)
}

/// The quota as a figure rather than a sentence: the used amount is the largest
/// text on the page after the verdict, and what is left sits opposite it.
pub fn storage_figures(used: impl Into<SharedString>, of_quota: impl Into<SharedString>, available: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    div()
        .w_full()
        .flex()
        .flex_row()
        .items_baseline()
        .justify_between()
        .gap(px(SPACE_MD))
        .child(
            div()
                .flex()
                .flex_row()
                .items_baseline()
                .gap(px(SPACE_XS))
                .child(div().text_size(gpui::rems(TITLE_LG / 16.0)).font_weight(FontWeight::SEMIBOLD).child(used.into()))
                .child(div().text_size(gpui::rems(TEXT_SM / 16.0)).text_color(theme.text_muted).child(of_quota.into())),
        )
        .child(div().text_size(gpui::rems(TEXT_SM / 16.0)).text_color(theme.text_muted).child(available.into()))
}

/// A horizontal track that the caller fills with [`meter_segment`] children.
/// Whatever is left unfilled is free space, so the track keeps the page
/// background rather than a colour of its own.
pub fn meter_track(theme: BrowserTheme) -> Div {
    div().w_full().h(px(METER_HEIGHT)).flex().flex_row().overflow_hidden().border_1().border_color(theme.border).bg(theme.page_bg)
}

/// `fraction` is the share of the whole quota this segment occupies.
pub fn meter_segment(fraction: f32, color: Hsla) -> Div {
    div().h_full().w(relative(fraction.clamp(0.0, 1.0))).bg(color)
}

pub fn meter_legend() -> Div {
    div().w_full().flex().flex_row().flex_wrap().gap_x(px(SPACE_MD)).gap_y(px(SPACE_XS))
}

pub fn meter_legend_entry(color: Hsla, label: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    div().flex().flex_row().items_center().gap(px(SPACE_XS)).child(div().w(px(8.0)).h(px(8.0)).flex_none().bg(color)).child(div().text_size(gpui::rems(TEXT_XS / 16.0)).text_color(theme.text_muted).child(label.into()))
}

/// Shades for the quota segments, walking the accent away from itself so
/// adjacent libraries stay distinguishable without introducing a second hue.
///
/// Past the shades that stay legible the sequence repeats rather than fading
/// into the track; a library with a colour it shares with another is a smaller
/// error than a library that appears to occupy nothing.
pub fn usage_segment_color(index: usize, theme: BrowserTheme) -> Hsla {
    const STEPS: usize = 5;
    let step = (index % STEPS) as f32 / STEPS as f32;
    let mut color = theme.accent;
    color.l = (color.l + step * 0.34).clamp(0.0, 0.92);
    color.s = (color.s - step * 0.18).clamp(0.0, 1.0);
    color
}

/// A right-aligned amount in a fixed column so sizes stack into a readable
/// column instead of drifting with the library name.
pub fn usage_amount(amount: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    div().w(px(USAGE_AMOUNT_WIDTH)).flex_none().text_right().text_size(gpui::rems(TEXT_XS / 16.0)).text_color(theme.text).child(amount.into())
}

/// The second of the two amounts a library row carries. Quieter than
/// [`usage_amount`] because the page leads with what the account is charged
/// for; what is downloaded here answers a different question.
pub fn usage_amount_secondary(amount: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    div().w(px(USAGE_AMOUNT_WIDTH)).flex_none().text_right().text_size(gpui::rems(TEXT_XS / 16.0)).text_color(theme.text_muted).child(amount.into())
}

/// Names the two amount columns, so a row of bare numbers cannot be read as one
/// quantity measured twice.
pub fn usage_column_headings(server: Option<SharedString>, device: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    let heading = |label: SharedString| div().w(px(USAGE_AMOUNT_WIDTH)).flex_none().text_right().text_size(gpui::rems(TEXT_XS / 16.0)).font_weight(FontWeight::SEMIBOLD).text_color(theme.text_muted).child(label);
    let mut row = div().w_full().flex().flex_row().items_center().gap(px(SPACE_MD)).px(px(SPACE_XS)).pb(px(SPACE_XS)).child(div().flex_1().min_w_0());
    if let Some(server) = server {
        row = row.child(heading(server));
    }
    // The trailing spacers keep the headings above the columns they name: the
    // comparison bar and the state tag occupy the rest of every row.
    row.child(heading(device.into())).child(div().w(px(USAGE_BAR_WIDTH)).flex_none())
}

/// One library's share of the largest library on the device, so the rows can be
/// compared to each other at a glance.
pub fn usage_bar(fraction: f32, theme: BrowserTheme) -> Div {
    div().w(px(USAGE_BAR_WIDTH)).h(px(USAGE_BAR_HEIGHT)).flex_none().flex().flex_row().overflow_hidden().bg(theme.page_bg).child(div().h_full().w(relative(fraction.clamp(0.0, 1.0))).bg(theme.accent))
}

/// The activity section: what the device has been doing, newest first.
///
/// Batch jobs are the only part of the page that reports events rather than
/// state, so they are a list of their own rather than rows in a table.
pub fn activity_list() -> Div {
    div().w_full().flex().flex_col()
}

/// One batch job. The dot carries the state, so the text never has to repeat
/// it; `progress` is the share done and is drawn only while a job is running,
/// where it is the only thing that changes between refreshes.
pub fn activity_event(state_color: Hsla, title: impl Into<SharedString>, detail: impl Into<SharedString>, progress: Option<f32>, theme: BrowserTheme) -> Div {
    let detail = detail.into();
    let mut text = div().flex_1().min_w_0().flex().flex_col().gap(px(SPACE_XXS)).child(div().text_size(gpui::rems(TEXT_SM / 16.0)).font_weight(FontWeight::SEMIBOLD).child(title.into()));
    if !detail.is_empty() {
        text = text.child(div().text_size(gpui::rems(TEXT_XS / 16.0)).text_color(theme.text_muted).child(detail));
    }
    if let Some(progress) = progress {
        text = text.child(div().w_full().h(px(PROGRESS_HEIGHT)).bg(theme.page_bg).child(div().h_full().w(relative(progress.clamp(0.0, 1.0))).bg(theme.accent)));
    }
    div()
        .w_full()
        .flex()
        .flex_row()
        .items_start()
        .gap(px(SPACE_SM))
        .py(px(SPACE_SM))
        .border_b_1()
        .border_color(theme.border)
        // The dot sits on the first line's optical centre rather than the top of
        // the row, so a two-line event does not appear to hang off it.
        .child(div().w(px(EVENT_DOT_SIZE)).h(px(EVENT_DOT_SIZE)).flex_none().mt(px(EVENT_DOT_OFFSET)).rounded(px(EVENT_DOT_SIZE / 2.0)).bg(state_color))
        .child(text)
}

/// Vertical rhythm for the storage block/// The signed-out form: a title, the inputs, the actions, and whatever the last
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
    div().text_size(gpui::rems(TEXT_XS / 16.0)).text_color(theme.text_muted).child(text.into())
}

/// Vertical rhythm for the storage block: figures, meter, legend.
pub fn storage_block() -> Div {
    div().w_full().flex().flex_col().gap(px(SPACE_SM)).pt(px(SPACE_XS))
}
