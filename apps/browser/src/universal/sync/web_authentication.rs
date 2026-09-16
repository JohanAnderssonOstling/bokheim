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
}

pub(super) struct WebAuthentication {
    panel: JsValue,
    _submit: Closure<dyn FnMut(String, String, String, String) -> js_sys::Promise>,
    _updates: gpui::Task<()>,
}
impl Drop for WebAuthentication {
    fn drop(&mut self) {
        close_authentication(&self.panel);
    }
}

impl SyncPage {
    pub(super) fn open_web_authentication(&mut self, cx: &mut Context<Self>) {
        let backend = self.services.backend.clone();
        let (updated, updates) = async_channel::unbounded();
        let submit = Closure::new(move |action: String, email: String, secret: String, token: String| {
            let backend = backend.clone();
            let updated = updated.clone();
            wasm_bindgen_futures::future_to_promise(async move {
                let result = match action.as_str() {
                    "login" => backend.login(email, secret).await.map(|_| ()),
                    "register" => backend.request_public_registration(email, secret).await,
                    "verify" => backend.verify_email(email, secret).await.map(|_| ()),
                    "resend" => backend.resend_verification(email).await,
                    "request-reset" => backend.request_password_reset(email).await,
                    "reset" => backend.reset_password(token, secret).await,
                    _ => Err("Unknown authentication action".into()),
                };
                result.map_err(|error| JsValue::from_str(&error))?;
                if matches!(action.as_str(), "login" | "verify") {
                    let _ = updated.send(()).await;
                }
                Ok(JsValue::UNDEFINED)
            })
        });
        let theme = ui_components::browser_theme(cx);
        let css = |color: gpui::Hsla| {
            let c = color.to_rgb();
            format!("rgba({},{},{},{})", (c.r * 255.0).round(), (c.g * 255.0).round(), (c.b * 255.0).round(), c.a)
        };
        let theme = serde_json::json!({"background": css(theme.page_bg), "text": css(theme.text), "muted": css(theme.text_muted), "border": css(theme.border), "accent": css(theme.accent), "accentText": css(theme.accent_text)});
        // Dispose an earlier panel before opening another modal; never leave
        // duplicate credential forms for password managers to discover.
        self.web_authentication = None;
        match open_authentication(&theme.to_string(), account_contract::PASSWORD_MIN_CHARACTERS as u32, submit.as_ref().unchecked_ref()) {
            Ok(panel) => {
                let task = cx.spawn(async move |page, cx| {
                    while updates.recv().await.is_ok() {
                        if page.update(cx, |page, cx| page.schedule_refresh(cx)).is_err() {
                            break;
                        }
                    }
                });
                self.web_authentication = Some(WebAuthentication { panel, _submit: submit, _updates: task });
            }
            Err(_) => {
                self.error = Some("Could not open the sign-in form".into());
                cx.notify();
            }
        }
    }
}
