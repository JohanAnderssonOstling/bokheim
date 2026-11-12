//! HTML widgets and font-adapter wiring, above generic UI components.
mod book_overlay;
mod detail;
pub use book_overlay::book_overlay_description;
pub use detail::{book_detail_description, html_fragment};

use gpui::{App, Global};
use html_gpui::backend::{FontLoadRequest, FontLoader};
use std::rc::Rc;
use ui_components::BundledFontRequest;

/// Adapt the application's engine-independent font policy to the HTML adapter.
pub fn bundled_font_loader(cx: &mut App) -> FontLoader {
    let load = ui_components::bundled_font_loader(cx);
    Rc::new(move |text_system, request| {
        let request = match request {
            FontLoadRequest::Family(family) => BundledFontRequest::Family(family),
            FontLoadRequest::TextFallbacks(text) => BundledFontRequest::TextFallbacks(text),
        };
        load(text_system, request)
    })
}

struct FontsConfigured;
impl Global for FontsConfigured {}

/// Install font integration only in applications displaying HTML.
pub fn configure_fonts(cx: &mut App) {
    if cx.try_global::<FontsConfigured>().is_none() {
        let loader = bundled_font_loader(cx);
        html_gpui::backend::configure_font_loader(loader, cx);
        cx.set_global(FontsConfigured);
    }
}
