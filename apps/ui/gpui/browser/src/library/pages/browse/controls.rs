use std::cell::Cell;
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{Context, Entity, IntoElement, Subscription, Window, px};
use gpui_component::input::{InputEvent, InputState};
use gpui_component::menu::{DropdownMenu as _, PopupMenuItem};
use ui_components as components;

/// A query input that reports changes without owning or transforming content.
pub(crate) struct SearchField {
    input: Entity<InputState>,
    suppress_change: Rc<Cell<bool>>,
}

impl SearchField {
    pub(crate) fn new<P: 'static>(placeholder: Option<&'static str>, on_changed: impl Fn(&mut P, &mut Context<P>) + 'static, window: &mut Window, cx: &mut Context<P>) -> (Self, Subscription) {
        let input = cx.new(|cx| {
            let input = InputState::new(window, cx);
            match placeholder {
                Some(placeholder) => input.placeholder(placeholder),
                None => input,
            }
        });
        let suppress_change = Rc::new(Cell::new(false));
        let subscription_suppression = suppress_change.clone();
        let subscription = cx.subscribe(&input, move |page: &mut P, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) && !subscription_suppression.get() {
                on_changed(page, cx);
                cx.notify();
            }
        });
        (Self { input, suppress_change }, subscription)
    }

    pub(crate) fn query(&self, cx: &gpui::App) -> String {
        self.input.read(cx).value().to_string()
    }

    pub(crate) fn clear<P: 'static>(&self, window: &mut Window, cx: &mut Context<P>) {
        self.suppress_change.set(true);
        self.input.update(cx, |input, cx| input.set_value("", window, cx));
        self.suppress_change.set(false);
    }

    pub(crate) fn render(&self) -> gpui::Div {
        components::browser_topbar_center().child(components::browser_search_box().child(components::browser_search_input(&self.input)))
    }
}

/// A value-only file-kind control. Its owner decides how content is rebuilt.
pub(crate) fn file_type_filter_control<P, Select>(id: &'static str, selected: epub_svelte_lib::library::LibraryFileTypeFilter, page: Entity<P>, select: Select, theme: components::BrowserTheme) -> impl IntoElement
where
    P: 'static,
    Select: Fn(&mut P, epub_svelte_lib::library::LibraryFileTypeFilter, &mut Context<P>) + Clone + 'static,
{
    components::catalog_dropdown_button(id, selected.label(), false, false, theme).h(px(components::TOPBAR_ACTION_HEIGHT)).dropdown_caret(true).dropdown_menu(move |menu, _, _| {
        epub_svelte_lib::library::LibraryFileTypeFilter::ALL.into_iter().fold(menu, |menu, file_type| {
            let page = page.clone();
            let select = select.clone();
            menu.item(PopupMenuItem::new(file_type.label()).checked(file_type == selected).on_click(move |_, _, cx| {
                page.update(cx, |page, cx| select(page, file_type, cx));
            }))
        })
    })
}
