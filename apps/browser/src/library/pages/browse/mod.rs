//! Browsing the library's books, folders, subjects, authors and recent activity.

mod browser_actions;
mod cards;
mod clipboard;
mod controls;
mod detail;
mod drag_drop;
mod folder_actions;
mod folder_picker;
#[cfg(all(feature = "graph", not(feature = "kobo")))]
mod graph;
mod languages;
mod layout;
mod listing;
mod navigator;
mod page;
pub(crate) mod profile;
mod section_card;
mod table;

pub(crate) use detail::spoken_duration;
pub(crate) use listing::BrowseKind;
pub(crate) use page::BrowsePage;
