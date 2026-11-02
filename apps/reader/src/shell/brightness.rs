use ui_text::text;

use gpui::prelude::*;
use gpui::{App, Context, Entity, FocusHandle, IntoElement, Pixels, Render, Window};
use gpui_component::{
    Disableable, IconName,
    popover::Popover,
    slider::{Slider, SliderEvent, SliderState},
};
use ui_components as components;

#[derive(Clone, Copy)]
enum LightKind {
    Brightness,
    Natural,
}
impl LightKind {
    fn label(self) -> &'static str {
        match self {
            Self::Brightness => text!(common::brightness),
            Self::Natural => text!(reader::natural_light),
        }
    }
}

/// Reopening reads both drivers again, including after wake.
pub struct BrightnessControls {
    brightness: Entity<LightControl>,
    natural: Entity<LightControl>,
}
impl BrightnessControls {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let brightness = cx.new(|cx| LightControl::new(LightKind::Brightness, window, cx));
        let natural = cx.new(|cx| LightControl::new(LightKind::Natural, window, cx));
        // Read after the entity has been installed in the window. Starting the
        // request inside another entity constructor can race the first Kobo
        // frame and leave the slider at its default value until the popover is
        // opened.
        cx.spawn_in(window, async move |panel, cx| {
            let _ = panel.update_in(cx, |panel, window, cx| panel.refresh(window, cx));
        })
        .detach();
        Self { brightness, natural }
    }
    fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        #[cfg(feature = "kobo")]
        if !components::kobo::has_frontlight(cx) {
            return;
        }
        self.brightness.update(cx, |control, cx| control.request(None, window, cx));
        #[cfg(feature = "kobo")]
        if !components::kobo::has_warmth(cx) {
            return;
        }
        self.natural.update(cx, |control, cx| control.request(None, window, cx));
    }
}
impl Render for BrightnessControls {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let panel = components::reader_settings_dropdown(window, components::browser_theme(cx)).child(self.brightness.clone());
        #[cfg(feature = "kobo")]
        let show_warmth = components::kobo::has_warmth(cx);
        #[cfg(not(feature = "kobo"))]
        let show_warmth = true;
        panel.when(show_warmth, |panel| panel.child(self.natural.clone()))
    }
}

struct LightControl {
    kind: LightKind,
    unsupported: bool,
    percent: Option<u8>,
    slider: Entity<SliderState>,
    busy: bool,
    error: Option<String>,
}

impl LightControl {
    fn new(kind: LightKind, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let slider = cx.new(|_| SliderState::new().min(0.0).max(100.0).step(if matches!(kind, LightKind::Natural) { 10.0 } else { 1.0 }));
        cx.subscribe_in(&slider, window, |panel, _, event, window, cx| match event {
            SliderEvent::Change(_) => cx.notify(),
            SliderEvent::Release(value) => {
                // Commit once per gesture so dragging does not queue driver writes.
                panel.request(Some(value.start().round().clamp(0.0, 100.0) as u8), window, cx);
            }
        })
        .detach();
        Self { kind, unsupported: false, percent: None, slider, busy: false, error: None }
    }

    fn request(&mut self, percent: Option<u8>, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.error = None;
        cx.notify();
        let kind = self.kind;
        cx.spawn_in(window, async move |panel, cx| {
            let result = match (kind, percent) {
                (LightKind::Brightness, Some(percent)) => kobo_device::control::set_brightness(percent).await.map(Some),
                (LightKind::Brightness, None) => kobo_device::control::brightness().await.map(Some),
                (LightKind::Natural, Some(percent)) => kobo_device::control::set_natural_light(percent).await.map(Some),
                (LightKind::Natural, None) => kobo_device::control::natural_light().await,
            };
            let _ = panel.update_in(cx, |panel, window, cx| {
                panel.busy = false;
                match result {
                    Ok(Some(percent)) => {
                        panel.unsupported = false;
                        panel.percent = Some(percent);
                        panel.slider.update(cx, |slider, cx| slider.set_value(f32::from(percent), window, cx));
                    }
                    Ok(None) => {
                        panel.percent = None;
                        panel.unsupported = true;
                    }
                    Err(error) => {
                        panel.percent = None;
                        log::warn!("Device control failed: {error}");
                        panel.error = Some(text!(reader::couldn_t_adjust_the_light_retry_or_restart_the_app).into());
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for LightControl {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let disabled = self.busy || self.percent.is_none();
        let label = if self.percent.is_some() {
            format!("{:.0}%", self.slider.read(cx).value().start())
        } else if self.busy {
            text!(common::loading).to_owned()
        } else {
            text!(common::unavailable).to_owned()
        };
        let mut panel = components::reader_settings_section(self.kind.label(), theme).child(
            gpui::div()
                .flex()
                .items_center()
                .gap(gpui::px(components::SPACE_MD))
                .child(gpui::div().flex_1().min_w_0().py(gpui::px(12.0)).child(Slider::new(&self.slider).disabled(disabled)))
                .child(gpui::div().w(gpui::px(48.0)).text_align(gpui::TextAlign::Right).child(label)),
        );
        if self.unsupported {
            panel = panel.child(gpui::div().text_color(theme.text_muted).child(text!(reader::not_supported_on_this_kobo)));
        } else if matches!(self.kind, LightKind::Natural) {
            panel = panel.child(gpui::div().flex().justify_between().text_color(theme.text_muted).child(text!(reader::cooler)).child(text!(reader::warmer)));
        }
        if let Some(error) = self.error.clone() {
            panel = panel
                .child(gpui::div().text_color(theme.text_muted).child(error))
                .child(components::topbar_action_button("brightness-retry", text!(common::retry), IconName::Redo2, theme).disabled(self.busy).on_click(cx.listener(|panel, _, window, cx| panel.request(None, window, cx))));
        }
        panel
    }
}

pub fn popover(panel: Entity<BrightnessControls>, document_focus: Option<&FocusHandle>, window: &Window) -> Popover {
    popover_with_height(panel, document_focus, None, window)
}

pub fn popover_with_height(panel: Entity<BrightnessControls>, document_focus: Option<&FocusHandle>, height: Option<Pixels>, window: &Window) -> Popover {
    let refresh = panel.clone();
    let mut popover = Popover::new("reader-brightness")
        .anchor(gpui::Anchor::TopRight)
        .top(gpui::px(components::reader_toolbar_size_px(window) + 4.0))
        .p_0()
        .when_some(document_focus, |popover, focus| popover.track_focus(focus))
        .trigger(components::outlined_icon_button("reader-brightness-button", text!(common::brightness), IconName::Sun))
        .on_open_change(move |open, window, cx: &mut App| {
            if *open {
                refresh.update(cx, |panel, cx| panel.refresh(window, cx));
            }
        });
    popover = if let Some(height) = height { popover.child(gpui::div().h(height).child(panel)) } else { popover.child(panel) };
    popover
}
