//! Context actions shared by desktop menus, mobile selection toolbars, and object sheets.

use ui_text::text;

use gpui::prelude::*;
use gpui::{AnyElement, Bounds, Context, Entity, Pixels, Window, anchored, deferred, div, point, px, size};
use gpui_component::Icon;
use gpui_component::menu::PopupMenu;
use ui_components as components;

pub(crate) fn selection_toolbar_bounds(selection: Bounds<Pixels>, window: &Window) -> Bounds<Pixels> {
    let viewport = window.viewport_size();
    let insets = window.insets().safe_area;
    let width = (viewport.width - px(16.0)).min(px(360.0)).max(px(1.0));
    let height = px(56.0);
    let min_y = insets.top + px(8.0);
    let max_y = (viewport.height - insets.bottom - height - px(8.0)).max(min_y);
    let above = selection.top() - height - px(12.0);
    let below = selection.bottom() + px(40.0);
    let y = if above >= min_y { above } else { below }.clamp(min_y, max_y);
    let x = (selection.center().x - width / 2.0).clamp(px(8.0), (viewport.width - width - px(8.0)).max(px(8.0)));
    Bounds { origin: point(x, y), size: size(width, height) }
}

pub(crate) struct ContextAction<A> {
    pub(crate) label: &'static str,
    pub(crate) icon: &'static str,
    pub(crate) command: A,
}

pub(crate) struct SelectionCommands<A> {
    pub(crate) annotate: Option<A>,
    pub(crate) lookup: Option<A>,
    pub(crate) translate: A,
    pub(crate) copy: A,
    pub(crate) copy_link: Option<A>,
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
        let mut selection = Vec::new();
        if let Some(command) = commands.annotate {
            selection.push(ContextAction { label: text!(reader::annotate), icon: "icons/replace.svg", command });
        }
        if let Some(command) = commands.lookup {
            selection.push(ContextAction { label: text!(reader::lookup), icon: "icons/search.svg", command });
        }
        selection.extend([
            ContextAction { label: text!(reader::translate), icon: "icons/translate.svg", command: commands.translate },
            ContextAction { label: text!(reader::copy_markdown), icon: "icons/copy.svg", command: commands.copy },
        ]);
        if let Some(command) = commands.copy_link {
            selection.push(ContextAction { label: text!(reader::copy_link), icon: "icons/link-simple.svg", command });
        }
        actions.add_section(selection);
        actions
    }

    pub(crate) fn mobile_selection(mut commands: SelectionCommands<A>) -> Self {
        commands.copy_link = None;
        let mut actions = Self::selection(commands);
        if let Some(copy) = actions.sections.first_mut().and_then(|section| section.last_mut()) {
            copy.label = text!(common::copy);
        }
        actions
    }

    pub(crate) fn toolbar<C: ContextActionTarget<Command = A>>(&self, bounds: Bounds<Pixels>, theme: components::BrowserTheme, target: &Entity<C>) -> AnyElement {
        let mut toolbar = div().id("reader-selection-toolbar").w(bounds.size.width).h(bounds.size.height)
            .flex().items_center().border_1().border_color(theme.border).bg(theme.page_bg).text_color(theme.text)
            .shadow(components::overlay_shadow(theme)).occlude();
        for (index, action) in self.sections.iter().flatten().enumerate() {
            let command = action.command.clone();
            let target = target.clone();
            toolbar = toolbar.child(div().id(("reader-selection-action", index)).flex_1().h_full().min_w_0()
                .flex().flex_col().items_center().justify_center().gap(px(2.0)).text_size(px(12.0))
                .child(Icon::empty().path(action.icon).size(px(20.0)))
                .child(action.label)
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    target.update(cx, |reader, cx| reader.run_context_action(command.clone(), window, cx));
                }));
        }
        deferred(anchored().position(bounds.origin).child(toolbar)).with_priority(3).into_any_element()
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

    pub(crate) fn sheet<C: ContextActionTarget<Command = A>>(&self, theme: components::BrowserTheme, safe_bottom: Pixels, target: &Entity<C>) -> AnyElement {
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
