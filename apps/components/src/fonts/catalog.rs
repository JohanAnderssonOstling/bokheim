//! Catalog lifecycle shared by the interface and book font pickers.
use super::SystemFontFamilies;
use gpui::{App, Context, ScrollStrategy, SharedString, Subscription, Task, TextSystem, UniformListScrollHandle};
use std::sync::Arc;

#[derive(Default)]
pub struct FontCatalog {
    pub families: Vec<String>,
    pub filtered: Vec<usize>,
    pub loaded: bool,
    pub loading: bool,
    waiting_for_registration: bool,
    text_system: Option<Arc<TextSystem>>,
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
        families.retain(|family| !family.starts_with('.'));
        families.sort_by_cached_key(|family| family.to_lowercase());
        families.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
        self.families = families;
        self.loaded = true;
        self.loading = false;
        self.waiting_for_registration = false;
    }
    pub fn filter(&mut self, query: &str) {
        let query = query.to_lowercase();
        self.filtered = self.families.iter().enumerate().filter_map(|(index, family)| family.to_lowercase().contains(&query).then_some(index)).collect();
    }
}

pub trait FontCatalogTarget: Sized + 'static {
    fn font_catalog(&mut self) -> &mut FontCatalog;
    fn font_query(&self, cx: &App) -> SharedString;
    fn font_scroll(&self) -> &UniformListScrollHandle;
    /// Book pickers can check glyph coverage on the background worker.
    fn font_filter(&self) -> impl Fn(&TextSystem, &str) -> bool + Send + 'static;

    fn filter_font_catalog(&mut self, cx: &App) {
        let query = self.font_query(cx);
        self.font_catalog().filter(&query);
        self.font_scroll().scroll_to_item(0, ScrollStrategy::Top);
    }
    fn install_font_catalog(&mut self, families: Vec<String>, cx: &mut Context<Self>) {
        self.font_catalog().replace(families);
        self.filter_font_catalog(cx);
        cx.notify();
    }
}

pub fn load_font_catalog<T: FontCatalogTarget>(owner: &mut T, cx: &mut Context<T>) -> Option<Task<()>> {
    let catalog = owner.font_catalog();
    if catalog.loaded || catalog.loading {
        return None;
    }
    // Bundled families are choices even when the current UI has never used
    // them. Register their previews only when the picker is actually opened.
    if let Err(error) = super::register_picker_fonts(cx) {
        log::error!("failed to install font picker previews: {error}");
    }
    let catalog = owner.font_catalog();
    catalog.loading = true;
    catalog.text_system = Some(cx.text_system().clone());
    let cached = cx.try_global::<SystemFontFamilies>().map(|fonts| fonts.0.clone());
    // Android publishes its catalog after background font registration.
    if cfg!(target_os = "android") && cached.as_ref().is_some_and(Vec::is_empty) {
        owner.font_catalog().waiting_for_registration = true;
        return None;
    }
    let select = owner.font_filter();
    let text_system = cx.text_system().clone();
    let fonts = cx.background_executor().spawn(async move {
        let all = cached.unwrap_or_else(|| text_system.all_font_names());
        let selected = all.iter().filter(|family| select(&text_system, family)).cloned().collect();
        (all, selected)
    });
    Some(cx.spawn(async move |owner, cx| {
        let (all, selected) = fonts.await;
        let _ = owner.update(cx, |owner, cx| {
            owner.install_font_catalog(selected, cx);
            #[cfg(target_os = "android")]
            cx.set_global(SystemFontFamilies(all));
            #[cfg(not(target_os = "android"))]
            drop(all);
        });
    }))
}

/// Resume a picker waiting for Android's catalog without enumerating at startup.
pub fn observe_font_catalog<T: FontCatalogTarget>(cx: &mut Context<T>, mut resume: impl FnMut(&mut T, &mut Context<T>) + 'static) -> Subscription {
    cx.observe_global::<SystemFontFamilies>(move |owner, cx| {
        if owner.font_catalog().waiting_for_registration && cx.try_global::<SystemFontFamilies>().is_some_and(|fonts| !fonts.0.is_empty()) {
            owner.font_catalog().waiting_for_registration = false;
            owner.font_catalog().loading = false;
            resume(owner, cx);
        }
    })
}
