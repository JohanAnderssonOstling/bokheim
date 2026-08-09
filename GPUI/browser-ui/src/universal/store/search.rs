//! Runs catalogue-wide and selected-source searches as Store mode transitions.

use epub_svelte_lib::catalog::{CatalogLinkTarget, CatalogSearchQuery, CatalogWideSearchOutcome};
use gpui::Context;

use super::{StoreMode, StorePage};

impl StorePage {
    pub(super) fn search_catalogs(&mut self, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        let query = self.wide_search_input.read(cx).value().trim().to_owned();
        if query.is_empty() {
            return;
        }
        self.loading = true;
        self.error = None;
        let operation = self.library.spawn(move |library| async move { library.search_catalogs(&query).await.map_err(|error| error.to_string()) });
        self.task = Some(cx.spawn(async move |view, cx| {
            let result = operation.await.unwrap_or_else(|error| Err(error.to_string()));
            let _ = view.update(cx, |view, cx| {
                view.loading = false;
                match result {
                    Ok(search_view) => {
                        let groups = search_view
                            .result
                            .groups
                            .into_iter()
                            .map(|group| {
                                let outcome = match group.outcome {
                                    CatalogWideSearchOutcome::Results { page } => {
                                        let mut publications = page.publications;
                                        publications.extend(page.groups.into_iter().flat_map(|group| group.publications));
                                        let cards = view.cards.group(&group.source, &publications, &search_view.owned_editions, false, cx);
                                        let external_links = page.navigation.into_iter().filter(|link| matches!(link.target, CatalogLinkTarget::External { .. })).collect();
                                        super::WideSearchOutcome::Results { cards, external_links }
                                    }
                                    CatalogWideSearchOutcome::AuthenticationRequired => super::WideSearchOutcome::AuthenticationRequired,
                                    CatalogWideSearchOutcome::Unsupported => super::WideSearchOutcome::Unsupported,
                                    CatalogWideSearchOutcome::TimedOut => super::WideSearchOutcome::TimedOut,
                                    CatalogWideSearchOutcome::Failed { message } => super::WideSearchOutcome::Failed(message),
                                };
                                super::WideSearchGroup { source: group.source, outcome }
                            })
                            .collect();
                        view.mode = StoreMode::WideSearch(super::WideSearchView { query: search_view.result.query, groups });
                    }
                    Err(error) => view.error = Some(error.into()),
                }
                cx.notify();
            });
        }));
    }

    pub(super) fn search_within_source(&mut self, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        let StoreMode::Catalogue(current) = &self.mode else { return };
        let Some(search) = current.search.clone() else { return };
        let query = self.search_input.read(cx).value().trim().to_owned();
        if query.is_empty() {
            return;
        }
        let source = current.source.clone();
        let mut levels = current.levels.clone();
        self.loading = true;
        self.error = None;
        let source_id = source.id.clone();
        let operation = self.library.spawn(move |library| async move { library.search_catalog(&source_id, &search, CatalogSearchQuery { query, title: None, author: None }, None).await.map_err(|error| error.to_string()) });
        self.task = Some(cx.spawn(async move |view, cx| {
            let result = operation.await.unwrap_or_else(|error| Err(error.to_string()));
            let _ = view.update(cx, |view, cx| {
                view.loading = false;
                match result {
                    Ok(page_view) => {
                        let page = page_view.page;
                        levels.truncate(1);
                        levels.push((page.title.clone(), page.locator.clone()));
                        let page = view.build_catalogue_view(source, page, page_view.owned_editions, levels, cx);
                        view.mode = StoreMode::Catalogue(page);
                    }
                    Err(error) => view.error = Some(error.into()),
                }
                cx.notify();
            });
        }));
    }
}
