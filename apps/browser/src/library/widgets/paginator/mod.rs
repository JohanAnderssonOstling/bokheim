//! Browser presentation policies layered over the reusable paginator.

mod policy;

#[cfg(all(feature = "graph", not(feature = "kobo")))]
pub(crate) use policy::graph_cover_policy;

pub(crate) use policy::{book_card_policy, browse_folder_card_policy, browse_folder_item_policy, browse_section_card_policy, browse_section_item_policy, browse_shelf_card_policy};
pub(crate) use ui_paginator::{
    CursorMove, Paginator, PaginatorAxis, PaginatorChild, PaginatorChildren, PaginatorDragPreview, PaginatorEvent, PaginatorGroup, PaginatorGroupPolicy, PaginatorPosition, PaginatorSizing, PaginatorSlots, PaginatorState,
    PaginatorWidthPolicy,
};

use gpui::SharedString;
use ui_components as components;
use ui_paginator::PaginatorStyle;

pub(crate) fn new_paginator(id: impl Into<SharedString>, groups: impl IntoIterator<Item = PaginatorGroup>) -> Paginator {
    Paginator::new(id, groups)
        .with_style(|cx| {
            let theme = components::browser_theme(cx);
            PaginatorStyle { item_background: theme.page_bg, item_shadow: components::section_card_shadow(theme), marquee_fill: theme.accent_wash.opacity(0.35), marquee_border: theme.accent }
        })
        .secondary_click_on_release(components::uses_mobile_navigation)
        .profile("browse_profile")
}
