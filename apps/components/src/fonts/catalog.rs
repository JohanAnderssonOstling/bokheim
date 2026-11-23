//! Catalog lifecycle shared by the interface and book font pickers.
use gpui::TextSystem;
use html_fonts::is_private_font_family;
use std::sync::Arc;

#[derive(Default)]
pub struct FontCatalog {
    pub families: Vec<String>,
    pub filtered: Vec<usize>,
    pub cursor: usize,
    pub loaded: bool,
    pub loading: bool,
    pub(super) waiting_for_registration: bool,
    pub(super) text_system: Option<Arc<TextSystem>>,
}
impl Drop for FontCatalog {
    fn drop(&mut self) {
        if let Some(text_system) = &self.text_system {
            text_system.release_font_data();
        }
    }
}

impl FontCatalog {
    pub fn replace(&mut self, mut families: Vec<String>) {
        families.retain(|family| !family.starts_with('.') && !is_private_font_family(family));
        families.sort_by_cached_key(|family| family.to_lowercase());
        families.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
        self.families = families;
        self.loaded = true;
        self.loading = false;
        self.waiting_for_registration = false;
    }
    pub fn filter(&mut self, query: &str) {
        self.cursor = 0;
        let query = query.to_lowercase();
        self.filtered = self.families.iter().enumerate().filter_map(|(index, family)| family.to_lowercase().contains(&query).then_some(index)).collect();
    }
}
