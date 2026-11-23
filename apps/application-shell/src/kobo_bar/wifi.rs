use gpui::{Context, Div, Entity, FocusHandle, Focusable, FontWeight, Render, TextAlign, Window, div, prelude::*, px, rems};
use gpui_component::{
    Disableable, Icon,
    input::{Input, InputContentType, InputEvent, InputState},
};
use kobo_device::wifi::{Network, Phase, Request, Security, Snapshot};
use ui_components as components;
use ui_text::text;

use super::device_button;

#[derive(PartialEq, Eq)]
enum Operation {
    Power,
    Scan,
    Connect(String),
    Disconnect,
}

pub(super) struct WifiPanel {
    snapshot: Snapshot,
    operation: Option<Operation>,
    request_id: u64,
    reading: bool,
    open: bool,
    error: Option<String>,
    selected: Option<Network>,
    password: Entity<InputState>,
    password_valid: bool,
    focus: FocusHandle,
}

impl WifiPanel {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let password = cx.new(|cx| InputState::new(window, cx).placeholder(text!(common::password)).masked(true));
        cx.subscribe_in(&password, window, |panel, _, event, window, cx| match event {
            InputEvent::Change => {
                let password = panel.password.read(cx).value();
                let valid = (8..=63).contains(&password.len()) && password.is_ascii() && !password.bytes().any(|byte| byte.is_ascii_control());
                let changed = panel.password_valid != valid;
                panel.password_valid = valid;
                // Typing already redraws the input. Only redraw the surrounding
                // form when the submit state or error actually changes.
                let cleared_error = panel.selected.is_some() && panel.error.take().is_some();
                if changed || cleared_error {
                    cx.notify();
                }
            }
            InputEvent::PressEnter { .. } => panel.submit(window, cx),
            _ => {}
        })
        .detach();
        match kobo_device::wifi::subscribe() {
            Ok(mut changes) => {
                cx.spawn_in(window, async move |panel, cx| {
                    loop {
                        let snapshot = changes.borrow_and_update().clone();
                        if panel.update_in(cx, |panel, window, cx| panel.apply_snapshot(snapshot, window, cx)).is_err() {
                            break;
                        }
                        if changes.changed().await.is_err() {
                            break;
                        }
                    }
                })
                .detach();
            }
            Err(error) => log::warn!("Could not observe Kobo Wi-Fi: {error}"),
        }
        Self { snapshot: Snapshot::default(), operation: None, request_id: 0, reading: false, open: false, error: None, selected: None, password, password_valid: false, focus: cx.focus_handle() }
    }

    pub(super) fn set_open(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.open = open;
        if open {
            if self.snapshot.phase != Phase::Off && !self.snapshot.scanning && self.operation.is_none() {
                self.run(Request::Scan, window, cx);
            } else {
                self.run(Request::Status, window, cx);
            }
        } else {
            self.clear_entry(window, cx);
        }
    }

    fn clear_entry(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = None;
        self.password_valid = false;
        self.password.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.set_masked(true, cx);
        });
        if self.password.read(cx).focus_handle(cx).is_focused(window) {
            self.focus.focus(window, cx);
        }
    }

    fn apply_snapshot(&mut self, snapshot: Snapshot, window: &mut Window, cx: &mut Context<Self>) {
        let became_enabled = self.snapshot.phase == Phase::Off && snapshot.phase != Phase::Off;
        let previous_label = self.label();
        let previous_error = self.error.clone();
        let panel_changed = self.open
            && (visible_phase(self.snapshot.phase) != visible_phase(snapshot.phase)
                || self.snapshot.network != snapshot.network
                || self.snapshot.scanning != snapshot.scanning
                || self.snapshot.power_available != snapshot.power_available
                || !network_rows(&self.snapshot).iter().map(NetworkRow::visible).eq(network_rows(&snapshot).iter().map(NetworkRow::visible)));
        if self.snapshot.error != snapshot.error || snapshot.error.is_none() {
            self.error = snapshot.error.as_ref().map(|error| components::wifi_error_message(error).to_owned());
        }
        if snapshot.phase == Phase::Connected && self.selected.as_ref().is_some_and(|network| snapshot.network.as_deref() == Some(network.name().as_str())) {
            self.clear_entry(window, cx);
        } else if self.open && snapshot.error.is_some() && self.snapshot.error != snapshot.error {
            // Saved credentials can fail too: offer the same password form.
            if self.selected.is_none()
                && let Some(name) = &self.snapshot.network
            {
                self.selected = snapshot.networks.iter().find(|network| network.name() == *name && network.security == Security::WpaPersonal).cloned();
            }
            if self.selected.as_ref().is_some_and(|network| network.security == Security::WpaPersonal && self.snapshot.network.as_deref() == Some(network.name().as_str())) {
                self.password.update(cx, |input, cx| input.focus(window, cx));
            }
        }
        self.snapshot = snapshot;
        if previous_label != self.label() || panel_changed || (self.open && previous_error != self.error) {
            cx.notify();
        }
        if self.open && became_enabled && self.snapshot.error.is_none() && !self.snapshot.scanning && self.operation.is_none() {
            self.run(Request::Scan, window, cx);
        }
    }

    pub(super) fn label(&self) -> &'static str {
        if self.unavailable() {
            return text!(device::wi_fi_unavailable);
        }
        if self.connecting_network().is_some() {
            return text!(device::connecting);
        }
        match self.snapshot.phase {
            Phase::Off => text!(device::wi_fi_off),
            Phase::Connected => text!(device::wi_fi_connected),
            _ => text!(device::wi_fi),
        }
    }

    pub(super) fn icon(&self) -> &'static str {
        // Connection progress lives in the selected row; the bar stays still.
        if self.snapshot.phase == Phase::Off { "icons/wifi-off.svg" } else { "icons/wifi.svg" }
    }

    fn unavailable(&self) -> bool {
        self.snapshot.phase == Phase::Off && !self.snapshot.power_available
    }

    fn connecting_network(&self) -> Option<&str> {
        match &self.operation {
            Some(Operation::Connect(name)) => Some(name),
            Some(Operation::Power | Operation::Disconnect) => None,
            _ if self.snapshot.error.is_none() && matches!(self.snapshot.phase, Phase::Connecting | Phase::ObtainingAddress) => self.snapshot.network.as_deref(),
            _ => None,
        }
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.password_valid
            && let Some(network) = self.selected.clone()
        {
            let password = self.password.read(cx).value().to_string();
            self.run(Request::Join { network, password }, window, cx);
        }
    }

    fn select(&mut self, row: &NetworkRow, window: &mut Window, cx: &mut Context<Self>) {
        if row.connected || self.connecting_network() == Some(row.name.as_str()) || self.operation == Some(Operation::Power) {
            return;
        }
        self.clear_entry(window, cx);
        self.error = None;
        if let Some(id) = row.saved_id {
            self.run(Request::ConnectSaved(id), window, cx);
        } else if let Some(network) = &row.network {
            match network.security {
                Security::Open => self.run(Request::Join { network: network.clone(), password: String::new() }, window, cx),
                Security::WpaPersonal => {
                    self.selected = Some(network.clone());
                    self.password.update(cx, |input, cx| input.focus(window, cx));
                    cx.notify();
                }
                Security::Unsupported => {}
            }
        }
    }

    fn run(&mut self, request: Request, window: &mut Window, cx: &mut Context<Self>) {
        let powering_on = matches!(request, Request::Power(true));
        let operation = match &request {
            Request::Status => None,
            Request::Power(_) => Some(Operation::Power),
            Request::Scan => Some(Operation::Scan),
            Request::ConnectSaved(id) => Some(Operation::Connect(self.snapshot.saved.iter().find(|network| network.id == *id).map(|network| network.name.clone()).unwrap_or_default())),
            Request::Join { network, .. } => Some(Operation::Connect(network.name())),
            Request::Disconnect => Some(Operation::Disconnect),
        };
        let reading = operation.is_none();
        if self.operation == Some(Operation::Power) || (reading && (self.reading || self.operation.is_some())) {
            return;
        }
        if let Some(Operation::Connect(name)) = &operation {
            if self.connecting_network() == Some(name.as_str()) {
                return;
            }
            // The form and keyboard disappear in the same frame as the
            // selected row becomes Connecting. Keep credentials for retry.
            if self.password.read(cx).focus_handle(cx).is_focused(window) {
                self.focus.focus(window, cx);
            }
        }
        if reading {
            self.reading = true;
        } else {
            self.request_id += 1;
            self.operation = operation;
            self.error = None;
            cx.notify();
        }
        let request_id = self.request_id;
        cx.spawn_in(window, async move |panel, cx| {
            let result = kobo_device::wifi::request(request).await;
            let _ = panel.update_in(cx, |panel, window, cx| {
                if reading {
                    panel.reading = false;
                }
                // A cancelled/replaced action or earlier status read must not
                // clear the newer action's busy state or replace its error.
                if request_id != panel.request_id {
                    return;
                }
                if !reading {
                    panel.operation = None;
                }
                let previous_error = panel.error.clone();
                // The subscription owns snapshots: replies can arrive after
                // a newer device event has already been observed.
                let error = match &result {
                    Err(error) => Some(error),
                    // Identical failures do not produce a subscription event.
                    Ok(snapshot) if snapshot.error == panel.snapshot.error => snapshot.error.as_ref(),
                    _ => None,
                };
                if let Some(error) = error {
                    panel.error = Some(components::wifi_error_message(error).to_owned());
                    if panel.open && panel.selected.is_some() {
                        panel.password.update(cx, |input, cx| input.focus(window, cx));
                    }
                }
                if !reading || previous_error != panel.error {
                    cx.notify();
                }
                if powering_on && panel.open && panel.snapshot.phase != Phase::Off && panel.error.is_none() && !panel.snapshot.scanning {
                    panel.run(Request::Scan, window, cx);
                }
            });
        })
        .detach();
    }

    fn render_network(&self, row: NetworkRow, cx: &mut Context<Self>) -> components::Button {
        let theme = components::browser_theme(cx);
        let connecting = self.connecting_network() == Some(row.name.as_str());
        let connected = row.connected && self.connecting_network().is_none();
        let security = row.network.as_ref().map(|network| network.security);
        let disabled = self.operation == Some(Operation::Power) || connecting || (!connected && row.saved_id.is_none() && security.is_none_or(|security| security == Security::Unsupported));
        let mut status = div().flex().flex_none().items_center().gap(px(6.0)).text_size(px(13.0)).text_color(theme.text_muted);
        if connecting {
            status = status.child(text!(device::connecting));
        } else if connected {
            status = status.child(text!(device::network_connected)).child(Icon::empty().path("icons/check.svg").size(px(16.0)));
        } else {
            if security == Some(Security::WpaPersonal) || (security.is_none() && row.saved_id.is_some()) {
                status = status.child(Icon::empty().path("icons/lock.svg").size(px(16.0)));
            }
            status = status.child(Icon::empty().path("icons/chevron-right.svg").size(px(16.0)));
        }
        let name = row.name.clone();
        components::base_button(format!("wifi-network-{name}-{security:?}"))
            .aria_label(if connecting {
                format!("{name} · {}", text!(device::connecting))
            } else if connected {
                format!(text!(device::connected), name)
            } else {
                name
            })
            .ghost()
            .no_fill()
            .min_h(px(52.0))
            .h_auto()
            .w_full()
            .px_0()
            .py(px(8.0))
            .gap(px(11.0))
            .border_b_1()
            .border_color(theme.rule)
            .justify_start()
            .text_size(px(16.0))
            .text_align(TextAlign::Left)
            .child(Icon::empty().path("icons/wifi.svg").size(px(18.0)))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .child(div().truncate().when(connected, |label| label.font_weight(FontWeight::BOLD)).child(row.name.clone()))
                    .when(security == Some(Security::Open) && !connected && !connecting, |label| label.child(div().mt(px(2.0)).text_size(px(13.0)).text_color(theme.text_muted).child(text!(device::open_network)))),
            )
            .child(status)
            .disabled(disabled)
            .on_click(cx.listener(move |panel, _, window, cx| panel.select(&row, window, cx)))
    }

    fn render_error(&self, cx: &Context<Self>) -> Div {
        div().border_l_2().border_color(components::browser_theme(cx).text).pl(px(10.0)).text_size(px(14.0)).children(self.error.clone())
    }
}

