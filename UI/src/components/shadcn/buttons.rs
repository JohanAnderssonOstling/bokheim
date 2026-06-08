use crate::style::palette;
use floem::prelude::{button, label, v_stack, Decorators, View};
use floem::style::{AlignContent, CursorStyle};
use floem::views::svg;

#[path = "primitives/button.rs"]
mod shad_button;
use shad_button::ShadButton;

pub fn create_button(text: String) -> impl View {
    ShadButton::new(label(move || text.clone()).style(|s| {
        let theme = palette();
        s.font_family(theme.ui_font_family.to_string()).color(theme.ink_text)
    })).build()
}

pub fn create_compact_button(text: &'static str) -> impl View {
    create_button(text.to_string()).style(|s| s.height(32))
}

pub fn create_dialog_button(text: &'static str) -> impl View {
    create_button(text.to_string()).style(|s| s.height(36))
}

pub fn create_dialog_tab_button(text: &'static str) -> impl View {
    create_button(text.to_string()).style(|s| s.height(30))
}

pub fn create_list_item_button(text: String) -> impl View {
    create_button(text).style(|s| s.flex_grow(1.0).justify_start().hover(|s| s.background(palette().hover_bg)))
}

pub fn create_square_button(text: String) -> impl View {
    create_button(text).style(move |s| s.hover(|s| s.background(palette().hover_bg)).text_ellipsis().font_size(20).width(36).height(36).padding(0).border_radius(8).justify_center().align_content(AlignContent::Center))
}

pub fn create_danger_button(text: &'static str) -> impl View {
    create_button(text.to_string()).style(|s| {
        let theme = palette();
        s.color(theme.danger_text).border_color(theme.danger_text).background(theme.white_surface).hover(|s| s.background(theme.danger_bg))
    })
}

pub fn create_tab_button(text: &'static str, is_active: impl Fn() -> bool + 'static) -> impl View {
    create_button(text.to_string()).style(move |s| {
        let s = s.padding_horiz(16).padding_vert(10).cursor(CursorStyle::Pointer).height(40).border_radius(8);
        let theme = palette();
        if is_active() {
            s.background(theme.active_highlight).border_bottom(2).border_color(theme.accent).color(theme.ink_text)
        } else {
            s.background(theme.white_surface).border_color(theme.border_color).color(theme.ink_text).hover(|s| s.background(theme.hover_bg))
        }
    })
}

pub fn create_sidebar_nav_button(text: &'static str, icon_svg: &'static str, is_active: impl Fn() -> bool + 'static) -> impl View {
    button(v_stack((svg(icon_svg.to_string()).style(|s| s.size(22, 22)), label(move || text).style(|s| {
        let theme = palette();
        s.font_size(12).font_family(theme.ui_font_family.to_string()).color(theme.ink_text)
    })))).style(move |s| {
        let s = s.width(92).height(84).border(1).cursor(CursorStyle::Pointer).flex_col().justify_center().items_center().gap(8);
        let theme = palette();
        if is_active() {
            s.background(theme.active_highlight).border_color(theme.accent)
        } else {
            s.background(theme.white_surface).border_color(theme.border_color).hover(|s| s.background(theme.hover_bg))
        }
    })
}

pub fn add_library_button() -> impl View {
    create_button("Add library".to_string()).style(|s| {
        let theme = palette();
        s.height(36).padding_left(10).padding_right(10).padding_top(6).padding_bottom(6).background(theme.white_surface).border_color(theme.border_color).hover(|s| s.background(theme.hover_bg))
    })
}
