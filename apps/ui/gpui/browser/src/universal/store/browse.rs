//! Loads catalogue sources and transitions the typed Store mode.

use epub_svelte_lib::catalog::{CatalogPage, CatalogSource};
use gpui::Context;

use super::{StoreMode, StorePage};

impl StorePage {
    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        self.loading = true;
        self.error = None;
        let operation = self.services.backend.spawn(move |core| async move {
            let available = core.catalog_sources();
            match core.refresh_catalog_sources().await {
                Ok(sources) => (sources, None),
                Err(error) => (available, Some(error.to_string())),
            }
        });
        self.task = Some(cx.spawn(async move |page, cx| {
            let result = operation.await;
            let _ = page.update(cx, |page, cx| {
                page.loading = false;
                match result {
                    Ok((sources, error)) => {
                        page.sources = std::sync::Arc::new(sources);
                        page.mode = StoreMode::Catalogues;
                        page.error = error.map(Into::into);
                    }
                    Err(error) => page.error = Some(error.to_string().into()),
                }
                cx.notify();
            });
        }));
    }

    pub(super) fn select_source(&mut self, source: CatalogSource, cx: &mut Context<Self>) {
        if self.loading || matches!(&self.mode, StoreMode::Catalogue(view) if view.source.id == source.id) {
            return;
        }
        self.error = None;
        self.load_page_for(source, None, true, cx);
    }

    pub(super) fn show_catalogues(&mut self, cx: &mut Context<Self>) {
        self.task.take();
        self.loading = false;
        self.mode = StoreMode::Catalogues;
        self.error = None;
        cx.notify();
    }

    pub(super) fn load_page(&mut self, locator: Option<String>, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        let StoreMode::Catalogue(view) = &self.mode else { return };
        self.load_page_for(view.source.clone(), locator, false, cx);
    }

    fn load_page_for(&mut self, source: CatalogSource, locator: Option<String>, reset: bool, cx: &mut Context<Self>) {
        self.loading = true;
        self.error = None;
        let source_id = source.id.clone();
        let operation = self.library.spawn(move |library| async move { library.catalog_page(&source_id, locator.as_deref()).await.map_err(|error| error.to_string()) });
        self.task = Some(cx.spawn(async move |view, cx| {
            let result = operation.await.unwrap_or_else(|error| Err(error.to_string()));
            let _ = view.update(cx, |view, cx| {
                view.loading = false;
                match result {
                    Ok(page_view) => {
                        let mut levels = match &view.mode {
                            StoreMode::Catalogue(current) if !reset && current.source.id == source.id => current.levels.clone(),
                            _ => Vec::new(),
                        };
                        push_level(&mut levels, &page_view.page, reset);
                        let page = view.build_catalogue_view(source, page_view.page, page_view.owned_publications, levels, None, cx);
                        view.mode = StoreMode::Catalogue(page);
                    }
                    Err(error) => view.error = Some(error.into()),
                }
                cx.notify();
            });
        }));
    }

    pub(super) fn build_catalogue_view(
        &self, source: CatalogSource, page: CatalogPage, owned: std::collections::HashSet<epub_svelte_lib::catalog::CatalogPublicationKey>, levels: Vec<(String, String)>, primary_section_title: Option<String>, cx: &mut Context<Self>,
    ) -> super::CatalogueView {
        let mut seen = std::collections::HashSet::new();
        let publications = self.cards.group_unique(&source, &page.publications, &owned, false, &mut seen, cx);
        let groups = page
            .groups
            .into_iter()
            .map(|group| {
                let number_of_items = group.number_of_items.unwrap_or(group.publications.len() as u32);
                let cards = self.cards.group_unique(&source, &group.publications, &owned, false, &mut seen, cx);
                super::CatalogueGroupView { title: group.title, page_locator: group.page_locator, number_of_items, navigation: group.navigation, cards }
            })
            .collect();
        super::CatalogueView { source, search: page.search, primary_section_title, levels, navigation: page.navigation, facets: page.facets, pagination: page.pagination, publications, groups }
    }
}

fn push_level(levels: &mut Vec<(String, String)>, page: &CatalogPage, reset: bool) {
    let next = (page.title.clone(), page.locator.clone());
    if reset || levels.is_empty() {
        *levels = vec![next];
    } else if let Some(index) = levels.iter().position(|(_, locator)| locator == &page.locator) {
        levels.truncate(index + 1);
    } else {
        levels.push(next);
    }
}

pub(super) fn source_detail(source: &CatalogSource) -> String {
    let languages = source.languages.join(", ");
    if languages.is_empty() {
        "OPDS".to_owned()
    } else {
        format!("OPDS · {languages}")
    }
}
