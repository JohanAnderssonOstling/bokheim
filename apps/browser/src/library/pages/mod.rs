//! Pages backed by the currently selected library.

use gpui::{AnyElement, App, ClickEvent, ElementId, IntoElement, SharedString, Window};
use gpui_component::IconName;

use ui_components::BrowserTheme;

pub(crate) mod browse;
pub(crate) mod trash;

/// Keeps existing page content visible alongside a failed request and its retry.
pub(crate) fn with_retry_banner(body: AnyElement, error: Option<SharedString>, retry_id: impl Into<ElementId>, theme: BrowserTheme, on_retry: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> AnyElement {
    if let Some(error) = error {
        let retry = ui_components::topbar_action_button(retry_id, "Retry", IconName::Redo2, theme).on_click(on_retry);
        ui_components::browser_error_banner(error, retry, body).into_any_element()
    } else {
        body
    }
}
