//! Runs catalogue-wide and selected-source searches as Store mode transitions.

use epub_svelte_lib::catalog::{CatalogLinkTarget, CatalogSearchQuery, CatalogWideSearchOutcome};
use gpui::{Context, Window};

use super::{StoreMode, StorePage};

fn ordered_group_titles<'a>(titles: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut ordered = Vec::<String>::new();
    for title in titles {
        if ordered.iter().any(|existing| existing.eq_ignore_ascii_case(title)) {
            continue;
        }
        if title.eq_ignore_ascii_case("Author matches") {
            ordered.insert(0, "Author matches".to_owned());
        } else {
            ordered.push(title.to_owned());
        }
    }
    ordered
}

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
        let cached_query = query.clone();
        let cached = self.library.spawn(move |library| async move { library.cached_search_catalogs(&cached_query).await.map_err(|error| error.to_string()) });
        let refresh = self.library.spawn(move |library| async move { library.search_catalogs(&query).await.map_err(|error| error.to_string()) });
        self.task = Some(cx.spawn(async move |view, cx| {
            let cached = cached.await.unwrap_or_else(|error| Err(error.to_string()));
            let _ = view.update(cx, |view, cx| {
                if let Ok(search_view) = cached {
                    view.install_wide_search(search_view, cx);
                }
                cx.notify();
            });
            let refreshed = refresh.await.unwrap_or_else(|error| Err(error.to_string()));
            let _ = view.update(cx, |view, cx| {
                view.loading = false;
                if let Ok(search_view) = refreshed {
                    view.install_wide_search(search_view, cx);
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
        let levels = current.levels.clone();
        self.enrichment_refresh_task.take();
        self.loading = true;
        self.error = None;
        let source_id = source.id.clone();
        let cached_source_id = source_id.clone();
        let cached_query = query.clone();
        let installed_query = query.clone();
        let cached = self.library.spawn(move |library| async move { library.cached_search_catalog(&cached_source_id, &cached_query).await.map_err(|error| error.to_string()) });
        let refresh = self.library.spawn(move |library| async move { library.search_catalog(&source_id, &search, CatalogSearchQuery { query, title: None, author: None }).await.map_err(|error| error.to_string()) });
        self.task = Some(cx.spawn(async move |view, cx| {
            let cached = cached.await.unwrap_or_else(|error| Err(error.to_string()));
            let _ = view.update(cx, |view, cx| {
                if let Ok(page_view) = cached {
                    view.install_catalog_search(source.clone(), page_view, levels.clone(), installed_query.clone(), cx);
                }
                cx.notify();
            });
            let refreshed = refresh.await.unwrap_or_else(|error| Err(error.to_string()));
            let _ = view.update(cx, |view, cx| {
                view.loading = false;
                if let Ok(page_view) = refreshed {
                    view.install_catalog_search(source, page_view, levels, installed_query, cx);
                }
                cx.notify();
            });
        }));
    }

    fn install_wide_search(&mut self, search_view: epub_svelte_lib::app::CatalogWideSearchView, cx: &mut Context<Self>) {
        struct ResultPage {
            source: epub_svelte_lib::catalog::CatalogSource,
            page: epub_svelte_lib::catalog::CatalogPage,
        }

        let mut pages = Vec::new();
        let mut statuses = Vec::new();
        for group in search_view.result.groups {
            match group.outcome {
                CatalogWideSearchOutcome::Results { page } => pages.push(ResultPage { source: group.source, page }),
                CatalogWideSearchOutcome::Unsupported => statuses.push(super::WideSearchStatus { source: group.source, outcome: super::WideSearchStatusKind::Unsupported, external_links: Vec::new() }),
                CatalogWideSearchOutcome::TimedOut => statuses.push(super::WideSearchStatus { source: group.source, outcome: super::WideSearchStatusKind::TimedOut, external_links: Vec::new() }),
                CatalogWideSearchOutcome::Failed { message } => statuses.push(super::WideSearchStatus { source: group.source, outcome: super::WideSearchStatusKind::Failed(message), external_links: Vec::new() }),
            }
        }

        let mut seen = std::collections::HashSet::new();
        let mut sections = Vec::new();

        let title_sources = pages
            .iter()
            .filter_map(|result| {
                let cards = self.cards.group_unique(&result.source, &result.page.publications, &search_view.owned_publications, true, &mut seen, cx);
                (!cards.is_empty()).then(|| super::WideSearchSourceResults { source: result.source.clone(), cards, external_links: Vec::new() })
            })
            .collect::<Vec<_>>();
        if !title_sources.is_empty() {
            sections.push(super::WideSearchResultSection { title: "Title matches".to_owned(), sources: title_sources });
        }

        let group_titles = ordered_group_titles(pages.iter().flat_map(|result| result.page.groups.iter().map(|group| group.title.as_str())));

        for title in group_titles {
            let mut sources = Vec::new();
            for result in &pages {
                let mut cards = self.cards.empty_group();
                for group in result.page.groups.iter().filter(|group| group.title.eq_ignore_ascii_case(&title)) {
                    cards.append(self.cards.group_unique(&result.source, &group.publications, &search_view.owned_publications, true, &mut seen, cx));
                }
                if !cards.is_empty() {
                    sources.push(super::WideSearchSourceResults { source: result.source.clone(), cards, external_links: Vec::new() });
                }
            }
            if !sources.is_empty() {
                sections.push(super::WideSearchResultSection { title, sources });
            }
        }

        for result in pages {
            let external_links = result.page.navigation.into_iter().filter(|link| matches!(link.target, CatalogLinkTarget::External { .. })).collect::<Vec<_>>();
            if let Some(source_result) = sections.iter_mut().flat_map(|section| section.sources.iter_mut()).find(|source_result| source_result.source.id == result.source.id) {
                source_result.external_links = external_links;
            } else {
                statuses.push(super::WideSearchStatus { source: result.source, outcome: super::WideSearchStatusKind::NoMatches, external_links });
            }
        }

        self.set_mode(StoreMode::WideSearch(super::WideSearchView { query: search_view.result.query, sections, statuses }));
    }

    fn install_catalog_search(&mut self, source: epub_svelte_lib::catalog::CatalogSource, page_view: epub_svelte_lib::app::CatalogPageView, mut levels: Vec<(String, String)>, query: String, cx: &mut Context<Self>) {
        let page = page_view.page;
        levels.truncate(1);
        levels.push((page.title.clone(), page.locator.clone()));
        let page = self.build_catalogue_view(source, page, page_view.owned_publications, levels, Some("Title matches".to_owned()), Some(query), cx);
        self.set_mode(StoreMode::Catalogue(page));
    }

    /// Returns to the level the search started from, which `install_catalog_search`
    /// kept as the first breadcrumb level.
    pub(super) fn clear_search_within_source(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        let StoreMode::Catalogue(current) = &self.mode else { return };
        let Some((_, locator)) = current.levels.first().cloned() else { return };
        self.search_input.update(cx, |input, cx| input.set_value("", window, cx));
        self.load_page(Some(locator), cx);
    }
}

#[cfg(test)]
mod tests {
    use super::ordered_group_titles;

    #[test]
    fn author_matches_precede_other_catalog_groups() {
        assert_eq!(ordered_group_titles(["Subjects", "author matches", "New releases", "Author matches"]), ["Author matches", "Subjects", "New releases"]);
    }
}