impl Render for WifiPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let off = self.snapshot.phase == Phase::Off;
        let unavailable = self.unavailable();
        let powering = self.operation == Some(Operation::Power);
        let scanning = self.snapshot.scanning || self.operation == Some(Operation::Scan);
        let target = self.connecting_network().map(str::to_owned);
        let form = self.selected.as_ref().filter(|network| target.as_deref() != Some(network.name().as_str()));
        let header = components::kobo::panel_header(text!(device::wi_fi))
            .min_h(px(52.0))
            .py(px(8.0))
            .pl(px(18.0))
            .pr(px(12.0))
            .gap(px(9.0))
            .border_b_1()
            .border_color(theme.rule)
            .child(
                components::base_button("wifi-power")
                    .ghost()
                    .no_fill()
                    .border_0()
                    .px_0()
                    .min_h(px(36.0))
                    .gap(px(8.0))
                    .aria_label(if off { text!(device::wi_fi_off) } else { text!(device::wi_fi) })
                    .child(div().text_size(px(14.0)).child(if off { text!(common::off) } else { text!(common::on) }))
                    .child(Icon::empty().path(if off { "icons/wifi-toggle-off.svg" } else { "icons/wifi-toggle-on.svg" }).w(px(36.0)).h(px(20.0)))
                    .disabled(powering || !self.snapshot.power_available)
                    .on_click(cx.listener(move |panel, _, window, cx| {
                        panel.clear_entry(window, cx);
                        panel.run(Request::Power(off), window, cx);
                    })),
            )
            .child(
                device_button("wifi-refresh", text!(device::refresh_networks), Icon::empty().path("icons/redo-2.svg")).disabled(powering || off || scanning).on_click(cx.listener(|panel, _, window, cx| panel.run(Request::Scan, window, cx))),
            )
            .child(components::kobo::panel_close());
        let mut body = div().id("wifi-panel-body").flex().flex_col().min_h_0().overflow_y_scroll().px(px(18.0)).pt(px(4.0)).pb(px(18.0)).text_size(px(16.0));
        if unavailable {
            body = body.child(div().mt(px(19.0)).text_color(theme.text_muted).child(text!(device::wi_fi_unavailable))).child(
                div()
                    .mt(px(18.0))
                    .flex()
                    .justify_end()
                    .child(components::base_button("wifi-retry").label(text!(common::retry)).no_fill().min_h(px(40.0)).disabled(self.reading).on_click(cx.listener(|panel, _, window, cx| panel.run(Request::Status, window, cx)))),
            );
        } else if let Some(network) = form {
            body = body
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(11.0))
                        .min_h(px(52.0))
                        .py(px(8.0))
                        .border_b_1()
                        .border_color(theme.rule)
                        .child(Icon::empty().path("icons/wifi.svg").size(px(18.0)))
                        .child(div().flex_1().min_w_0().truncate().child(network.name()))
                        .child(Icon::empty().path("icons/lock.svg").size(px(16.0))),
                )
                .child(div().mt(px(18.0)).mb(px(6.0)).text_size(px(14.0)).child(text!(common::password)))
                .child(div().when(self.error.is_some(), |field| field.border_2().border_color(theme.text)).child(Input::new(&self.password).h(px(42.0)).mask_toggle().content_type(InputContentType::Password)))
                .when(self.error.is_some(), |body| body.child(div().mt(px(10.0)).child(self.render_error(cx))))
                .child(
                    div()
                        .mt(px(18.0))
                        .flex()
                        .items_center()
                        .justify_end()
                        .gap(px(10.0))
                        .child(components::base_button("wifi-back").label(text!(common::back)).ghost().no_fill().min_h(px(40.0)).on_click(cx.listener(|panel, _, window, cx| {
                            panel.clear_entry(window, cx);
                            panel.error = None;
                            cx.notify();
                        })))
                        .child(
                            components::base_button("wifi-join")
                                .label(text!(device::connect))
                                .primary()
                                .no_fill()
                                .min_h(px(40.0))
                                .border_2()
                                .border_color(theme.text)
                                .font_weight(FontWeight::BOLD)
                                .disabled(powering || !self.password_valid)
                                .on_click(cx.listener(|panel, _, window, cx| panel.submit(window, cx))),
                        ),
                );
        } else if !off {
            if self.error.is_some() {
                body = body.child(div().my(px(10.0)).child(self.render_error(cx)));
            }
            let mut rows = network_rows(&self.snapshot);
            if let Some(name) = &target {
                if let Some(index) = rows.iter().position(|row| &row.name == name) {
                    let row = rows.remove(index);
                    rows.insert(0, row);
                } else {
                    rows.insert(0, NetworkRow { name: name.clone(), network: None, saved_id: None, connected: false });
                }
            }
            let has_primary = target.is_some() || rows.first().is_some_and(|row| row.connected);
            if rows.is_empty() && !scanning {
                body = body.child(div().mt(px(19.0)).text_color(theme.text_muted).child(text!(device::refresh_to_find_networks)));
            } else {
                for (index, row) in rows.into_iter().enumerate() {
                    if index == usize::from(has_primary) {
                        body = body.child(div().pt(px(17.0)).pb(px(3.0)).text_size(px(12.0)).text_color(theme.text_muted).child(text!(device::networks).to_uppercase()));
                    }
                    body = body.child(self.render_network(row, cx));
                }
            }
            body = body.when(target.is_some() || scanning, |body| {
                body.child(
                    div()
                        .mt(px(12.0))
                        .min_h(px(28.0))
                        .flex()
                        .items_center()
                        .justify_end()
                        .text_size(px(14.0))
                        .text_color(theme.text_muted)
                        .when(target.is_some(), |status| {
                            status.child(components::base_button("wifi-cancel-connect").label(text!(common::cancel)).ghost().no_fill().min_h(px(28.0)).on_click(cx.listener(|panel, _, window, cx| {
                                panel.clear_entry(window, cx);
                                panel.run(Request::Disconnect, window, cx);
                            })))
                        })
                        .when(scanning && target.is_none(), |status| status.child(text!(device::scanning))),
                )
            });
        } else if self.error.is_some() {
            body = body.child(div().mt(px(10.0)).child(self.render_error(cx)));
        } else {
            body = body.py_0();
        }
        components::kobo::panel(window, cx).p_0().gap_0().track_focus(&self.focus).child(header).child(body)
    }
}

