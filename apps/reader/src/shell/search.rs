//! Search controls shared by every reader surface.

use gpui::prelude::*;
use gpui::{AnyElement, Context, Entity, Window, div, px};
use gpui_component::input::{Input, InputState};
use gpui_component::{Disableable, IconName};
use ui_components as components;

/// The two operations driven by the common search controls.
pub(crate) trait SearchBarTarget: Sized + 'static {
    fn search_navigate(&mut self, direction: i8, cx: &mut Context<Self>);
    fn search_close(&mut self, window: &mut Window, cx: &mut Context<Self>);
}

/// Both rows on compact screens retain the reader's control size and inset.
pub(crate) fn bar_height(window: &Window) -> f32 {
    if components::WindowWidthClass::for_window(window).is_compact() { 2.0 * components::READER_TOOLBAR_CONTROL_HEIGHT + 3.0 * components::CONTENT_INSET } else { components::READER_TOOLBAR_HEIGHT }
}

/// The input stays wide on desktop and gets its own row on compact screens.
pub(crate) fn search_bar<C: SearchBarTarget>(target: &Entity<C>, input: &Entity<InputState>, position: (usize, usize), window: &Window, theme: components::BrowserTheme) -> AnyElement {
    let previous_target = target.clone();
    let next_target = target.clone();
    let close_target = target.clone();
    let (current, total) = position;
    let compact = components::WindowWidthClass::for_window(window).is_compact();
    let query = components::reader_search_box(theme).when(compact, |field| field.flex_none().w_full()).key_context("ReaderSearch").child(Input::new(input).appearance(false).cleanable(true).size_full());
    let controls = div()
        .flex_none()
        .flex()
        .items_center()
        .gap(px(components::SPACE_SM))
        .when(compact, |row| row.w_full())
        .child(components::reader_match_count(format!("{current} of {total}"), theme).when(compact, |count| count.flex_1().text_left()))
        .child(components::reader_search_icon_button("reader-search-previous", "Previous match", IconName::ChevronUp, theme).disabled(total == 0).tooltip("Previous match (Shift+Enter)").on_click(move |_, _, cx| {
            previous_target.update(cx, |this, cx| this.search_navigate(-1, cx));
        }))
        .child(components::reader_search_icon_button("reader-search-next", "Next match", IconName::ChevronDown, theme).disabled(total == 0).tooltip("Next match (Enter)").on_click(move |_, _, cx| {
            next_target.update(cx, |this, cx| this.search_navigate(1, cx));
        }))
        .child(components::reader_search_icon_button("reader-search-close", "Close search", IconName::Close, theme).tooltip("Close search (Esc)").on_click(move |_, window, cx| {
            close_target.update(cx, |this, cx| this.search_close(window, cx));
        }));
    components::reader_search_bar(theme).h(px(bar_height(window))).when(compact, |bar| bar.flex_col().items_stretch().py(px(components::CONTENT_INSET))).child(query).child(controls).into_any_element()
}
