//! Browser integration fixture for the production GPUI platform and UI controls.
#![cfg(target_family = "wasm")]

use gpui::prelude::*;
use gpui::{AppContext, Context, Entity, Render, Subscription, TouchEvent, TouchId, TouchPhase, Window, WindowOptions, div, px};
use gpui_component::Root;
use gpui_component::input::{Input, InputEvent, InputState};
use std::cell::OnceCell;
use ui_components as components;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(inline_js = r#"
export function openProbeDialog() {
    const dialog = document.createElement('dialog');
    dialog.id = 'probe-dom-dialog';
    dialog.innerHTML = '<input id="probe-dom-input" autocomplete="off" aria-label="Native DOM field">';
    document.body.append(dialog);
    dialog.showModal();
    dialog.querySelector('input').focus();
}
"#)]
extern "C" {
    #[wasm_bindgen(js_name = openProbeDialog)]
    fn open_probe_dialog();
}

thread_local! {
    static APPLICATION: OnceCell<gpui::ApplicationHandle> = const { OnceCell::new() };
}

struct Probe {
    input: Entity<InputState>,
    taps: usize,
    scrolls: usize,
    holds: usize,
    pinches: usize,
    touches: usize,
    scrub_contact: Option<TouchId>,
    scrub: Option<f32>,
    seeks: usize,
    last_seek: Option<f32>,
    _subscription: Subscription,
}

impl Probe {
    fn publish(&self, window: &Window, cx: &Context<Self>) {
        let insets = window.insets();
        let state = serde_json::json!({
            "touch": components::touch_ui(),
            "platformTouch": cx.supports_touch_input(),
            "compact": components::WindowWidthClass::for_window(window).is_compact(),
            "readerTouch": components::uses_mobile_reader_controls(window),
            "nativeBack": components::has_native_back_button(),
            "controlHeight": components::topbar_action_height_rem() * f32::from(window.rem_size()),
            "width": f32::from(window.viewport_size().width),
            "height": f32::from(window.viewport_size().height),
            "keyboardBottom": f32::from(insets.ime.bottom),
            "safeBottom": f32::from(insets.safe_area.bottom),
            "text": self.input.read(cx).value().as_str(),
            "taps": self.taps, "scrolls": self.scrolls, "holds": self.holds,
            "pinches": self.pinches, "touches": self.touches,
            "scrub": self.scrub, "seeks": self.seeks, "lastSeek": self.last_seek,
        });
        js_sys::Reflect::set(&js_sys::global(), &"__BOKHEIM_MOBILE_PROBE__".into(), &state.to_string().into()).unwrap();
    }
}

impl Render for Probe {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.publish(window, cx);
        let theme = components::browser_theme(cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .p(px(16.0))
            .gap(px(16.0))
            .bg(theme.page_bg)
            .child(components::outlined_text_button("probe-button", "Tap").w(px(120.0)).h(px(48.0)).on_click(cx.listener(|view, _, _, cx| {
                view.taps += 1;
                cx.notify();
            })))
            .child(Input::new(&self.input).w(px(280.0)).h(px(48.0)))
            .child(
                div()
                    .id("probe-surface")
                    .relative()
                    .w_full()
                    .h(px(240.0))
                    .bg(theme.accent_wash)
                    .capture_any_touch(cx.listener(|view, _, _, cx| {
                        view.touches += 1;
                        cx.notify();
                    }))
                    .on_scroll_wheel(cx.listener(|view, _, _, cx| {
                        view.scrolls += 1;
                        cx.notify();
                    }))
                    .on_long_press(cx.listener(|view, _, _, cx| {
                        view.holds += 1;
                        cx.notify();
                    }))
                    .on_pinch(cx.listener(|view, _, _, cx| {
                        view.pinches += 1;
                        cx.notify();
                    }))
                    .child("Touch surface")
                    .child(components::now_playing_scrub_track(0.5, theme).absolute().left_0().top(px(150.0)).child(components::playback_scrub_touch_listener(cx.listener(|view, event: &TouchEvent, window, cx| {
                        if event.phase == TouchPhase::Started && view.scrub_contact.is_none() {
                            view.scrub_contact = Some(event.id);
                        }
                        if view.scrub_contact != Some(event.id) {
                            return;
                        }
                        window.prevent_default();
                        cx.stop_propagation();
                        match event.phase {
                            TouchPhase::Started | TouchPhase::Moved => view.scrub = Some(f32::from(event.position.x)),
                            TouchPhase::Ended | TouchPhase::Cancelled => {
                                if event.phase == TouchPhase::Ended {
                                    view.seeks += 1;
                                    view.last_seek = Some(f32::from(event.position.x));
                                }
                                view.scrub = None;
                                view.scrub_contact = None;
                            }
                        }
                        cx.notify();
                    })))),
            )
            .child(components::outlined_text_button("probe-close", "Blur").w(px(120.0)).h(px(48.0)))
            .child(components::outlined_text_button("probe-dom", "DOM field").w(px(120.0)).h(px(48.0)).on_click(|_, _, _| open_probe_dialog()))
    }
}

#[wasm_bindgen(start)]
pub fn start() {
    gpui_platform::web_init();
    let application = gpui_platform::single_threaded_web().with_assets(components::Assets::default());
    let handle = application.run_embedded(|cx| {
        gpui_component::init(cx);
        components::register_fonts(cx).unwrap();
        components::apply_appearance(&Default::default(), cx);
        cx.open_window(WindowOptions::default(), |window, cx| {
            let view = cx.new(|cx| {
                let input = cx.new(|cx| InputState::new(window, cx));
                let subscription = cx.subscribe(&input, |_: &mut Probe, _, _: &InputEvent, cx| cx.notify());
                Probe { input, taps: 0, scrolls: 0, holds: 0, pinches: 0, touches: 0, scrub_contact: None, scrub: None, seeks: 0, last_seek: None, _subscription: subscription }
            });
            cx.new(|cx| Root::new(view, window, cx))
        })
        .unwrap();
        cx.activate(true);
    });
    APPLICATION.with(|application| {
        application.set(handle).ok();
    });
}
