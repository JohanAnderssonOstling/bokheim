use gpui::prelude::*;
use gpui::{App, ClickEvent, SharedString, Window, div, px};
use gpui_component::menu::PopupMenuItem;
use gpui_component::Icon;

use crate::{browser_theme, TEXT_SM};

/// Build a text-only popup menu item with the project's common layout.
pub fn menu_item<L, F>(label: L, on_click: F) -> PopupMenuItem
where
    L: Into<SharedString>,
    F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
{
    menu_item_impl(label.into(), None, on_click)
}

/// Build a popup menu item with an icon and the project's common layout.
pub fn menu_item_with_icon<L, I, F>(label: L, icon: I, on_click: F) -> PopupMenuItem
where
    L: Into<SharedString>,
    I: Into<Icon> + 'static,
    F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
{
    menu_item_impl(label.into(), Some(icon.into()), on_click)
}

fn menu_item_impl<F>(label: SharedString, icon: Option<Icon>, on_click: F) -> PopupMenuItem
where
    F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
{

    let item = PopupMenuItem::element(move |_, cx| {
        let theme = browser_theme(cx);
        let row = div()
            .flex()
            .flex_1()
            .items_center()
            .gap(px(6.0))
            .text_size(gpui::rems(TEXT_SM))
            .text_color(theme.text);

        row.child(div().min_w_0().text_ellipsis().child(label.clone()))
    });
    let item = if let Some(icon) = icon { item.icon(icon) } else { item };
    item.on_click(on_click)
}

/// Build a non-interactive popup menu section heading.
pub fn menu_section(label: impl Into<SharedString>) -> PopupMenuItem {
    PopupMenuItem::label(label)
}
