//! On-demand Store adapter for the parallel browse page.

use std::rc::Rc;
use std::sync::Arc;

use epub_svelte_lib::app::CatalogSubjectBrowseView;
use epub_svelte_lib::catalog::{CatalogLinkTarget, CatalogSource, CatalogSubjectBrowseRequest};
use epub_svelte_lib::{BrowseRow, LibraryBackend};
use gpui::{Context, Entity};

use super::browse_page::{BrowseAdapter, BrowsePage, BrowseQuery, BrowseRequest};
use crate::catalog::{CatalogCardGroup, CatalogCardServices, CatalogPublicationCardView};
use crate::services::AppServices;

pub(crate) struct StoreBrowseAdapter {
    library: LibraryBackend,
    sources: Arc<Vec<CatalogSource>>,
    card_services: CatalogCardServices,
    children: Rc<Vec<BrowseRow>>,
    books: CatalogCardGroup,
}

impl StoreBrowseAdapter {
    fn new(services: AppServices, library: LibraryBackend) -> Self {
        let sources = Arc::new(services.backend.catalog_sources());
        let import_library = library.clone();
        let import = Rc::new(move |request| Ok(import_library.spawn(move |library| async move { library.import_catalog_book(epub_svelte_lib::ROOT_DIR_ID, &request).await.map(|_| ()).map_err(|error| error.to_string()) })));
        let card_services = CatalogCardServices::new(&services, import);
        let books = card_services.empty_group();
        Self { library, sources, card_services, children: Rc::new(Vec::new()), books }
    }
}

impl BrowseAdapter for StoreBrowseAdapter {
    type Response = CatalogSubjectBrowseView;
    type Book = CatalogPublicationCardView;

    fn request(&self, query: &BrowseQuery) -> BrowseRequest<Self::Response> {
        let locator = (!query.location.is_empty()).then(|| query.location.clone());
        let search = query.search.clone();
        BrowseRequest::Async(self.library.spawn(move |library| async move {
            library.browse_catalog_subjects(CatalogSubjectBrowseRequest { locator, supported_languages: Vec::new(), source_ids: Vec::new(), query: search, limit: 100 }).await.map_err(|error| error.to_string())
        }))
    }

    fn install(&mut self, response: Self::Response, cx: &mut Context<BrowsePage<Self>>) {
        let children = response
            .response
            .navigation
            .iter()
            .filter_map(|link| {
                let CatalogLinkTarget::Page { locator } = &link.target else {
                    return None;
                };
                Some(BrowseRow { id: locator.clone(), name: link.title.clone(), book_count: link.number_of_items.unwrap_or_default() as usize, downloaded_book_count: 0 })
            })
            .collect();
        let mut seen = std::collections::HashSet::new();
        let mut books = self.card_services.empty_group();
        for item in &response.response.publications {
            let Some(source) = self.sources.iter().find(|source| source.id == item.source_id) else {
                continue;
            };
            books.append(self.card_services.group_unique(source, std::slice::from_ref(&item.publication), &response.owned_publications, true, &mut seen, cx));
        }
        self.children = Rc::new(children);
        self.books = books;
    }

    fn children(&self) -> Rc<Vec<BrowseRow>> {
        self.children.clone()
    }

    fn books(&self) -> Rc<Vec<(Entity<Self::Book>, usize)>> {
        self.books.spanned_views()
    }
}

pub(crate) fn store(services: AppServices, library: LibraryBackend, cx: &mut Context<BrowsePage<StoreBrowseAdapter>>) -> BrowsePage<StoreBrowseAdapter> {
    BrowsePage::new(StoreBrowseAdapter::new(services, library), String::new(), "Store", Vec::new(), "store", None, None, |_| {}, cx)
}
