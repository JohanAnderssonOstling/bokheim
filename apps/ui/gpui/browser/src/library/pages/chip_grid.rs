//! Shared responsive chip-grid rendering.

use std::ops::Range;

use gpui::prelude::*;
use gpui::{App, ClickEvent, Div, ElementId, FontWeight, SharedString, Stateful, Window, div, px, rems};
use gpui_component::label::Label;
use gpui_component::{Icon, IconName, Sizable};
use ui_components::BrowserTheme;

const CHIP_HEIGHT_REM: f32 = 3.5;
const CHIP_MIN_WIDTH_REM: f32 = 15.0;
const CHIP_GRID_GAP_PX: f32 = 4.0;
const CHIP_HORIZONTAL_PADDING_PX: f32 = 12.0;
const CHIP_CONTENT_GAP_PX: f32 = 10.0;

#[derive(Clone, Copy)]
pub(crate) struct ChipGridLayout {
    pub(crate) columns: usize,
    pub(crate) row_count: usize,
    pub(crate) row_height: f32,
}

pub(crate) fn layout(available_width: f32, rem_size: f32, chip_count: usize) -> ChipGridLayout {
    let min_width = CHIP_MIN_WIDTH_REM * rem_size;
    let available_width = available_width.max(min_width);
    let columns = (((available_width + CHIP_GRID_GAP_PX) / (min_width + CHIP_GRID_GAP_PX)).floor() as usize).max(1).min(chip_count.max(1));

    ChipGridLayout { columns, row_count: chip_count.div_ceil(columns), row_height: CHIP_HEIGHT_REM * rem_size + CHIP_GRID_GAP_PX }
}

pub(crate) fn row_range(row: usize, layout: ChipGridLayout, chip_count: usize) -> Range<usize> {
    let start = row.saturating_mul(layout.columns).min(chip_count);
    start..(start + layout.columns).min(chip_count)
}

pub(crate) fn row(layout: ChipGridLayout) -> Div {
    div().w_full().h(px(layout.row_height)).grid().grid_cols(layout.columns as u16).gap(px(CHIP_GRID_GAP_PX))
}

pub(crate) fn chip(id: ElementId, name: SharedString, count: usize, on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static, cx: &App) -> Stateful<Div> {
    let theme = ui_components::browser_theme(cx);

    div().id(id).h(rems(CHIP_HEIGHT_REM)).px(px(CHIP_HORIZONTAL_PADDING_PX)).flex().items_center().gap(px(CHIP_CONTENT_GAP_PX)).bg(theme.raised_bg).child(icon(theme)).child(name_label(name)).child(number(count, theme)).on_click(on_click)
}

fn icon(theme: BrowserTheme) -> Icon {
    Icon::new(IconName::Folder).small().text_color(theme.accent)
}

fn name_label(name: SharedString) -> Label {
    Label::new(name).flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().font_weight(FontWeight::MEDIUM)
}

fn number(count: usize, theme: BrowserTheme) -> Label {
    Label::new(count.to_string()).flex_none().text_color(theme.text_muted)
}
