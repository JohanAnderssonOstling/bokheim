//! Loads catalogue sources and transitions the typed Store mode.

use epub_svelte_lib::catalog::{CatalogPage, CatalogSource, CatalogSubjectBrowseRequest, CatalogSubjectBrowseResponse};
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
            let refresh_error = core.refresh_catalog_sources().await.err().map(|error| error.to_string());
            (core.supported_catalog_sources(), refresh_error)
        });
        self.task = Some(cx.spawn(async move |page, cx| {
            let result = operation.await;
            let _ = page.update(cx, |page, cx| {
                page.loading = false;
                match result {
                    Ok((sources, error)) => {
                        page.sources = std::sync::Arc::new(sources);
                        page.set_mode(StoreMode::Catalogues);
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
        self.load_page_for(source, None, true, true, cx);
    }

    pub(super) fn show_catalogues(&mut self, cx: &mut Context<Self>) {
        self.task.take();
        self.enrichment_refresh_task.take();
        self.loading = false;
        self.set_mode(StoreMode::Catalogues);
        self.error = None;
        cx.notify();
    }

    pub(super) fn load_subjects(&mut self, locator: Option<String>, title: Option<String>, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        self.loading = true;
        self.error = None;
        let requested_locator = locator.clone();
        let operation = self.library.spawn(move |library| async move { library.browse_catalog_subjects(CatalogSubjectBrowseRequest { locator, supported_languages: Vec::new(), limit: 100 }).await.map_err(|error| error.to_string()) });
        self.task = Some(cx.spawn(async move |page, cx| {
            let result = operation.await.unwrap_or_else(|error| Err(error.to_string()));
            let _ = page.update(cx, |page, cx| {
                page.loading = false;
                match result {
                    Ok(view) => {
                        let mut levels = match &page.mode {
                            StoreMode::Subjects(current) => current.levels.clone(),
                            _ => vec![("Subjects".to_owned(), None)],
                        };
                        if let Some(index) = levels.iter().position(|(_, locator)| locator == &requested_locator) {
                            levels.truncate(index + 1);
                        } else if let (Some(title), Some(locator)) = (title, requested_locator) {
                            levels.push((title, Some(locator)));
                        }
                        let subjects = page.build_subject_view(view.response, view.owned_publications, levels, cx);
                        page.set_mode(StoreMode::Subjects(subjects));
                    }
                    Err(error) => page.error = Some(error.into()),
                }
                cx.notify();
            });
        }));
    }

    pub(super) fn load_next_subject_page(&mut self, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        let StoreMode::Subjects(current) = &self.mode else { return };
        let Some(locator) = current.response.pagination.next.clone() else { return };
        self.loading = true;
        let operation =
            self.library.spawn(move |library| async move { library.browse_catalog_subjects(CatalogSubjectBrowseRequest { locator: Some(locator), supported_languages: Vec::new(), limit: 100 }).await.map_err(|error| error.to_string()) });
        self.task = Some(cx.spawn(async move |page, cx| {
            let result = operation.await.unwrap_or_else(|error| Err(error.to_string()));
            let _ = page.update(cx, |page, cx| {
                page.loading = false;
                match result {
                    Ok(next) => {
                        let Some((mut response, mut owned, levels)) = (match &page.mode {
                            StoreMode::Subjects(current) => Some((current.response.clone(), current.owned_publications.clone(), current.levels.clone())),
                            _ => None,
                        }) else {
                            return;
                        };
                        let mut seen = response.publications.iter().map(|item| (item.source_id.clone(), item.publication.identity.clone())).collect::<std::collections::HashSet<_>>();
                        response.publications.extend(next.response.publications.into_iter().filter(|item| seen.insert((item.source_id.clone(), item.publication.identity.clone()))));
                        response.pagination = next.response.pagination;
                        owned.extend(next.owned_publications);
                        let subjects = page.build_subject_view(response, owned, levels, cx);
                        page.set_mode(StoreMode::Subjects(subjects));
                    }
                    Err(error) => page.error = Some(error.into()),
                }
                cx.notify();
            });
        }));
    }

    fn build_subject_view(
        &self, response: CatalogSubjectBrowseResponse, owned: std::collections::HashSet<epub_svelte_lib::catalog::CatalogPublicationKey>, levels: Vec<(String, Option<String>)>, cx: &mut Context<Self>,
    ) -> super::SubjectBrowseView {
        let mut seen = std::collections::HashSet::new();
        let sources = self
            .sources
            .iter()
            .filter_map(|source| {
                let publications = response.publications.iter().filter(|item| item.source_id == source.id).map(|item| item.publication.clone()).collect::<Vec<_>>();
                if publications.is_empty() {
                    return None;
                }
                let cards = self.cards.group_unique(source, &publications, &owned, false, &mut seen, cx);
                Some(super::WideSearchSourceResults { source: source.clone(), cards, external_links: Vec::new() })
            })
            .collect();
        super::SubjectBrowseView { response, owned_publications: owned, levels, sources }
    }

    pub(super) fn load_page(&mut self, locator: Option<String>, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        let StoreMode::Catalogue(view) = &self.mode else { return };
        let is_pagination = locator.as_ref().is_some_and(|locator| view.pagination.next.as_ref() == Some(locator) || view.pagination.previous.as_ref() == Some(locator));
        self.load_page_for(view.source.clone(), locator, false, !is_pagination, cx);
    }

    pub(super) fn load_next_page(&mut self, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        let StoreMode::Catalogue(current) = &self.mode else { return };
        let Some(locator) = current.pagination.next.clone() else { return };
        let source = current.source.clone();
        let source_id = source.id.clone();
        self.loading = true;
        self.error = None;
        let operation = self.library.spawn(move |library| async move { library.catalog_page(&source_id, Some(&locator)).await.map_err(|error| error.to_string()) });
        self.task = Some(cx.spawn(async move |view, cx| {
            let result = operation.await.unwrap_or_else(|error| Err(error.to_string()));
            let _ = view.update(cx, |view, cx| {
                view.loading = false;
                match result {
                    Ok(page_view) => {
                        let Some((mut snapshot, levels, primary_section_title, query, mut owned)) = (match &view.mode {
                            StoreMode::Catalogue(current) if current.source.id == source.id => {
                                Some((current.snapshot.clone(), current.levels.clone(), current.primary_section_title.clone(), current.query.clone(), current.owned_publications.clone()))
                            }
                            _ => None,
                        }) else {
                            return;
                        };
                        append_catalog_page(&mut snapshot, page_view.page);
                        owned.extend(page_view.owned_publications);
                        let page = view.build_catalogue_view(source, snapshot, owned, levels, primary_section_title, query, cx);
                        view.set_mode(StoreMode::Catalogue(page));
                        view.schedule_enrichment_refresh(cx);
                    }
                    Err(error) => view.error = Some(error.into()),
                }
                cx.notify();
            });
        }));
    }

    fn load_page_for(&mut self, source: CatalogSource, locator: Option<String>, reset: bool, update_breadcrumbs: bool, cx: &mut Context<Self>) {
        self.enrichment_refresh_task.take();
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
                        if update_breadcrumbs {
                            push_level(&mut levels, &page_view.page, reset);
                        }
                        let page = view.build_catalogue_view(source, page_view.page, page_view.owned_publications, levels, None, None, cx);
                        view.set_mode(StoreMode::Catalogue(page));
                        view.schedule_enrichment_refresh(cx);
                    }
                    Err(error) => view.error = Some(error.into()),
                }
                cx.notify();
            });
        }));
    }

    pub(super) fn build_catalogue_view(
        &self, source: CatalogSource, page: CatalogPage, owned: std::collections::HashSet<epub_svelte_lib::catalog::CatalogPublicationKey>, levels: Vec<(String, String)>, primary_section_title: Option<String>, query: Option<String>,
        cx: &mut Context<Self>,
    ) -> super::CatalogueView {
        let snapshot = page.clone();
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
        super::CatalogueView { snapshot, source, search: page.search, query, primary_section_title, levels, navigation: page.navigation, facets: page.facets, pagination: page.pagination, owned_publications: owned, publications, groups }
    }

    fn schedule_enrichment_refresh(&mut self, cx: &mut Context<Self>) {
        self.enrichment_refresh_task.take();
        let StoreMode::Catalogue(current) = &self.mode else { return };
        if !page_needs_enrichment_refresh(&current.snapshot) {
            return;
        }
        let timer = cx.background_executor().clone();
        let library = self.library.clone();
        self.enrichment_refresh_task = Some(cx.spawn(async move |entity, cx| {
            for _ in 0..30 {
                timer.timer(std::time::Duration::from_secs(2)).await;
                let request = entity
                    .update(cx, |page, _| match &page.mode {
                        StoreMode::Catalogue(current) if current.query.is_none() && page_needs_enrichment_refresh(&current.snapshot) => Some((current.source.clone(), current.snapshot.locator.clone())),
                        _ => None,
                    })
                    .ok()
                    .flatten();
                let Some((source, locator)) = request else { break };
                let source_id = source.id.clone();
                let result = library.spawn(move |library| async move { library.catalog_page(&source_id, Some(&locator)).await.map_err(|error| error.to_string()) }).await;
                let Ok(Ok(page_view)) = result else { continue };
                let keep_polling = entity
                    .update(cx, |page, cx| {
                        let replacement = match &page.mode {
                            StoreMode::Catalogue(current) if current.query.is_none() && current.source.id == source.id && current.snapshot.locator == page_view.page.locator && current.snapshot != page_view.page => {
                                Some((current.levels.clone(), current.primary_section_title.clone()))
                            }
                            StoreMode::Catalogue(current) if current.query.is_none() && current.source.id == source.id => None,
                            _ => return false,
                        };
                        if let Some((levels, primary_section_title)) = replacement {
                            let refreshed = page.build_catalogue_view(source, page_view.page, page_view.owned_publications, levels, primary_section_title, None, cx);
                            page.set_mode(StoreMode::Catalogue(refreshed));
                            cx.notify();
                        }
                        matches!(&page.mode, StoreMode::Catalogue(current) if page_needs_enrichment_refresh(&current.snapshot))
                    })
                    .unwrap_or(false);
                if !keep_polling {
                    break;
                }
            }
        }));
    }
}

fn page_needs_enrichment_refresh(page: &CatalogPage) -> bool {
    page.publications.iter().chain(page.groups.iter().flat_map(|group| group.publications.iter())).any(|publication| publication.image_url.as_deref().is_some_and(|url| url.starts_with("urn:bokheim:epub-cover:")))
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

fn append_catalog_page(current: &mut CatalogPage, next: CatalogPage) {
    let mut seen = current.publications.iter().chain(current.groups.iter().flat_map(|group| group.publications.iter())).map(|publication| publication.identity.clone()).collect::<std::collections::HashSet<_>>();
    current.publications.extend(next.publications.into_iter().filter(|publication| seen.insert(publication.identity.clone())));
    for mut next_group in next.groups {
        if let Some(group) = current.groups.iter_mut().find(|group| group.title == next_group.title && group.page_locator == next_group.page_locator) {
            group.publications.extend(next_group.publications.drain(..).filter(|publication| seen.insert(publication.identity.clone())));
            group.number_of_items = group.number_of_items.or(next_group.number_of_items);
        } else {
            next_group.publications.retain(|publication| seen.insert(publication.identity.clone()));
            current.groups.push(next_group);
        }
    }
    current.pagination = next.pagination;
}
