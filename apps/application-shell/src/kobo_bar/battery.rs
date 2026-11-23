use ui_text::text;

use gpui::{App, Context, Div, Entity, Render, Window, div, prelude::*, px, rems};
use gpui_component::Icon;
use kobo_device::battery::{BatteryReading, BatteryStatus};
use ui_components::kobo::DevicePopover as Popover;

pub(super) struct BatteryPanel {
    status: Option<BatteryStatus>,
    error: Option<String>,
    busy: bool,
}
impl BatteryPanel {
    pub(super) fn new(cx: &mut Context<Self>) -> Self {
        let mut readings = kobo_device::battery::subscribe();
        cx.spawn(async move |panel, cx| {
            loop {
                let reading = readings.borrow_and_update().clone();
                if let Some(reading) = reading {
                    if panel.update(cx, |panel, cx| panel.apply_reading(reading, cx)).is_err() {
                        break;
                    }
                }
                if readings.changed().await.is_err() {
                    break;
                }
            }
        })
        .detach();
        Self { status: None, error: None, busy: false }
    }

    fn apply_reading(&mut self, reading: BatteryReading, cx: &mut Context<Self>) {
        let previous = (self.status.clone(), self.error.clone());
        match reading {
            Ok(status) => {
                self.status = Some(status);
                self.error = None;
            }
            Err(error) => {
                self.status = None;
                log::warn!("Device control failed: {error}");
                self.error = Some(ui_components::user_facing_error(&error, text!(device::could_not_read_the_battery_level_try_reopening_this_panel)).into());
            }
        }
        if previous != (self.status.clone(), self.error.clone()) {
            cx.notify();
        }
    }
    fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        cx.spawn(async move |panel, cx| {
            let result = kobo_device::control::battery().await;
            let _ = panel.update(cx, |panel, cx| {
                panel.busy = false;
                match result {
                    Ok(_) => {}
                    Err(error) => {
                        panel.status = None;
                        log::warn!("Device control failed: {error}");
                        panel.error = Some(text!(device::could_not_read_the_battery_level_try_reopening_this_panel).into());
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }
}
impl Render for BatteryPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = ui_components::browser_theme(cx);
        ui_components::kobo::panel(window, cx)
            .child(ui_components::kobo::panel_header(text!(device::battery)).child(ui_components::kobo::panel_close()))
            .when_some(self.status.as_ref(), |panel, status| {
                panel.child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .py(px(12.0))
                        .gap(px(8.0))
                        .child(div().text_size(rems(4.0)).child(format!("{}%", status.percent)))
                        .child(div().text_color(theme.text_muted).child(if status.status == "Charging" { text!(device::charging) } else { status.status.as_str() }.to_owned()))
                        .child(div().w_full().h(px(12.0)).border_1().border_color(theme.text).p(px(2.0)).child(div().h_full().w(gpui::relative(f32::from(status.percent) / 100.0)).bg(theme.text))),
                )
            })
            .when(self.status.is_none(), |panel| panel.child(self.error.clone().unwrap_or_else(|| text!(common::loading).into())))
    }
}
fn indicator(status: Option<&BatteryStatus>, failed: bool, cx: &App) -> Div {
    let theme = ui_components::browser_theme(cx);
    let charging = status.is_some_and(|status| status.status == "Charging");
    let low = status.is_some_and(|status| status.percent <= 20) && !charging;
    let value = status.map(|status| status.percent.to_string()).unwrap_or_else(|| if failed { "!" } else { "…" }.into());
    div()
        .relative()
        .flex_none()
        .w(rems(1.75))
        .h(rems(1.25))
        .child(
            div()
                .absolute()
                .left_0()
                .top(rems(0.1875))
                .w(rems(1.5))
                .h(rems(0.875))
                .border_1()
                .border_color(theme.text)
                .rounded(rems(0.125))
                .flex()
                .items_center()
                .justify_center()
                .gap(rems(0.0625))
                .text_size(rems(0.5625))
                .line_height(rems(0.75))
                .font_weight(gpui::FontWeight::BOLD)
                .when(low, |body| body.bg(theme.text).text_color(theme.page_bg))
                .children(charging.then(|| Icon::empty().path("icons/lightning.svg").w(rems(0.375)).h(rems(0.625))))
                .child(value),
        )
        .child(div().absolute().right_0().top(rems(0.4375)).w(rems(0.125)).h(rems(0.375)).bg(theme.text))
}

pub(super) fn popover(panel: &Entity<BatteryPanel>, cx: &App) -> Popover {
    let state = panel.read(cx);
    let label = state.status.as_ref().map(|status| format!("{}%", status.percent)).unwrap_or_else(|| "—".into());
    let trigger = ui_components::base_button("kobo-battery-button").tooltip(format!(text!(device::battery_state), label = label)).ghost().no_fill().border_0().size(rems(super::DEVICE_BAR_SIZE_REM)).px_0().child(indicator(
        state.status.as_ref(),
        state.error.is_some(),
        cx,
    ));
    let refresh = panel.clone();
    Popover::new("kobo-battery-popup")
        .anchor(gpui::Anchor::TopRight)
        .p_0()
        .trigger(trigger)
        .on_open_change(move |open, _, cx| {
            if *open {
                refresh.update(cx, |panel, cx| panel.refresh(cx));
            }
        })
        .child(panel.clone())
}
