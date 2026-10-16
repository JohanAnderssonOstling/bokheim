//! Search controls shared by every reader surface.

use gpui::prelude::*;
use gpui::{AnyElement, App, Context, Div, ElementId, Entity, Focusable, FontWeight, Stateful, Window, div, px};
use gpui_component::input::{Input, InputState};
use gpui_component::{Disableable, IconName};
use ui_components as components;

/// Match navigation driven by the common search controls.
pub(crate) trait SearchBarTarget: Sized + 'static {
    fn search_navigate(&mut self, direction: i8, cx: &mut Context<Self>);
    fn search_position(&self) -> (usize, usize) {
        (0, 0)
    }
}

/// Match count and navigation sit in the panel header, before its close button.
pub(crate) fn header_controls<C: SearchBarTarget>(owner: &C, target: &Entity<C>, theme: components::BrowserTheme) -> AnyElement {
    let previous_target = target.clone();
    let next_target = target.clone();
    let (current, total) = owner.search_position();
    let controls = div()
        .min_w_0()
        .flex_none()
        .debug_selector(|| "reader-search-controls".into())
        .flex()
        .items_center()
        .gap(px(components::SPACE_XXS))
        .child(components::reader_match_count(format!("{current} of {total}"), theme))
        .child(components::outlined_icon_button("reader-search-previous", "Previous match", IconName::ChevronLeft, theme).disabled(total == 0).tooltip("Previous match (Shift+Enter)").on_click(move |_, _, cx| {
            previous_target.update(cx, |this, cx| this.search_navigate(-1, cx));
        }))
        .child(components::outlined_icon_button("reader-search-next", "Next match", IconName::ChevronRight, theme).disabled(total == 0).tooltip("Next match (Enter)").on_click(move |_, _, cx| {
            next_target.update(cx, |this, cx| this.search_navigate(1, cx));
        }));
    controls.into_any_element()
}

/// The query occupies its own full-width row below the panel header.
pub(crate) fn search_bar(input: &Entity<InputState>, theme: components::BrowserTheme, window: &mut Window, cx: &mut App) -> AnyElement {
    let focus = input.read(cx).focus_handle(cx);
    // This state lives only while the query is rendered, so reopening the
    // panel focuses it again without stealing focus on subsequent renders.
    window.use_keyed_state(("reader-search-autofocus", input.entity_id()), cx, move |window, cx| focus.focus(window, cx));
    let query = components::reader_search_box(theme).debug_selector(|| "reader-search-query".into()).key_context("ReaderSearch").child(Input::new(input).appearance(false).cleanable(false).size_full().font_weight(FontWeight::BOLD));
    components::reader_search_bar(theme).debug_selector(|| "reader-search-row".into()).child(query).into_any_element()
}

/// The query stays visible while the excerpts scroll below it.
pub(crate) fn panel(bar: AnyElement, rows: Vec<AnyElement>, theme: components::BrowserTheme) -> AnyElement {
    div().size_full().min_h_0().min_w_0().flex().flex_col().child(bar).child(components::reader_state_panel(theme).children(rows)).into_any_element()
}

pub(crate) fn result_row(id: impl Into<ElementId>, excerpt: String, selected: bool, theme: components::BrowserTheme) -> Stateful<Div> {
    components::reader_state_card(id, selected, theme)
        .flex_none()
        .cursor_pointer()
        .child(div().w_full().min_w_0().whitespace_normal().line_clamp(3).text_ellipsis().text_size(gpui::rems(components::TEXT_SM)).line_height(gpui::relative(1.35)).text_color(theme.text).child(excerpt))
}
