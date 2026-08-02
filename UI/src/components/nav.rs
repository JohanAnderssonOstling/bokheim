use crate::components::text::ui_label;
use crate::components::tokens::TOKENS;
use crate::style::palette;
use floem::prelude::{button, v_stack, Decorators, View, ViewTuple};
use floem::style::{AlignItems, CursorStyle};
use floem::views::svg;

pub fn nav_rail<VT: ViewTuple + 'static>(items: VT) -> impl View {
    v_stack(items).style(|s| {
        let theme = palette();
        s.width(TOKENS.size.nav_rail_width).height_full().padding(0).gap(0).align_items(AlignItems::Start).background(theme.surface_bg).border_right(1).border_color(theme.border_color)
    })
}

pub fn nav_rail_item(text: &'static str, icon_svg: &'static str, is_active: impl Fn() -> bool + 'static) -> floem::views::Button {
    button(v_stack((svg(icon_svg.to_string()).style(|s| s.size(22, 22)), ui_label(move || text.to_string()).style(|s| s.font_size(TOKENS.typography.text_xs))))).style(move |s| {
        let theme = palette();
        let base = s.width(TOKENS.size.nav_rail_width).height(TOKENS.size.nav_item_height).border(1).cursor(CursorStyle::Pointer).flex_col().justify_center().items_center().gap(TOKENS.spacing.sm);

        if is_active() {
            base.background(theme.active_highlight).border_color(theme.accent)
        } else {
            base.background(theme.surface_bg).border_color(theme.border_color).hover(|s| s.background(theme.hover_bg))
        }
    })
}
