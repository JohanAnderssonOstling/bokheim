//! Application-owned observable state, read by all browser windows.

pub(crate) mod preferences;

pub(crate) use preferences::{
    BrowseViewSetting, CoverTextSetting, GeneratedCoversOnly, Preferences, current_book_list_view, current_catalog_grouping, current_catalog_languages, current_cover_text, current_graph_view, generated_covers_only, save_book_list_view,
    save_catalog_grouping, save_catalog_languages, save_graph_view,
};
