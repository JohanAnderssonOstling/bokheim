use crate::components::tokens::TOKENS;
use crate::style::palette;
use floem::prelude::{container, Decorators, View, ViewTuple};
use floem::views::{h_stack, Container};

pub fn page_surface(child: impl View + 'static) -> impl View {
    container(child).style(|s| s.width_full().height_full().flex_grow(1.0).min_width(0).min_height(0).background(palette().app_bg))
}

/// Base for elevated panels: padding, border and drop shadow. Surfaces derive
/// from this and only set background, radius and elevation depth.
fn elevated_surface(child: impl View + 'static) -> Container {
    container(child).style(|s| {
        let theme = palette();
        s.width_full().padding(TOKENS.spacing.lg).border(1).border_color(theme.border_color).box_shadow_color(theme.box_shadow).box_shadow_spread(0).box_shadow_h_offset(0)
    })
}

pub fn panel_surface(child: impl View + 'static) -> impl View {
    elevated_surface(child).style(|s| s.background(palette().surface_bg).border_radius(TOKENS.radius.xl).box_shadow_blur(18).box_shadow_v_offset(10))
}

pub fn card_surface(child: impl View + 'static) -> impl View {
    elevated_surface(child).style(|s| s.background(palette().card_bg).border_radius(TOKENS.radius.lg).box_shadow_blur(14).box_shadow_v_offset(7))
}

pub fn toolbar_surface<VT: ViewTuple + 'static>(children: VT) -> impl View {
    h_stack(children).style(|s| {
        let theme = palette();
        s.width_full().padding_horiz(TOKENS.spacing.lg).padding_vert(TOKENS.spacing.sm).align_items(floem::style::AlignItems::Center).gap(TOKENS.spacing.md).border_bottom(1).border_color(theme.border_color).background(theme.surface_bg)
    })
}
