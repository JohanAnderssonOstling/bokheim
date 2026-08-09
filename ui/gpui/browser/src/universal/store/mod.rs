//! Catalogue browsing and import.
//!
//! Route-scoped: sources, browsing state, and cover work are discarded when
//! leaving the store. The selected library id is captured when entering.

mod browse;
mod search;
mod view;

use std::rc::Rc;
use std::sync::Arc;

use ui_components as components;
use epub_svelte_lib::LibraryBackend;
use epub_svelte_lib::catalog::{CatalogFacet, CatalogLink, CatalogLinkTarget, CatalogPagination, CatalogSearch, CatalogSource};
use gpui::prelude::*;
use gpui::{AnyElement, Context, Entity, FontWeight, IntoElement, Render, SharedString, Task, Window, div, px};
use gpui_component::Disableable;
use gpui_component::IconName;
use gpui_component::button::Button;
use gpui_component::input::{Input, InputState};

use crate::catalog::{CatalogCardGroup, CatalogCardServices};
use crate::services::AppServices;

const STORE_GRID_GAP: f32 = 16.0;

#[derive(Clone)]
enum StoreMode {
    Catalogues,
    WideSearch(WideSearchView),
    Catalogue(CatalogueView),
}

#[derive(Clone)]
struct WideSearchView {
    query: String,
    groups: Vec<WideSearchGroup>,
}

#[derive(Clone)]
struct WideSearchGroup {
    source: CatalogSource,
    outcome: WideSearchOutcome,
}

#[derive(Clone)]
enum WideSearchOutcome {
    Results { cards: CatalogCardGroup, external_links: Vec<CatalogLink> },
    Unsupported,
    TimedOut,
    Failed(String),
}

#[derive(Clone)]
struct CatalogueView {
    source: CatalogSource,
    search: Option<CatalogSearch>,
    levels: Vec<(String, String)>,
    navigation: Vec<CatalogLink>,
    facets: Vec<CatalogFacet>,
    pagination: CatalogPagination,
    publications: CatalogCardGroup,
    groups: Vec<CatalogueGroupView>,
}

#[derive(Clone)]
struct CatalogueGroupView {
    title: String,
    page_locator: Option<String>,
    number_of_items: u32,
    navigation: Vec<CatalogLink>,
    cards: CatalogCardGroup,
}

pub(crate) struct StorePage {
    services: AppServices,
    library: LibraryBackend,
    sources: Arc<Vec<CatalogSource>>,
    mode: StoreMode,
    cards: CatalogCardServices,
    loading: bool,
    error: Option<SharedString>,
    task: Option<Task<()>>,
    search_input: Entity<InputState>,
    wide_search_input: Entity<InputState>,
}

impl StorePage {
    pub(crate) fn new(services: AppServices, library: LibraryBackend, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search_input = cx.new(|cx| InputState::new(window, cx));
        let wide_search_input = cx.new(|cx| InputState::new(window, cx));
        let import_library = library.clone();
        let import = Rc::new(move |request| Ok(import_library.spawn(move |library| async move { library.import_catalog_book("__root__", &request).await.map(|_| ()).map_err(|error| error.to_string()) })));
        let cards = CatalogCardServices::new(&services, import);
        Self { services, library, sources: Arc::new(Vec::new()), mode: StoreMode::Catalogues, cards, loading: false, error: None, task: None, search_input, wide_search_input }
    }
}
