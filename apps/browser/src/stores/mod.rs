//! Application-owned observable state, read by all browser windows.

pub(crate) mod preferences;

pub(crate) use preferences::{CoverTextSetting, GeneratedCoversOnly, Preferences, current_catalog_grouping, current_catalog_languages, current_cover_text, generated_covers_only, save_catalog_grouping, save_catalog_languages};