fn visible_phase(phase: Phase) -> Phase {
    if phase == Phase::ObtainingAddress { Phase::Connecting } else { phase }
}

struct NetworkRow {
    name: String,
    network: Option<Network>,
    saved_id: Option<u32>,
    connected: bool,
}

impl NetworkRow {
    fn visible(&self) -> (&str, Option<Security>, Option<u32>, bool) {
        (&self.name, self.network.as_ref().map(|network| network.security), self.saved_id, self.connected)
    }
}

fn network_rows(snapshot: &Snapshot) -> Vec<NetworkRow> {
    let mut rows: Vec<NetworkRow> = Vec::new();
    for network in &snapshot.networks {
        if let Some(row) = rows.iter_mut().find(|row| row.network.as_ref().is_some_and(|other| other.ssid == network.ssid && other.security == network.security)) {
            if row.network.as_ref().is_some_and(|other| other.signal < network.signal) {
                row.network = Some(network.clone());
            }
        } else {
            rows.push(NetworkRow { name: network.name(), network: Some(network.clone()), saved_id: None, connected: false });
        }
    }
    for saved in &snapshot.saved {
        if let Some(row) = rows.iter_mut().find(|row| row.name == saved.name && row.saved_id.is_none()) {
            row.saved_id = Some(saved.id);
        }
    }
    if snapshot.phase == Phase::Connected
        && let Some(name) = &snapshot.network
    {
        if let Some(row) = rows.iter_mut().find(|row| &row.name == name) {
            row.connected = true;
        } else {
            rows.push(NetworkRow { name: name.clone(), network: None, saved_id: None, connected: true });
        }
    }
    rows.sort_by(|a, b| b.connected.cmp(&a.connected).then_with(|| b.network.as_ref().map(|n| n.signal).cmp(&a.network.as_ref().map(|n| n.signal))).then_with(|| a.name.cmp(&b.name)));
    rows
}
