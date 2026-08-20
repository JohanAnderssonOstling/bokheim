//! Store source for the shared chip-and-book browse layout.

use std::rc::Rc;
use std::sync::Arc;

use epub_svelte_lib::catalog::{CatalogLink, CatalogLinkTarget, CatalogSource, CatalogSubjectBrowseRequest};
use epub_svelte_lib::{BrowseRow, LibraryBackend};
use gpui::prelude::*;
use gpui::{App, Context, IntoElement, Render, SharedString, Task, Window, rems};

use super::browse_grid;
use crate::catalog::{CatalogCardGroup, CatalogCardServices};
use crate::services::AppServices;

pub(crate) struct StoreBrowsePage {
    library: LibraryBackend,
    sources: Arc<Vec<CatalogSource>>,
    card_services: CatalogCardServices,
    location: Option<String>,
    navigation: Rc<Vec<CatalogLink>>,
    chips: Rc<Vec<BrowseRow>>,
    books: CatalogCardGroup,
    loading: bool,
    error: Option<SharedString>,
    task: Option<Task<()>>,
}

pub(crate) fn new(services: AppServices, library: LibraryBackend, cx: &mut Context<StoreBrowsePage>) -> StoreBrowsePage {
    let sources = Arc::new(services.backend.catalog_sources());
    let import_library = library.clone();
    let import = Rc::new(move |request| {
        Ok(import_library.spawn(move |library| async move {
            library
                .import_catalog_book(epub_svelte_lib::ROOT_DIR_ID, &request)
                .await
                .map(|_| ())
                .map_err(|error| error.to_string())
        }))
    });
    let card_services = CatalogCardServices::new(&services, import);
    let books = card_services.empty_group();
    let mut page = StoreBrowsePage {
        library,
        sources,
        card_services,
        location: None,
        navigation: Rc::new(Vec::new()),
        chips: Rc::new(Vec::new()),
        books,
        loading: false,
        error: None,
        task: None,
    };
    page.load(None, cx);
    page
}

impl StoreBrowsePage {
    fn load(&mut self, location: Option<String>, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        self.loading = true;
        self.error = None;
        cx.notify();

        let request_location = location.clone();
        let operation = self.library.spawn(move |library| async move {
            library
                .browse_catalog_subjects(CatalogSubjectBrowseRequest {
                    locator: location,
                    supported_languages: Vec::new(),
                    source_ids: Vec::new(),
                    query: String::new(),
                    limit: 100,
                })
                .await
                .map_err(|error| error.to_string())
        });
        self.task = Some(cx.spawn(async move |page, cx| {
            let result = operation.await.unwrap_or_else(|error| Err(error.to_string()));
            let _ = page.update(cx, |page, cx| {
                page.loading = false;
                page.task.take();
                match result {
                    Ok(view) => page.install(request_location, view, cx),
                    Err(error) => page.error = Some(error.into()),
                }
                cx.notify();
            });
        }));
    }

    fn install(&mut self, location: Option<String>, view: epub_svelte_lib::app::CatalogSubjectBrowseView, cx: &mut Context<Self>) {
        let navigation = view.response.navigation;
        let chips = navigation
            .iter()
            .enumerate()
            .map(|(index, link)| BrowseRow {
                id: index.to_string(),
                name: link.title.clone(),
                book_count: link.number_of_items.unwrap_or_default() as usize,
                downloaded_book_count: 0,
            })
            .collect();

        let mut seen = std::collections::HashSet::new();
        let mut books = self.card_services.empty_group();
        for item in &view.response.publications {
            let Some(source) = self.sources.iter().find(|source| source.id == item.source_id) else {
                continue;
            };
            books.append(self.card_services.group_unique(
                source,
                std::slice::from_ref(&item.publication),
                &view.owned_publications,
                true,
                &mut seen,
                cx,
            ));
        }

        self.location = location;
        self.navigation = Rc::new(navigation);
        self.chips = Rc::new(chips);
        self.books = books;
    }

    fn open_chip(&mut self, id: String, cx: &mut Context<Self>) {
        let Some(link) = id.parse::<usize>().ok().and_then(|index| self.navigation.get(index)).cloned() else {
            return;
        };
        match link.target {
            CatalogLinkTarget::Page { locator } => self.load(Some(locator), cx),
            CatalogLinkTarget::External { url } => cx.open_url(&url),
        }
    }

    fn retry(&mut self, cx: &mut Context<Self>) {
        self.load(self.location.clone(), cx);
    }
}

impl Render for StoreBrowsePage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = ui_components::browser_theme(cx);
        let body = if self.loading {
            ui_components::browser_loading_message("Loading Store…", theme).into_any_element()
        } else if self.chips.is_empty() && self.books.is_empty() {
            ui_components::browser_actionable_empty("Nothing here", "No catalogue books are available here.", gpui::div(), theme).into_any_element()
        } else {
            let page = cx.entity();
            let open_location: browse_grid::OpenLocation = Rc::new(move |id, cx: &mut App| {
                page.update(cx, |page, cx| page.open_chip(id, cx));
            });
            browse_grid::render(
                format!("store-contents-{}", self.location.as_deref().unwrap_or("root")).into(),
                self.chips.clone(),
                self.books.spanned_views(),
                cx.entity(),
                open_location,
            )
        };

        let page = cx.entity();
        let body = ui_components::browser_error_banner(self.error.clone(), "Retry".into(), theme, body, move |cx| {
            page.update(cx, |page, cx| page.retry(cx));
        });

        ui_components::full_size_column()
            .p(rems(1.0))
            .child(body)
    }
}
