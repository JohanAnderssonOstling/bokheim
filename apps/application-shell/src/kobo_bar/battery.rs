use ui_text::text;

use gpui::{Context, Entity, Render, Window, prelude::*, px};
use gpui_component::{IconName, popover::Popover};
use kobo_device::battery::{BatteryReading, BatteryStatus};

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
                self.error = Some(text!(device::could_not_read_the_battery_level_try_reopening_this_panel).into());
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
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = ui_components::browser_theme(cx);
        gpui::div()
            .p(px(12.0))
            .flex()
            .flex_col()
            .gap(px(8.0))
            .bg(theme.page_bg)
            .text_color(theme.text)
            .child(text!(device::battery))
            .child(self.status.as_ref().map(|status| format!("{}% · {}", status.percent, status.status)).unwrap_or_else(|| self.error.clone().unwrap_or_else(|| text!(common::loading).into())))
    }
}
pub(super) fn popover(panel: &Entity<BatteryPanel>, top: gpui::Pixels, cx: &gpui::App) -> Popover {
    let state = panel.read(cx);
    let icon = match &state.status {
        Some(status) if status.status == text!(device::charging) => IconName::BatteryCharging,
        Some(status) if status.percent <= 20 => IconName::BatteryLow,
        Some(status) if status.percent <= 70 => IconName::BatteryMedium,
        Some(_) => IconName::BatteryFull,
        None => IconName::BatteryWarning,
    };
    let label = state.status.as_ref().map(|status| format!("{}%", status.percent)).unwrap_or_else(|| "—".into());
    let refresh = panel.clone();
    Popover::new("kobo-battery-popup")
        .anchor(gpui::Anchor::TopRight)
        .top(top)
        .p_0()
        .trigger(super::device_button("kobo-battery-button", format!(text!(device::battery_state), label = label), icon))
        .on_open_change(move |open, _, cx| {
            if *open {
                refresh.update(cx, |panel, cx| panel.refresh(cx));
            }
        })
        .child(panel.clone())
}
