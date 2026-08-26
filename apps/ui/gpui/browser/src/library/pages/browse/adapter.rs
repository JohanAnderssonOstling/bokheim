//! Data-source boundary for the shared browse page.

use std::rc::Rc;
use std::time::Duration;

use app_contract::catalog::{CatalogResultRange, CatalogResultRangeResponse};
use app_contract::library::{BrowseChipSort, BrowseFuture, BrowseRow};
use gpui::{AnyElement, App, Entity, Render, Window};

use super::controls::{BrowseBookOrder, BrowseControls, BrowseFormatFilter};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BrowseQuery {
    pub(crate) location: String,
    pub(crate) search: String,
    pub(crate) formats: Vec<BrowseFormatFilter>,
    pub(crate) chip_sort: Option<BrowseChipSort>,
    pub(crate) book_sort: Option<BrowseBookOrder>,
    pub(crate) languages: Vec<&'static str>,
    pub(crate) range: CatalogResultRange,
}

pub(crate) enum BrowseRequest<T> {
    Ready(T),
    Async(BrowseFuture<Result<T, String>>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BrowseCrumb {
    pub(crate) location: String,
    pub(crate) label: String,
}

/// Supplies data and item behavior without depending on the page's concrete
/// GPUI context. This keeps adapters independently testable and prevents them
/// from reaching into page state while installing a response.
pub(crate) trait BrowseAdapter: Sized + 'static {
    type Response: 'static;
    type Book: Render + 'static;

    fn request(&self, query: &BrowseQuery) -> BrowseRequest<Self::Response>;
    fn install(&mut self, response: Self::Response, cx: &mut App);
    fn children(&self) -> Rc<Vec<BrowseRow>>;
    fn books(&self) -> Rc<Vec<Entity<Self::Book>>>;
    fn controls(&self) -> &BrowseControls;
    fn request_controls(&self, _query: &BrowseQuery) -> Option<BrowseFuture<Result<BrowseControls, String>>> {
        None
    }
    fn install_controls(&mut self, _controls: BrowseControls) {}
    fn minimum_search_length() -> usize {
        1
    }
    fn custom_body(&self) -> Option<AnyElement> {
        None
    }
    fn prepare_book(book: &Entity<Self::Book>, cx: &mut App);
    fn book_ready(book: &Entity<Self::Book>, cx: &App) -> bool;
    fn activate_book(_book: &Entity<Self::Book>, _window: &mut Window, _cx: &mut App) {}
    fn initial_cover_timeout() -> Option<Duration> {
        None
    }
    /// Enables server-windowed pagination for adapters whose result order is
    /// authoritative remotely.
    fn result_window_limit() -> Option<u32> {
        None
    }
    fn result_range(&self) -> Option<CatalogResultRangeResponse> {
        None
    }
    fn path_to(&self, _child: &BrowseRow) -> Result<Option<Vec<BrowseCrumb>>, String> {
        Ok(None)
    }
}
