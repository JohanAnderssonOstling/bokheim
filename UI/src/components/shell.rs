use crate::components::page_surface;
use crate::components::tokens::TOKENS;
use crate::components::toolbar_surface;
use floem::prelude::{container, h_stack, v_stack, Decorators, View, ViewTuple};
use floem::views::stack;

pub fn app_root(child: impl View + 'static) -> impl View {
    page_surface(child).style(|s| s.size_pct(100.0, 100.0).flex_col())
}

pub fn full_page_stack<VT: ViewTuple + 'static>(children: VT) -> impl View {
    v_stack(children).style(|s| s.flex_col().flex_grow(1.0).height_full())
}

pub fn top_bar(left: impl View + 'static, right: impl View + 'static) -> impl View {
    toolbar_surface((container(left).style(|s| s.flex_grow(1.0)), container(right).style(|s| s.justify_end()))).style(|s| s.flex_shrink(0.0))
}

pub fn top_bar_group<VT: ViewTuple + 'static>(children: VT) -> impl View {
    h_stack(children).style(|s| s.gap(TOKENS.spacing.sm).padding_horiz(TOKENS.spacing.lg).padding_vert(TOKENS.spacing.md))
}

pub fn navigation_stack<VT: ViewTuple + 'static>(children: VT) -> impl View {
    stack(children).style(|s| s.width_full().height_full().flex_grow(1.0).min_width(0).min_height(0))
}

pub fn navigation_layer(child: impl View + 'static, visible: impl Fn() -> bool + 'static) -> impl View {
    child.style(move |s| if visible() { s.width_full().height_full().flex_grow(1.0).min_width(0).min_height(0) } else { s.hide() })
}

pub fn reader_navigation_surface(child: impl View + 'static) -> impl View {
    container(child).style(|s| s.width_full().height_full().flex_grow(1.0).min_width(0).min_height(0))
}

pub fn library_workspace(sidebar: impl View + 'static, content: impl View + 'static) -> impl View {
    page_surface(h_stack((sidebar, content))).style(|s| s.flex_row().flex_basis(0))
}

/// A tab's content layer: fills the workspace when its tab is active, hidden otherwise.
pub fn tab_panel(child: impl View + 'static, visible: impl Fn() -> bool + 'static) -> impl View {
    child.style(move |s| if visible() { s.size_full() } else { s.hide() })
}
