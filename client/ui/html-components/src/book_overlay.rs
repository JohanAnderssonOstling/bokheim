//! HTML-to-text descriptions for the generic book overlay.
use gpui::prelude::*;
use gpui::{Div, div, relative, rems};
use ui_components::{BrowserTheme, TEXT_SM};

const DESCRIPTION_LINES: usize = 6;
const DESCRIPTION_LINE_HEIGHT: f32 = 1.5;

/// The first lines of the blurb, stored as HTML, read as plain text and cut
/// with an ellipsis. `None` for a blurb of markup alone.
pub fn book_overlay_description(description: &str, theme: BrowserTheme) -> Option<Div> {
    let text = html_parse::plain_text_from_fragment(description);
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (!text.is_empty()).then(|| div().w_full().min_w_0().whitespace_normal().line_clamp(DESCRIPTION_LINES).text_ellipsis().text_size(rems(TEXT_SM)).line_height(relative(DESCRIPTION_LINE_HEIGHT)).text_color(theme.text).child(text))
}
