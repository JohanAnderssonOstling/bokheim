//! Context actions shared by desktop popup menus and mobile bottom sheets.

use ui_text::text;

use gpui::prelude::*;
use gpui::{AnyElement, Context, Entity, Window, div, px};
use gpui_component::Icon;
use gpui_component::menu::PopupMenu;
use ui_components as components;

pub(crate) struct ContextAction<A> {
    pub(crate) label: &'static str,
    pub(crate) icon: &'static str,
    pub(crate) command: A,
}

pub(crate) struct SelectionCommands<A> {
    pub(crate) annotate: A,
    pub(crate) lookup: Option<A>,
    pub(crate) translate: A,
    pub(crate) copy: A,
    pub(crate) copy_link: A,
}

pub(crate) trait ContextActionTarget: Sized + 'static {
    type Command: Clone + 'static;

    fn run_context_action(&mut self, command: Self::Command, window: &mut Window, cx: &mut Context<Self>);
    fn dismiss_context_actions(&mut self, cx: &mut Context<Self>);
}

pub(crate) struct ContextActions<A> {
    sections: Vec<Vec<ContextAction<A>>>,
}

impl<A> Default for ContextActions<A> {
    fn default() -> Self {
        Self { sections: Vec::new() }
    }
}

impl<A: Clone + 'static> ContextActions<A> {
    pub(crate) fn selection(commands: SelectionCommands<A>) -> Self {
        let mut actions = Self::default();
        let mut selection = vec![ContextAction { label: text!(reader::annotate), icon: "icons/replace.svg", command: commands.annotate }];
        if let Some(command) = commands.lookup {
            selection.push(ContextAction { label: text!(reader::lookup), icon: "icons/search.svg", command });
        }
        selection.extend([
            ContextAction { label: text!(reader::translate), icon: "icons/translate.svg", command: commands.translate },
            ContextAction { label: text!(common::copy), icon: "icons/copy.svg", command: commands.copy },
            ContextAction { label: text!(reader::copy_link), icon: "icons/link-simple.svg", command: commands.copy_link },
        ]);
        actions.add_section(selection);
        actions
    }

    pub(crate) fn add_section(&mut self, actions: Vec<ContextAction<A>>) {
        if !actions.is_empty() {
            self.sections.push(actions);
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.sections.is_empty()
    }

    pub(crate) fn popup<C: ContextActionTarget<Command = A>>(&self, mut menu: PopupMenu, target: &Entity<C>) -> PopupMenu {
        for (index, section) in self.sections.iter().enumerate() {
            if index > 0 {
                menu = menu.separator();
            }
            for action in section {
                let command = action.command.clone();
                let target = target.clone();
                menu = menu.item(components::menu_item_with_icon(action.label, Icon::empty().path(action.icon), move |_, window, cx| {
                    target.update(cx, |reader, cx| reader.run_context_action(command.clone(), window, cx));
                }));
            }
        }
        menu
    }

    pub(crate) fn sheet<C: ContextActionTarget<Command = A>>(&self, theme: components::BrowserTheme, safe_bottom: gpui::Pixels, target: &Entity<C>) -> AnyElement {
        let mut rows = div().id("reader-context-sheet-rows").flex_1().min_h_0().overflow_y_scroll();
        for (section_index, section) in self.sections.iter().enumerate() {
            if section_index > 0 {
                rows = rows.child(components::bottom_sheet_divider(theme));
            }
            for (action_index, action) in section.iter().enumerate() {
                let command = action.command.clone();
                let target = target.clone();
                let label = div().flex().items_center().gap(px(12.0)).child(Icon::empty().path(action.icon).size(px(20.0))).child(div().flex_1().min_w_0().child(action.label));
                rows = rows.child(components::bottom_sheet_row(("reader-context-action", section_index * 10 + action_index), label, false, theme).on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    target.update(cx, |reader, cx| {
                        reader.dismiss_context_actions(cx);
                        reader.run_context_action(command.clone(), window, cx);
                    });
                }));
            }
        }
        let target = target.clone();
        components::bottom_sheet_overlay()
            .child(components::modal_scrim("reader-context-sheet-scrim", theme).on_click(move |_, _, cx| {
                target.update(cx, |reader, cx| reader.dismiss_context_actions(cx));
            }))
            .child(components::bottom_sheet_surface(theme).id("reader-context-sheet").child(rows).pb(safe_bottom))
            .into_any_element()
    }
}
