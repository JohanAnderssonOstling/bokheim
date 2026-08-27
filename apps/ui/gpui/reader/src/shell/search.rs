//! Search controls shared by every reader surface.

use gpui::prelude::*;
use gpui::{AnyElement, Context, Entity, Window};
use gpui_component::input::{Input, InputState};
use ui_components as components;

/// The two operations driven by the common search controls.
pub(crate) trait SearchBarTarget: Sized + 'static {
    fn search_navigate(&mut self, direction: i8, cx: &mut Context<Self>);
    fn search_close(&mut self, window: &mut Window, cx: &mut Context<Self>);
}

/// Builds the common search input, match position, navigation, and close
/// controls. Format-specific filters are inserted before navigation.
pub(crate) fn search_bar<C: SearchBarTarget>(target: &Entity<C>, input: &Entity<InputState>, position: (usize, usize), filters: Vec<AnyElement>, theme: components::BrowserTheme) -> AnyElement {
    let previous_target = target.clone();
    let next_target = target.clone();
    let close_target = target.clone();
    let (current, total) = position;

    components::reader_search_bar(theme)
        .child(components::reader_search_box(theme).key_context("ReaderSearch").child(Input::new(input).appearance(false).size_full()))
        .child(components::reader_match_count(format!("{current}/{total}"), theme))
        .children(filters)
        .child(components::reader_square_button("reader-search-previous", "‹", theme).tooltip("Previous match (Shift+Enter)").on_click(move |_, _, cx| {
            previous_target.update(cx, |this, cx| this.search_navigate(-1, cx));
        }))
        .child(components::reader_square_button("reader-search-next", "›", theme).tooltip("Next match (Enter)").on_click(move |_, _, cx| {
            next_target.update(cx, |this, cx| this.search_navigate(1, cx));
        }))
        .child(components::reader_square_button("reader-search-close", "×", theme).tooltip("Close search (Esc)").on_click(move |_, window, cx| {
            close_target.update(cx, |this, cx| this.search_close(window, cx));
        }))
        .into_any_element()
}
