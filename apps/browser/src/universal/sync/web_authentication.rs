//! The browser owns the visible authentication form so autofill and keyboard
//! focus operate on real fields. Credentials go straight to the existing client.
use super::SyncPage;
use gpui::Context;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(module = "/web/authentication.js")]
extern "C" {
    #[wasm_bindgen(js_name = openAuthentication, catch)]
    fn open_authentication(theme: &str, minimum: u32, submit: &js_sys::Function) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_name = closeAuthentication)]
    fn close_authentication(panel: &JsValue);
    #[wasm_bindgen(js_name = registerAuthenticationFont, catch)]
    fn register_authentication_font(family: &str, weight: u16, style: &str, data: &[u8]) -> Result<(), JsValue>;
}

pub(super) struct WebAuthentication {
    panel: JsValue,
    _submit: Closure<dyn FnMut(String, String, String, String) -> js_sys::Promise>,
}
impl Drop for WebAuthentication {
    fn drop(&mut self) {
        close_authentication(&self.panel);
    }
}

impl SyncPage {
    pub(super) fn open_web_authentication(&mut self, cx: &mut Context<Self>) {
        let events = self.events.clone();
        let submit = Closure::new(move |action: String, email: String, secret: String, token: String| {
            let events = events.clone();
            wasm_bindgen_futures::future_to_promise(async move {
                let (reply, result) = async_channel::bounded(1);
                events.try_send(super::SyncEvent::Authenticate { action, email, secret, token, reply }).map_err(|_| JsValue::from_str("The application window is closed"))?;
                result.recv().await.map_err(|_| JsValue::from_str("The application window is closed"))?.map_err(|error| JsValue::from_str(&error))?;
                Ok(JsValue::UNDEFINED)
            })
        });
        let theme = ui_components::browser_theme(cx);
        let family = theme.ui_font_family;
        for face in ui_components::interface_font_faces(family) {
            if register_authentication_font(family.family_name(), face.weight, face.style, face.data).is_err() {
                log::warn!("failed to install sign-in form font");
            }
        }
        let css = |color: gpui::Hsla| {
            let c = color.to_rgb();
            format!("rgba({},{},{},{})", (c.r * 255.0).round(), (c.g * 255.0).round(), (c.b * 255.0).round(), c.a)
        };
        let theme = serde_json::json!({
            "background": css(theme.page_bg), "text": css(theme.text), "muted": css(theme.text_muted), "border": css(theme.border),
            "accent": css(theme.accent), "accentText": css(theme.accent_text), "textAccent": css(theme.text_accent),
            "font-family": format!("\"{}\"", family.family_name()),
            "font-size": format!("{}px", f32::from(gpui_component::Theme::global(cx).font_size)),
        });
        // Dispose an earlier panel before opening another modal; never leave
        // duplicate credential forms for password managers to discover.
        self.web_authentication = None;
        match open_authentication(&theme.to_string(), account_contract::PASSWORD_MIN_CHARACTERS as u32, submit.as_ref().unchecked_ref()) {
            Ok(panel) => {
                self.web_authentication = Some(WebAuthentication { panel, _submit: submit });
            }
            Err(_) => {
                self.error = Some("Could not open the sign-in form".into());
                cx.notify();
            }
        }
    }
}
