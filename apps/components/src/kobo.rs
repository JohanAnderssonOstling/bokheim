//! The selected hardware profile also controls which device controls are shown.
use gpui::{App, Div, Global, Window, div, prelude::*, px, rems};
use gpui_component::{
    IconName, Root,
    scroll::{Scrollable, ScrollableElement},
};
use kobo_profile::{Frontlight, Profile};

mod popup;
mod slider;
pub use popup::{DevicePopover, has_open_panel};
pub use slider::ReleaseSlider;

const PANEL_WIDTH: f32 = 360.0;

pub struct DeviceProfile(pub &'static Profile);
impl Global for DeviceProfile {}

pub fn has_gyroscope(cx: &App) -> bool {
    cx.try_global::<DeviceProfile>().is_some_and(|profile| profile.0.has_gyroscope())
}

pub fn has_frontlight(cx: &App) -> bool {
    cx.try_global::<DeviceProfile>().is_some_and(|profile| profile.0.frontlight() != Frontlight::None)
}

pub fn has_warmth(cx: &App) -> bool {
    cx.try_global::<DeviceProfile>().is_some_and(|profile| !matches!(profile.0.frontlight(), Frontlight::None | Frontlight::Standard))
}

pub fn has_bluetooth(cx: &App) -> bool {
    cx.try_global::<DeviceProfile>().is_some_and(|profile| profile.0.has_bluetooth())
}

pub fn panel(window: &Window, cx: &App) -> Scrollable<Div> {
    let theme = crate::browser_theme(cx);
    div()
        .w(px(PANEL_WIDTH))
        .max_w((window.input_viewport().size.width - px(24.0)).max(px(1.0)))
        .max_h((window.input_viewport().size.height - window.rem_size() * 2.0 - px(12.0)).max(px(1.0)))
        .overflow_y_scrollbar()
        .flex()
        .flex_col()
        .gap(px(12.0))
        .p(px(16.0))
        .bg(theme.page_bg)
        .text_color(theme.text)
}

pub fn panel_header(title: impl Into<gpui::SharedString>) -> Div {
    div().flex().flex_none().items_center().gap(px(12.0)).child(div().flex_1().min_w_0().text_size(rems(1.375)).child(title.into()))
}

pub fn panel_close() -> crate::Button {
    crate::base_button("device-panel-close").icon(IconName::Close).tooltip(ui_text::text!(common::close)).ghost().no_fill().size(rems(2.0)).px_0().on_click(|_, window, cx| {
        cx.stop_propagation();
        Root::dismiss_active_overlay(window, cx);
    })
}
