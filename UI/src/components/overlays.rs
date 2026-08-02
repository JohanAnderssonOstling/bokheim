use crate::components::list_item_button;
use crate::components::panel_surface;
use crate::components::text::subtle_label;
use crate::components::tokens::TOKENS;
use crate::style::palette;
use floem::prelude::{container, h_stack, v_stack, Button, Decorators, RwSignal, SignalGet, View, ViewTuple};
use floem::style::AlignItems;
use floem::views::ScrollExt;

pub fn create_switcher_select_button(name: String, is_selected: impl Fn() -> bool + 'static) -> Button {
    list_item_button(name).style(move |s| {
        let theme = palette();
        let s = s.flex_grow(1.0).height(36).justify_start().padding_horiz(12);
        if is_selected() {
            s.background(theme.active_highlight)
        } else {
            s.color(theme.ink_text)
        }
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

pub fn modal_panel<VT: ViewTuple + 'static>(children: VT) -> impl View {
    panel_surface(v_stack(children).style(|s| s.gap(TOKENS.spacing.sm))).style(|s| s.width(360).background(palette().white_surface))
}

pub fn centered_modal(panel: impl View + 'static) -> impl View {
    container(panel).style(|s| s.absolute().inset(0).size_full().items_center().justify_center().z_index(101))
}

pub fn form_field_row(label_text: &'static str, input: impl View + 'static) -> impl View {
    v_stack((subtle_label(move || label_text.to_string()).style(|s| s.font_size(TOKENS.typography.text_sm)), input)).style(|s| s.gap(TOKENS.spacing.xs).padding_vert(TOKENS.spacing.xs))
}

pub fn form_tab_row(first: impl View + 'static, second: impl View + 'static) -> impl View {
    h_stack((first, second)).style(|s| s.gap(TOKENS.spacing.sm).padding_bottom(TOKENS.spacing.md))
}

pub fn form_actions(primary: impl View + 'static, secondary: impl View + 'static) -> impl View {
    h_stack((primary, secondary)).style(|s| s.gap(TOKENS.spacing.sm).padding_top(TOKENS.spacing.md).justify_end())
}

pub fn account_status(name: String, action: impl View + 'static) -> impl View {
    h_stack((subtle_label(move || name.clone()).style(|s| s.padding_right(TOKENS.spacing.sm)), action)).style(|s| s.gap(TOKENS.spacing.sm).items_center())
}

/// Layout slot for the account button in the top bar.
pub fn account_button_area(child: impl View + 'static) -> impl View {
    child.style(|s| s.padding(TOKENS.spacing.sm))
}

pub fn switcher_panel<VT: ViewTuple + 'static>(children: VT) -> impl View {
    panel_surface(v_stack(children).style(|s| s.gap(TOKENS.spacing.sm))).style(|s| s.width(620).max_width_pct(92.0).background(palette().white_surface))
}

pub fn switcher_header(title: impl View + 'static, action: impl View + 'static) -> impl View {
    h_stack((title, action)).style(|s| s.width_full().justify_between().align_items(AlignItems::Center).padding_bottom(TOKENS.spacing.sm))
}

pub fn switcher_rows(child: impl View + 'static) -> impl View {
    child.style(|s| s.width_full().flex_col().gap(TOKENS.spacing.sm)).scroll().style(|s| s.width_full().max_height(420).min_height(40))
}

pub fn switcher_item_row(select: impl View + 'static, delete: impl View + 'static) -> impl View {
    h_stack((select, delete)).style(|s| s.width_full().gap(TOKENS.spacing.sm).align_items(AlignItems::Center))
}

fn switcher_overlay_content(panel: impl View + 'static) -> impl View {
    container(panel).style(|s| s.size_full().flex_col().justify_center().items_center().padding(TOKENS.spacing.lg))
}

pub fn switcher_overlay_root(panel: impl View + 'static, show_switcher: RwSignal<bool>) -> impl View {
    create_overlay_scrim(show_switcher, switcher_overlay_content(panel))
}
