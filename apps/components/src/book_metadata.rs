//! Presentation shared by book overlays and detail pages.
//! Callers supply plain or selectable text without duplicating its styling.

use gpui::prelude::*;
use gpui::{Div, ElementId, Stateful, div, px, rems};

use crate::{BrowserTheme, SPACE_SM, SPACE_XS, TEXT_LG, TEXT_MD, TEXT_SM, TEXT_XS, TITLE_MD};

pub(crate) fn title(title: impl IntoElement, subtitle: Option<impl IntoElement>, large: bool, theme: BrowserTheme) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(SPACE_XS))
        .child(div().when_some(theme.display_font_family(), |text, family| text.font_family(family)).text_size(rems(if large { TITLE_MD } else { TEXT_LG })).line_height_auto().text_color(theme.text).child(title))
        .children(subtitle.map(|subtitle| div().text_size(rems(TEXT_MD)).text_color(theme.text_muted).child(subtitle)))
}

pub(crate) fn credit_line(theme: BrowserTheme) -> Div {
    div().flex().flex_row().flex_wrap().items_baseline().gap(px(SPACE_XS)).text_size(rems(TEXT_SM)).text_color(theme.text_muted)
}

pub(crate) fn name_link(id: impl Into<ElementId>, name: impl IntoElement, theme: BrowserTheme) -> Stateful<Div> {
    div().id(id).cursor_pointer().text_color(theme.text_accent).hover(|style| style.underline()).child(name)
}

pub(crate) fn facts<F: IntoElement, S: IntoElement>(facts: impl IntoIterator<Item = F>, separator: impl Fn(usize) -> S, theme: BrowserTheme) -> Div {
    let mut row = div().flex().flex_row().flex_wrap().items_baseline().gap(px(SPACE_SM)).text_size(rems(TEXT_XS)).text_color(theme.text_muted);
    for (index, fact) in facts.into_iter().enumerate() {
        if index > 0 {
            row = row.child(div().flex_none().text_color(theme.rule).child(separator(index)));
        }
        row = row.child(div().flex_none().child(fact));
    }
    row
}
