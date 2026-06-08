use crate::components::create_list_item_button;
use crate::style::palette;
use floem::prelude::{button, container, h_stack, label, v_stack, Decorators, RwSignal, SignalGet, View, ViewTuple};
use floem::style::AlignItems;
use floem::views::ScrollExt;

pub fn create_page_message(text: &'static str) -> impl View {
    container(label(move || text).style(|s| {
        let theme = palette();
        s.font_family(theme.ui_font_family.to_string()).color(theme.subtle_text)
    }))
    .style(|s| s.padding(16))
}

pub fn create_empty_page_background(child: impl View + 'static) -> impl View {
    container(child).style(|s| s.width_full().height_full().background(palette().app_bg))
}

pub fn create_switcher_select_button(name: String, is_selected: impl Fn() -> bool + 'static) -> impl View {
    create_list_item_button(name).style(move |s| {
        let theme = palette();
        if is_selected() { s.background(theme.active_highlight) } else { s.color(theme.ink_text) }
    })
}

pub fn create_overlay_scrim(show: RwSignal<bool>, child: impl View + 'static) -> impl View {
    container(child).style(move |s| {
        let base = s.absolute().inset(0).size_full().background(palette().overlay_scrim).z_index(100);
        if show.get() {
            base
        } else {
            base.hide()
        }
    })
}

pub fn full_page_stack<VT: ViewTuple + 'static>(children: VT) -> impl View {
    v_stack(children).style(|s| s.width_full().height_full().flex_col().flex_grow(1.0).min_height(0).min_width(0))
}

pub fn app_root(child: impl View + 'static) -> impl View {
    container(child).style(|s| s.size_pct(100.0, 100.0).width_full().height_full().min_width(0).min_height(0).flex_col())
}

pub fn full_page_container(child: impl View + 'static) -> impl View {
    container(child).style(|s| s.width_full().height_full().flex_grow(1.0).min_height(0).min_width(0))
}

pub fn top_bar(left: impl View + 'static, right: impl View + 'static) -> impl View {
    h_stack((
        container(left).style(|s| s.flex_grow(1.0)),
        container(right).style(|s| s.padding_horiz(16).padding_vert(10).justify_end()),
    ))
    .style(|s| s.width_full().items_center())
    .style(|s| {
        let theme = palette();
        s.width_full().flex_shrink(0.0).border_bottom(1).border_color(theme.border_color).background(theme.white_surface)
    })
}

pub fn top_bar_group<VT: ViewTuple + 'static>(children: VT) -> impl View {
    h_stack(children).style(|s| s.gap(8).padding_horiz(16).padding_vert(10))
}

pub fn modal_panel<VT: ViewTuple + 'static>(children: VT) -> impl View {
    v_stack(children).style(|s| {
        let theme = palette();
        s.width(360).padding(20).gap(8).background(theme.white_surface).border(1).border_color(theme.border_color).border_radius(12)
    })
}

pub fn centered_modal(panel: impl View + 'static) -> impl View {
    container(panel).style(|s| s.absolute().inset(0).size_full().items_center().justify_center().z_index(101))
}

pub fn modal_title(text: &'static str) -> impl View {
    label(move || text.to_string()).style(|s| {
        let theme = palette();
        s.font_size(20).font_family(theme.title_font_family.to_string()).color(theme.ink_text).padding_bottom(8)
    })
}

pub fn form_field_row(label_text: &'static str, input: impl View + 'static) -> impl View {
    v_stack((label(move || label_text.to_string()).style(|s| {
        let theme = palette();
        s.color(theme.subtle_text).font_family(theme.ui_font_family.to_string()).font_size(13)
    }), input))
    .style(|s| s.gap(4).padding_vert(4))
}

pub fn form_tab_row(first: impl View + 'static, second: impl View + 'static) -> impl View {
    h_stack((first, second)).style(|s| s.gap(8).padding_bottom(12))
}

pub fn form_actions(primary: impl View + 'static, secondary: impl View + 'static) -> impl View {
    h_stack((primary, secondary)).style(|s| s.gap(8).padding_top(12).justify_end())
}

pub fn dialog_action_button(content: impl View + 'static) -> impl View {
    button(content).style(|s| s.height(36))
}

pub fn subtle_label(text: impl Fn() -> String + 'static) -> impl View {
    label(text).style(|s| {
        let theme = palette();
        s.color(theme.subtle_text).font_family(theme.ui_font_family.to_string())
    })
}

pub fn danger_message(message: String) -> impl View {
    label(move || message.clone()).style(|s| s.color(palette().danger_text).padding_top(4))
}

pub fn account_status(name: String, action: impl View + 'static) -> impl View {
    h_stack((label(move || name.clone()).style(|s| s.color(palette().subtle_text).padding_right(8)), action)).style(|s| s.gap(8).items_center())
}

pub fn account_button_area(child: impl View + 'static) -> impl View {
    container(child).style(|s| s.padding(8))
}

pub fn switcher_panel<VT: ViewTuple + 'static>(children: VT) -> impl View {
    v_stack(children).style(|s| {
        let theme = palette();
        s.width(620).max_width_pct(92.0).gap(10).padding(12).background(theme.white_surface).border(1).border_color(theme.border_color).border_radius(12)
    })
}

pub fn switcher_header(title: impl View + 'static, action: impl View + 'static) -> impl View {
    h_stack((title, action)).style(|s| s.width_full().justify_between().align_items(AlignItems::Center).padding_bottom(8))
}

pub fn switcher_title(text: &'static str) -> impl View {
    label(move || text.to_string()).style(|s| {
        let theme = palette();
        s.font_size(22).font_family(theme.title_font_family.to_string()).color(theme.subtle_text)
    })
}

pub fn switcher_list(child: impl View + 'static) -> impl View {
    container(child).style(|s| s.width_full())
}

pub fn switcher_rows(child: impl View + 'static) -> impl View {
    child
        .style(|s| s.width_full().flex_col().gap(8))
        .scroll()
        .style(|s| s.width_full().max_height(420).min_height(40))
}

pub fn switcher_item_row(select: impl View + 'static, delete: impl View + 'static) -> impl View {
    h_stack((select, delete)).style(|s| s.width_full().gap(8).align_items(AlignItems::Center))
}

pub fn switcher_overlay_content(panel: impl View + 'static) -> impl View {
    container(panel).style(|s| s.size_full().flex_col().justify_center().items_center().padding(20))
}
