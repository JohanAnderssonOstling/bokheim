//! On-demand Store adapter for the parallel browse page.

use std::rc::Rc;
use std::time::Duration;

use app_contract::catalog::{CatalogLinkTarget, CatalogPublicationKey, CatalogResultRangeResponse, CatalogSubjectBrowseRequest, CatalogSubjectBrowseView, CatalogSubjectSort};
use app_contract::library::{BrowseFuture, BrowseRow};
use gpui::{App, AppContext, Context, Entity};

use super::adapter::{BrowseAdapter, BrowseQuery, BrowseRequest};
use super::controls::{BrowseBookOrder, BrowseControls, BrowseFormatFilter};
use super::page::{BrowsePage, BrowsePageConfig};
use super::store_card::{StoreBookCardView, StoreBookServices};
use crate::library::services::LibraryContext;

/// A bounded prefetch window. It is intentionally close to a large desktop
/// paginator capacity while leaving enough adjacent cards for smooth paging.
const STORE_RESULT_WINDOW_LIMIT: u32 = 32;

pub(crate) struct StoreBrowseAdapter {
    library: LibraryContext,
    book_services: StoreBookServices,
    controls: BrowseControls,
    children: Rc<Vec<BrowseRow>>,
    books: Rc<Vec<Entity<StoreBookCardView>>>,
    range: CatalogResultRangeResponse,
}

impl StoreBrowseAdapter {
    fn new(library: LibraryContext) -> Self {
        let import_library = library.clone();
        let import = Rc::new(move |request| import_library.backend().import_catalog_book(request));
        let book_services = StoreBookServices::new(library.clone(), import);
        Self { library, book_services, controls: BrowseControls::catalog(), children: Rc::new(Vec::new()), books: Rc::new(Vec::new()), range: CatalogResultRangeResponse::default() }
    }

    fn catalog_request(query: &BrowseQuery) -> CatalogSubjectBrowseRequest {
        let locator = (!query.location.is_empty()).then(|| query.location.clone());
        let sort = match query.book_sort {
            Some(BrowseBookOrder::Catalog(sort)) => sort,
            Some(BrowseBookOrder::Library(_)) | None => CatalogSubjectSort::default(),
        };
        let book_formats = query
            .formats
            .iter()
            .filter_map(|format| match format {
                BrowseFormatFilter::Catalog(format) => Some(*format),
                BrowseFormatFilter::Any | BrowseFormatFilter::Library(_) => None,
            })
            .collect();
        CatalogSubjectBrowseRequest { locator, supported_languages: query.languages.iter().map(|language| (*language).to_owned()).collect(), source_ids: Vec::new(), query: query.search.clone(), sort, book_formats, range: query.range }
    }
}

impl BrowseAdapter for StoreBrowseAdapter {
    type Response = CatalogSubjectBrowseView;
    type Book = StoreBookCardView;

    fn request(&self, query: &BrowseQuery) -> BrowseRequest<Self::Response> {
        let request = Self::catalog_request(query);
        let refresh: BrowseFuture<Result<CatalogSubjectBrowseView, String>> = self.library.backend().refresh_catalog_subjects(request);
        // Ordering, filtering, facets, and navigation are server-authoritative.
        // The client-side catalogue cache is used only to resolve publication
        // payloads named by that current server range.
        BrowseRequest::Async(refresh)
    }

    fn install(&mut self, response: Self::Response, cx: &mut App) {
        self.controls = BrowseControls::catalog_capabilities(&response.response.capabilities);
        self.range = response.response.range;
        let children = response
            .response
            .navigation
            .iter()
            .filter(|_| response.response.range.offset == 0)
            .filter_map(|link| {
                let CatalogLinkTarget::Page { locator } = &link.target else {
                    return None;
                };
                Some(BrowseRow { id: locator.clone(), name: link.title.clone(), path: None, book_count: link.number_of_items.unwrap_or_default() as usize, downloaded_book_count: 0 })
            })
            .collect();
        let owned_publications = response.owned_publications;
        let mut seen = std::collections::HashSet::new();
        let books = response
            .response
            .publications
            .into_iter()
            .filter_map(|item| {
                let key = CatalogPublicationKey { source_id: item.source_id.clone(), publication: item.publication.identity.clone() };
                if owned_publications.contains(&key) || !seen.insert(key) {
                    return None;
                }
                let services = self.book_services.clone();
                Some(cx.new(|cx| StoreBookCardView::new(item.source_id, item.publication, services, cx)))
            })
            .collect();
        self.children = Rc::new(children);
        self.books = Rc::new(books);
    }

    fn children(&self) -> Rc<Vec<BrowseRow>> {
        self.children.clone()
    }

    fn books(&self) -> Rc<Vec<Entity<Self::Book>>> {
        self.books.clone()
    }

    fn controls(&self) -> &BrowseControls {
        &self.controls
    }

    fn request_controls(&self, query: &BrowseQuery) -> Option<BrowseFuture<Result<BrowseControls, String>>> {
        let operation = self.library.backend().catalog_subject_capabilities(Self::catalog_request(query));
        Some(Box::pin(async move { operation.await.map(|capabilities| BrowseControls::catalog_capabilities(&capabilities)) }))
    }

    fn install_controls(&mut self, controls: BrowseControls) {
        self.controls = controls;
    }

    fn minimum_search_length() -> usize {
        3
    }

    fn prepare_book(book: &Entity<Self::Book>, cx: &mut App) {
        book.update(cx, |book, cx| book.prepare(cx));
    }

    fn book_ready(book: &Entity<Self::Book>, cx: &App) -> bool {
        book.read(cx).is_prepared(cx)
    }

    fn initial_cover_timeout() -> Option<Duration> {
        Some(Duration::from_millis(500))
    }

    fn result_window_limit() -> Option<u32> {
        Some(STORE_RESULT_WINDOW_LIMIT)
    }

    fn result_range(&self) -> Option<CatalogResultRangeResponse> {
        Some(self.range)
    }
}

pub(crate) fn store(ui: LibraryContext, cx: &mut Context<BrowsePage<StoreBrowseAdapter>>) -> BrowsePage<StoreBrowseAdapter> {
    // The Store is backed by the remote catalog. Local scanner notifications can be
    // frequent and do not change its inventory; rebuilding the grid for each one also
    // resets cover preparation and produces visible flashes on large pages.
    BrowsePage::new(StoreBrowseAdapter::new(ui), BrowsePageConfig::new("", "Store", "store"), cx)
}
