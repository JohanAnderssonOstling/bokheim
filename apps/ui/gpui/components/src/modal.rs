use gpui::prelude::*;
use gpui::{Div, ElementId, Stateful, div, px};

use crate::{BrowserTheme, RADIUS_MD};

/// The shared full-window layer for a centered modal. The caller supplies the
/// scrim and surface so it can retain its own identity and dismissal behavior.
pub fn modal_overlay() -> Div {
    crate::absolute_full_size().flex().items_center().justify_center().p(px(crate::SPACE_MD))
}

/// A modal's pointer-blocking backdrop.
pub fn modal_scrim(id: impl Into<ElementId>, theme: BrowserTheme) -> Stateful<Div> {
    crate::absolute_full_size().id(id).occlude().bg(theme.overlay_scrim).cursor_default()
}

/// The common elevated surface for dialogs, editors, and other modal content.
pub fn modal_surface(theme: BrowserTheme) -> Div {
    div().relative().min_h_0().min_w_0().occlude().rounded(px(RADIUS_MD)).border_1().border_color(theme.border).bg(theme.surface).shadow(crate::overlay_shadow(theme))
}
