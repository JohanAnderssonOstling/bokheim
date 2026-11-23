//! Kobo device chrome sits above the browser, independently of browser routes.
use ui_text::text;

mod battery;
mod wifi;
use gpui::prelude::*;
use gpui::{Context, Div, Entity, Render, Stateful, Window, px, rems};
use gpui_component::{Disableable, Icon, switch::Switch};
use kobo_device::bluetooth::{Request as BluetoothRequest, Status as BluetoothStatus};
use ui_components as components;
use ui_components::{Button, kobo::DevicePopover as Popover};
use wifi::WifiPanel;

const DEVICE_BAR_SIZE_REM: f32 = 2.0;

fn device_button(id: &'static str, label: impl Into<gpui::SharedString>, icon: impl Into<Icon>) -> Button {
    components::base_button(id).icon(icon).tooltip(label).ghost().no_fill().border_0().size(rems(DEVICE_BAR_SIZE_REM)).px_0()
}

pub(super) struct KoboDeviceBar {
    panel: Entity<WifiPanel>,
    bluetooth: Entity<BluetoothPanel>,
    brightness: Entity<reader_ui::BrightnessControls>,
    battery: Entity<battery::BatteryPanel>,
    services: browser_ui::AppServices,
    auto_rotate: bool,
    power_menu_open: bool,
    power_error: Option<String>,
    power_confirmation: Option<&'static str>,
}
impl KoboDeviceBar {
    pub(super) fn new(services: browser_ui::AppServices, auto_rotate: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let brightness = cx.new(|cx| reader_ui::BrightnessControls::new(window, cx));
        let battery = cx.new(|cx| battery::BatteryPanel::new(cx));
        cx.observe(&battery, |_, _, cx| cx.notify()).detach();
        let panel = cx.new(|cx| WifiPanel::new(window, cx));
        cx.observe(&panel, |_, _, cx| cx.notify()).detach();
        let bluetooth = cx.new(|cx| BluetoothPanel::new(cx));
        cx.observe(&bluetooth, |_, _, cx| cx.notify()).detach();
        let app = cx.to_async();
        let idle_app = app.clone();
        gpui_kobo::on_idle_sleep_check(move || idle_app.update(|cx| crate::playback::kobo_idle_sleep_allowed(cx)));
        gpui_kobo::on_prepare_sleep(move || {
            let _ = app.update(crate::playback::prepare_kobo_sleep);
        });
        let (power_events, pending_power_events) = async_channel::bounded(1);
        gpui_kobo::on_power_event(move || {
            let _ = power_events.try_send(());
        });
        cx.spawn(async move |bar, cx| {
            while pending_power_events.recv().await.is_ok() {
                if bar
                    .update(cx, |bar, cx| {
                        if gpui_kobo::take_power_menu_request() {
                            bar.power_menu_open = !bar.power_menu_open;
                            cx.notify();
                        }
                        if let Some(message) = gpui_kobo::take_power_message() {
                            bar.power_error = Some(match message {
                                gpui_kobo::KoboPowerMessage::LowBattery(percent) => format!(text!(startup::battery_low_connect_a_charger), percent = percent),
                                gpui_kobo::KoboPowerMessage::Failed(error) => {
                                    let error = operation_failure::OperationError::from_error(error.as_ref());
                                    components::user_facing_error(&error, text!(device::couldn_t_complete_the_power_action_retry_or_restart_the_app)).into()
                                }
                            });
                            bar.power_menu_open = true;
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        Self { panel, bluetooth, brightness, battery, services, auto_rotate, power_menu_open: false, power_error: None, power_confirmation: None }
    }

    fn exit_kobo(&mut self, action: &str, cx: &mut Context<Self>) {
        // Older launchers do not offer the exit action file.
        let Some(path) = std::env::var_os("BOKHEIM_KOBO_EXIT_ACTION_FILE") else {
            self.power_error = Some(text!(device::reinstall_using_the_latest_installer_then_retry_this_action).into());
            cx.notify();
            return;
        };
        match std::fs::write(path, action) {
            Ok(()) => cx.quit(),
            Err(error) => {
                log::warn!("Could not leave Bokheim: {error}");
                self.power_error = Some(text!(device::couldn_t_exit_the_app_restart_it_and_retry).into());
                cx.notify();
            }
        }
    }
}
impl KoboDeviceBar {
    fn render_bar(&mut self, compact: bool, window: &mut Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = components::browser_theme(cx);
        let has_frontlight = components::kobo::has_frontlight(cx);
        let has_gyroscope = components::kobo::has_gyroscope(cx);
        let has_bluetooth = components::kobo::has_bluetooth(cx);
        let state = self.panel.read(cx);
        let label = state.label();
        let icon = state.icon();
        let panel = self.panel.clone();
        let rotate_services = self.services.clone();
        let rotate_icon = if self.auto_rotate { "icons/lock-open.svg" } else { "icons/lock.svg" };
        let rotate_label = if self.auto_rotate { text!(device::auto_rotate_on) } else { text!(device::auto_rotate_off) };
        let bluetooth = self.bluetooth.clone();
        let bluetooth_state = bluetooth.read(cx).status.clone();
        let bluetooth_icon = if bluetooth_state.powered { "icons/bluetooth.svg" } else { "icons/bluetooth-off.svg" };
        let bluetooth_label = if bluetooth_state.available { if bluetooth_state.powered { text!(device::bluetooth_on) } else { text!(device::bluetooth_off) } } else { text!(device::bluetooth_unavailable) };
        gpui::div()
            .id("kobo-device-topbar")
            .flex_none()
            .flex()
            .items_center()
            .h(rems(DEVICE_BAR_SIZE_REM))
            .when(!compact, |bar| bar.w_full().px(px(components::CONTENT_INSET)))
            .gap(px(12.0))
            .bg(theme.page_bg)
            .child(
                Popover::new("kobo-power-menu")
                    .anchor(gpui::Anchor::TopLeft)
                    .p_0()
                    .open(self.power_menu_open)
                    .on_open_change(cx.listener(|bar, open, _, cx| {
                        bar.power_menu_open = *open;
                        if !*open {
                            bar.power_confirmation = None;
                        }
                        cx.notify();
                    }))
                    .trigger(device_button("kobo-power-button", text!(device::power), Icon::empty().path("icons/power.svg")))
                    .child(
                        components::kobo::panel(window, cx)
                            .child(components::kobo::panel_header(text!(device::power)).child(components::kobo::panel_close()))
                            .when(self.power_confirmation.is_none(), |panel| {
                                panel
                                    .child(components::base_button("kobo-sleep").label(text!(common::sleep)).ghost().no_fill().w_full().justify_start().min_h(px(52.0)).on_click(cx.listener(|bar, _, _, cx| {
                                        bar.power_menu_open = false;
                                        cx.notify();
                                        gpui_kobo::request_sleep();
                                    })))
                                    .child(components::base_button("kobo-return").label(text!(device::return_to_kobo)).ghost().no_fill().w_full().justify_start().min_h(px(52.0)).on_click(|_, _, cx| cx.quit()))
                                    .child(components::base_button("kobo-poweroff").label(text!(device::power_off)).ghost().no_fill().w_full().justify_start().min_h(px(52.0)).on_click(cx.listener(|bar, _, _, cx| {
                                        bar.power_confirmation = Some("poweroff");
                                        cx.notify();
                                    })))
                                    .child(components::base_button("kobo-restart").label(text!(common::restart)).ghost().no_fill().w_full().justify_start().min_h(px(52.0)).on_click(cx.listener(|bar, _, _, cx| {
                                        bar.power_confirmation = Some("reboot");
                                        cx.notify();
                                    })))
                            })
                            .when_some(self.power_confirmation, |panel, action| {
                                panel.child(if action == "reboot" { text!(common::restart) } else { text!(device::power_off) }).child(
                                    gpui::div()
                                        .flex()
                                        .justify_end()
                                        .gap(px(12.0))
                                        .child(components::base_button("kobo-power-cancel").label(text!(common::cancel)).ghost().no_fill().on_click(cx.listener(|bar, _, _, cx| {
                                            bar.power_confirmation = None;
                                            cx.notify();
                                        })))
                                        .child(
                                            components::base_button("kobo-power-confirm")
                                                .label(if action == "reboot" { text!(common::restart) } else { text!(device::power_off) })
                                                .on_click(cx.listener(move |bar, _, _, cx| bar.exit_kobo(action, cx))),
                                        ),
                                )
                            })
                            .children(self.power_error.clone()),
                    ),
            )
            .when(!compact, |bar| bar.child(gpui::div().flex_1()))
            .when(has_frontlight, |bar| bar.child(reader_ui::brightness_popover(self.brightness.clone(), None, window).trigger(device_button("kobo-brightness-button", text!(common::brightness), gpui_component::IconName::Sun))))
            .when(has_gyroscope, |bar| {
                bar.child(device_button("kobo-auto-rotate-button", rotate_label, Icon::empty().path(rotate_icon)).on_click(cx.listener(move |bar, _, _, cx| {
                    bar.auto_rotate = !bar.auto_rotate;
                    rotate_services.save_kobo_auto_rotate(bar.auto_rotate, cx);
                    cx.notify();
                })))
            })
            .when(has_bluetooth, |bar| {
                bar.child(
                    Popover::new("kobo-bluetooth-popup")
                        .anchor(gpui::Anchor::TopRight)
                        .p_0()
                        .trigger(device_button("kobo-bluetooth-button", bluetooth_label, Icon::empty().path(bluetooth_icon)))
                        .on_open_change({
                            let bluetooth = bluetooth.clone();
                            move |open, _, cx| {
                                if *open {
                                    bluetooth.update(cx, |panel, cx| panel.run(BluetoothRequest::Status, cx));
                                }
                            }
                        })
                        .child(bluetooth),
                )
            })
            .child(battery::popover(&self.battery, cx))
            .child(
                Popover::new("kobo-wifi-popup")
                    .anchor(gpui::Anchor::TopRight)
                    .p_0()
                    .shadow_none()
                    .border_color(theme.text)
                    .trigger(device_button("kobo-wifi-button", label, Icon::empty().path(icon)))
                    .on_open_change(move |open, window, cx| {
                        panel.update(cx, |panel, cx| {
                            panel.set_open(*open, window, cx);
                        });
                    })
                    .child(self.panel.clone()),
            )
    }
}

impl Render for KoboDeviceBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_bar(false, window, cx)
    }
}

/// The same controls and ordering as browse, fitted into a reader header.
pub(super) struct KoboDeviceHeader {
    bar: Entity<KoboDeviceBar>,
    _updates: gpui::Subscription,
}

impl KoboDeviceHeader {
    pub(super) fn new(bar: Entity<KoboDeviceBar>, cx: &mut Context<Self>) -> Self {
        let updates = cx.observe(&bar, |_, _, cx| cx.notify());
        Self { bar, _updates: updates }
    }
}

impl Render for KoboDeviceHeader {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.bar.update(cx, |bar, cx| bar.render_bar(true, window, cx))
    }
}

#[derive(Clone, PartialEq)]
enum BluetoothOperation {
    Power,
    Scan,
    Connect(String),
}

struct BluetoothPanel {
    status: BluetoothStatus,
    operation: Option<BluetoothOperation>,
    reading: bool,
    error: Option<String>,
}

impl BluetoothPanel {
    fn new(cx: &mut Context<Self>) -> Self {
        if components::kobo::has_bluetooth(cx) {
            cx.spawn(async move |panel, cx| {
                let _ = panel.update(cx, |panel, cx| panel.run(BluetoothRequest::Status, cx));
            })
            .detach();
        }
        Self { status: BluetoothStatus::default(), operation: None, reading: false, error: None }
    }

    fn run(&mut self, request: BluetoothRequest, cx: &mut Context<Self>) {
        let operation = match &request {
            BluetoothRequest::Status => None,
            BluetoothRequest::Power(_) => Some(BluetoothOperation::Power),
            BluetoothRequest::Scan => Some(BluetoothOperation::Scan),
            BluetoothRequest::Connect(address) => Some(BluetoothOperation::Connect(address.clone())),
        };
        if self.operation.is_some() || (operation.is_none() && self.reading) {
            return;
        }
        let reading = operation.is_none();
        if reading {
            self.reading = true;
        } else {
            self.operation = operation;
            self.error = None;
            cx.notify();
        }
        cx.spawn(async move |panel, cx| {
            let result = kobo_device::control::bluetooth(request).await;
            let _ = panel.update(cx, |panel, cx| {
                if reading {
                    panel.reading = false;
                    // A status read begun before an action must not undo it.
                    if panel.operation.is_some() {
                        return;
                    }
                }
                let previous = (panel.status.clone(), panel.error.clone());
                if !reading {
                    panel.operation = None;
                }
                match result {
                    Ok(status) => {
                        panel.status = status;
                        panel.error = None;
                    }
                    Err(error) => {
                        log::warn!("Bluetooth control failed: {error}");
                        panel.error = Some(text!(device::couldn_t_change_bluetooth_retry_or_restart_the_app).into());
                    }
                }
                if !reading || previous != (panel.status.clone(), panel.error.clone()) {
                    cx.notify();
                }
            });
        })
        .detach();
    }
}

impl Render for BluetoothPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let enabled = self.status.powered;
        let busy = self.operation.is_some();
        let scanning = self.operation == Some(BluetoothOperation::Scan);
        let mut panel = components::kobo::panel(window, cx).child(
            components::kobo::panel_header(text!(device::bluetooth))
                .child(
                    Switch::new("bluetooth-power")
                        .label(if enabled { text!(common::on) } else { text!(common::off) })
                        .checked(enabled)
                        .disabled(busy || !self.status.available)
                        .on_click(cx.listener(|panel, enabled, _, cx| panel.run(BluetoothRequest::Power(*enabled), cx))),
                )
                .child(
                    device_button("bluetooth-refresh", text!(device::search_for_devices), Icon::empty().path("icons/redo-2.svg"))
                        .disabled(busy || (!enabled && self.status.available))
                        .on_click(cx.listener(|panel, _, _, cx| panel.run(if panel.status.available { BluetoothRequest::Scan } else { BluetoothRequest::Status }, cx))),
                )
                .child(components::kobo::panel_close()),
        );
        if !self.status.available {
            panel = panel.child(gpui::div().text_color(theme.text_muted).child(text!(device::bluetooth_unavailable)));
        } else if enabled {
            if self.status.devices.is_empty() && !scanning {
                panel = panel.child(gpui::div().text_color(theme.text_muted).child(text!(device::no_devices_found_press_refresh_to_search)));
            }
            for (index, device) in self.status.devices.iter().enumerate() {
                let address = device.address.clone();
                let connected = device.connected;
                let connecting = self.operation.as_ref() == Some(&BluetoothOperation::Connect(address.clone()));
                panel = panel.child(
                    components::base_button(format!("bluetooth-device-{index}"))
                        .ghost()
                        .no_fill()
                        .w_full()
                        .justify_start()
                        .min_h(px(52.0))
                        .when(connected, |button| button.icon(Icon::empty().path("icons/check.svg")))
                        .child(gpui::div().flex_1().min_w_0().truncate().child(device.name.clone()))
                        .children(connecting.then_some(text!(device::connecting)))
                        .disabled(busy && !connecting)
                        .on_click(cx.listener(move |panel, _, _, cx| {
                            if !connected {
                                panel.run(BluetoothRequest::Connect(address.clone()), cx);
                            }
                        })),
                );
            }
        }
        panel = panel.child(gpui::div().min_h(px(24.0)).text_color(theme.text_muted).children(self.error.clone().or_else(|| scanning.then(|| text!(device::searching).to_owned()))));
        panel
    }
}
