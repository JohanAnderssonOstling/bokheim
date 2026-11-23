//! The reading ruler settings UI shared by HTML and PDF readers.

use ui_text::text;

use gpui::{Context, Div, div, prelude::*, px, rems};
use ui_components as components;

use super::settings_navigation::{SettingsNavigation, SettingsRow};

/// Each renderer supplies its own style type and preference setters. Labels,
/// controls, limits, and keyboard row highlighting are defined here once.
pub(crate) struct ReadingRulerSettings<C: 'static, S> {
    pub current: S,
    pub off: S,
    /// Band, Focus, and Lines, in their displayed order.
    pub styles: [S; 3],
    pub lines: usize,
    pub set_enabled: fn(&C, bool, &mut Context<C>),
    pub set_style: fn(&C, S, &mut Context<C>),
    pub set_lines: fn(&C, usize, &mut Context<C>),
}

pub(crate) fn append_rows(rows: &mut Vec<SettingsRow>, enabled: bool) {
    rows.push(SettingsRow::ReadingRulerStyle);
    if enabled {
        rows.push(SettingsRow::ReadingRulerLines);
    }
}

pub(crate) fn render<C: 'static, S: Copy + PartialEq + 'static>(settings: ReadingRulerSettings<C, S>, navigation: &SettingsNavigation, rows: &[SettingsRow], theme: components::BrowserTheme, cx: &mut Context<C>) -> Div {
    let ReadingRulerSettings { current, off, styles, lines, set_enabled, set_style, set_lines } = settings;
    let enabled = current != off;
    let off = components::settings_choice_button("reader-ruler-off", text!(common::off), !enabled, theme).on_click(cx.listener(move |reader, _, _, cx| set_enabled(reader, false, cx)));
    let choices = styles
        .into_iter()
        .zip([text!(reader::band), text!(reader::focus), text!(reader::lines)])
        .enumerate()
        .map(|(index, (style, label))| components::settings_choice_button(("reader-ruler-style", index), label, current == style, theme).on_click(cx.listener(move |reader, _, _, cx| set_style(reader, style, cx))));
    let style = components::reader_setting_row(text!(reader::reading_ruler), theme).child(components::reader_choice_button_group("reader-ruler-style", std::iter::once(off).chain(choices), theme));
    let lines = lines.clamp(1, 5);
    let count = components::reader_setting_row(text!(reader::ruler_lines), theme).child(components::reader_settings_stepper(
        components::settings_stepper_button("reader-ruler-fewer-lines", text!(reader::minus), !enabled || lines == 1).on_click(cx.listener(move |reader, _, _, cx| set_lines(reader, lines.saturating_sub(1), cx))),
        lines.to_string(),
        components::settings_stepper_button("reader-ruler-more-lines", "+", !enabled || lines == 5).on_click(cx.listener(move |reader, _, _, cx| set_lines(reader, lines + 1, cx))),
        theme,
    ));
    components::reader_settings_group(theme).child(
        div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_row()
            .flex_wrap()
            .items_start()
            .gap(px(components::READER_SETTINGS_GAP))
            .child(navigation.row(SettingsRow::ReadingRulerStyle, rows, style, theme).flex_grow(2.0).flex_shrink(1.0).flex_basis(rems(20.0)).min_w_0().max_w_full())
            .child(navigation.row(SettingsRow::ReadingRulerLines, rows, count, theme).flex_1().flex_basis(rems(11.0)).min_w_0().max_w_full()),
    )
}
