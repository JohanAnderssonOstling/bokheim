//! Account page visuals: the verdict banner, the storage meter, and the
//! library usage rows that read against it.
//!
//! The page opens with its conclusion, so the banner is the only element on the
//! page allowed to carry a semantic colour at full strength; everything below it
//! is evidence for that conclusion and stays quiet.

use gpui::prelude::*;
use gpui::{div, px, relative, Div, FontWeight, Hsla, SharedString};

use crate::{BrowserTheme, SPACE_MD, SPACE_SM, SPACE_XS, SPACE_XXS, TEXT_SM, TEXT_XS, TITLE_LG, TITLE_MD};

/// Width of the quota meter and of the per-library comparison bars. Both are
/// read as lengths against each other, so they share a scale.
const METER_HEIGHT: f32 = 10.0;
const USAGE_BAR_WIDTH: f32 = 96.0;
const USAGE_BAR_HEIGHT: f32 = 4.0;
const USAGE_AMOUNT_WIDTH: f32 = 76.0;
const VERDICT_STRIPE_WIDTH: f32 = 3.0;

/// The banner the page opens with: one line that answers "is everything
/// synced?", the evidence for it underneath, and the actions that change it.
pub fn verdict_banner(headline: impl Into<SharedString>, detail: impl Into<SharedString>, settled: bool, actions: impl IntoElement, theme: BrowserTheme) -> Div {
    // A settled page carries no state colour: the stripe stays on the theme's
    // own accent, and amber appears only when something is actually waiting.
    let state_color = if settled { theme.accent } else { theme.warning };
    // The stripe is a child rather than a left border because a border colour in
    // GPUI applies to every side; a coloured frame would make the whole banner
    // read as an alert.
    div().w_full().flex().flex_row().items_stretch().border_1().border_color(theme.border).bg(theme.raised_bg).child(div().w(px(VERDICT_STRIPE_WIDTH)).flex_none().bg(state_color)).child(
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .flex_wrap()
            .gap(px(SPACE_MD))
            .px(px(SPACE_MD))
            .py(px(SPACE_MD))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(SPACE_XXS))
                    .child(div().text_size(gpui::rems(TITLE_MD / 16.0)).font_weight(FontWeight::SEMIBOLD).child(headline.into()))
                    .child(div().text_size(gpui::rems(TEXT_XS / 16.0)).text_color(theme.text_muted).child(detail.into())),
            )
            .child(actions),
    )
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

/// The single line that stands in for the batch job sections while they are
/// empty. It states what is happening and offers the way in; it never grows
/// into a section of its own.
pub fn activity_footer(summary: impl Into<SharedString>, action: impl IntoElement, theme: BrowserTheme) -> Div {
    div()
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .flex_wrap()
        .gap(px(SPACE_SM))
        .px(px(SPACE_MD))
        .py(px(SPACE_SM))
        .border_1()
        .border_color(theme.border)
        .bg(theme.raised_bg)
        .child(div().flex_1().min_w_0().text_size(gpui::rems(TEXT_XS / 16.0)).text_color(theme.text_muted).child(summary.into()))
        .child(action)
}

/// Vertical rhythm for the storage block: figures, meter, legend.
pub fn storage_block() -> Div {
    div().w_full().flex().flex_col().gap(px(SPACE_SM)).pt(px(SPACE_XS))
}
