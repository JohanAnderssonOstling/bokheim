//! Leaf widgets used by selected-library pages.

mod book_card;
mod cover_cache;
mod cover_frame;
pub(crate) mod mobile_context_menu;
mod page_controls;
mod paginator;

pub(crate) use book_card::{BookCardEvent, BookCardEvents, BookPlacement, LoadedCover, cover_aspect_ratio, generated_cover_variant, loaded_cover_from_jpeg, mark_feed_book_added};
pub(crate) use book_card::{BookCardInteraction, BookCardPresentation, BookCardView};
pub(crate) use book_card::{CoversEvicted, cover_generation, evict_covers};
pub(crate) use paginator::{
    CursorMove, Paginator, PaginatorAxis, PaginatorChild, PaginatorGroup, PaginatorGroupPolicy, PaginatorPosition, PaginatorSizing, PaginatorState, PaginatorWidthPolicy, book_card_policy, browse_section_card_policy,
    browse_section_item_policy, browse_section_subject_rows, browse_shelf_card_policy, graph_cover_policy,
};

pub(crate) use page_controls::activate_paginator;
