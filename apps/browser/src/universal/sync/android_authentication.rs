use ui_components::user_facing_error;

use super::SyncPage;
use crate::android_authentication::{Request, Session};
use gpui::Context;

pub(super) struct AndroidAuthentication {
    _session: Session,
    _task: gpui::Task<()>,
}
impl SyncPage {
    pub(super) fn open_android_authentication(&mut self, cx: &mut Context<Self>) {
        self.android_authentication = None;
        let theme = ui_components::browser_theme(cx);
        let colors = [theme.page_bg, theme.text, theme.text_muted, theme.accent, theme.accent_text, theme.border, theme.text_accent, theme.danger, theme.accent_wash]
            .into_iter()
            .map(|color| {
                let c = color.to_rgb();
                0xff000000 | ((c.r * 255.0).round() as u32) << 16 | ((c.g * 255.0).round() as u32) << 8 | (c.b * 255.0).round() as u32
            })
            .collect();
        let (session, requests) = match Session::open(colors) {
            Ok(value) => value,
            Err(error) => {
                self.error = Some(user_facing_error(&error, "Could not open the sign-in form. Please try again.").into());
                cx.notify();
                return;
            }
        };
        let id = session.0;
        let backend = self.services.backend.clone();
        let task = cx.spawn(async move |page, cx| {
            while let Ok(Request::Submit(request)) = requests.recv().await {
                let signs_in = request.signs_in();
                let result = request.submit(&backend).await;
                let signed_in = result.is_ok() && signs_in;
                Session::complete(id, result.err());
                if signed_in {
                    let _ = page.update(cx, |page, cx| page.schedule_refresh(cx));
                }
            }
        });
        self.android_authentication = Some(AndroidAuthentication { _session: session, _task: task });
    }
}
