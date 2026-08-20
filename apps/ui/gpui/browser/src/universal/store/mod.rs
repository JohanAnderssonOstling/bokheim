//! Catalogue browsing and import.
//!
//! Route-scoped: sources, browsing state, and cover work are discarded when
//! leaving the store. The selected library id is captured when entering.

mod browse;
mod search;
mod view;

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use epub_svelte_lib::LibraryBackend;
use epub_svelte_lib::catalog::{CatalogFacet, CatalogLink, CatalogLinkTarget, CatalogPagination, CatalogSearch, CatalogSource, CatalogSourceId, CatalogSubjectBrowseResponse};
use gpui::prelude::*;
use gpui::{AnyElement, Context, Div, Entity, FontWeight, IntoElement, Render, SharedString, Subscription, Task, Window, div, px};
use gpui_component::Disableable;
use gpui_component::IconName;
use gpui_component::input::{InputEvent, InputState};
use ui_components as components;

use crate::catalog::{CatalogCardGroup, CatalogCardServices};
use crate::services::AppServices;
use crate::stores::Preferences;

const STORE_GRID_GAP: f32 = 16.0;

fn toggled_catalog_languages(included: &[String], value: &str) -> Vec<String> {
    if included.is_empty() {
        return vec![value.to_owned()];
    }
    let mut selected = included.to_vec();
    if selected.iter().any(|language| language == value) {
        selected.retain(|language| language != value);
    } else {
        selected.push(value.to_owned());
    }
    selected.sort();
    selected.dedup();
    selected
}

fn catalog_language_available(source_languages: impl IntoIterator<Item = impl AsRef<str>>, value: &str, label: &str) -> bool {
    let value = value.to_ascii_lowercase();
    let label = label.to_lowercase();
    source_languages.into_iter().any(|language| {
        let candidate = language.as_ref().trim().replace('_', "-").to_lowercase();
        candidate == value || candidate == label || candidate.strip_prefix(&value).is_some_and(|suffix| suffix.starts_with('-'))
    })
}

enum StoreMode {
    Catalogues,
    Subjects(SubjectBrowseView),
    WideSearch(WideSearchView),
    Catalogue(CatalogueView),
}

struct SubjectBrowseView {
    response: CatalogSubjectBrowseResponse,
    owned_publications: std::collections::HashSet<epub_svelte_lib::catalog::CatalogPublicationKey>,
    levels: Vec<(String, Option<String>)>,
    cards: CatalogCardGroup,
}

struct WideSearchView {
    query: String,
    sections: Vec<WideSearchResultSection>,
    statuses: Vec<WideSearchStatus>,
}

struct WideSearchResultSection {
    title: String,
    sources: Vec<WideSearchSourceResults>,
}

struct WideSearchSourceResults {
    source: CatalogSource,
    cards: CatalogCardGroup,
    external_links: Vec<CatalogLink>,
}

struct WideSearchStatus {
    source: CatalogSource,
    outcome: WideSearchStatusKind,
    external_links: Vec<CatalogLink>,
}

enum WideSearchStatusKind {
    NoMatches,
    Unsupported,
    TimedOut,
    Failed(String),
}

struct CatalogueView {
    snapshot: epub_svelte_lib::catalog::CatalogPage,
    source: CatalogSource,
    search: Option<CatalogSearch>,
    /// The query these results answer, when they came from searching rather
    /// than browsing. Retained so the applied-filter line can name it and
    /// clearing it can return to the level the search started from.
    query: Option<String>,
    primary_section_title: Option<String>,
    levels: Vec<(String, String)>,
    navigation: Vec<CatalogLink>,
    facets: Vec<CatalogFacet>,
    pagination: CatalogPagination,
    owned_publications: std::collections::HashSet<epub_svelte_lib::catalog::CatalogPublicationKey>,
    publications: CatalogCardGroup,
    groups: Vec<CatalogueGroupView>,
}

struct CatalogueGroupView {
    title: String,
    page_locator: Option<String>,
    number_of_items: u32,
    navigation: Vec<CatalogLink>,
    cards: CatalogCardGroup,
}

pub(crate) struct StorePage {
    services: AppServices,
    preferences: Entity<Preferences>,
    library: LibraryBackend,
    sources: Arc<Vec<CatalogSource>>,
    mode: StoreMode,
    cards: CatalogCardServices,
    loading: bool,
    error: Option<SharedString>,
    task: Option<Task<()>>,
    enrichment_refresh_task: Option<Task<()>>,
    search_input: Entity<InputState>,
    wide_search_input: Entity<InputState>,
    subject_search_input: Entity<InputState>,
    subject_source_ids: Vec<CatalogSourceId>,
    _search_subscription: Subscription,
    _wide_search_subscription: Subscription,
    _subject_search_subscription: Subscription,
    virtual_rows: HashMap<(usize, usize, u32, u32), view::StoreVirtualRows>,
    catalog_link_columns: Cell<usize>,
}

impl StorePage {
    pub(crate) fn new(services: AppServices, preferences: Entity<Preferences>, library: LibraryBackend, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search_input = cx.new(|cx| InputState::new(window, cx).placeholder("Catalogue search"));
        let wide_search_input = cx.new(|cx| InputState::new(window, cx).placeholder("Catalogue search"));
        let subject_search_input = cx.new(|cx| InputState::new(window, cx).placeholder("Search subjects"));
        let search_subscription = cx.subscribe(&search_input, |page: &mut Self, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                page.search_within_source(cx);
            }
        });
        let wide_search_subscription = cx.subscribe(&wide_search_input, |page: &mut Self, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                page.search_catalogs(cx);
            }
        });
        let subject_search_subscription = cx.subscribe(&subject_search_input, |page: &mut Self, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                page.search_subjects(cx);
            }
        });
        let import_library = library.clone();
        let import = Rc::new(move |request| Ok(import_library.spawn(move |library| async move { library.import_catalog_book(epub_svelte_lib::ROOT_DIR_ID, &request).await.map(|_| ()).map_err(|error| error.to_string()) })));
        let cards = CatalogCardServices::new(&services, import);
        let mut page = Self {
            services,
            preferences,
            library,
            sources: Arc::new(Vec::new()),
            mode: StoreMode::Catalogues,
            cards,
            loading: false,
            error: None,
            task: None,
            enrichment_refresh_task: None,
            search_input,
            wide_search_input,
            subject_search_input,
            subject_source_ids: Vec::new(),
            _search_subscription: search_subscription,
            _wide_search_subscription: wide_search_subscription,
            _subject_search_subscription: subject_search_subscription,
            virtual_rows: HashMap::new(),
            catalog_link_columns: Cell::new(1),
        };
        page.refresh(cx);
        page
    }

    fn set_mode(&mut self, mode: StoreMode) {
        self.mode = mode;
        self.virtual_rows.clear();
    }

    fn update_catalog_languages(&mut self, languages: Vec<String>, cx: &mut Context<Self>) {
        match self.preferences.update(cx, |preferences, cx| preferences.update_browsing(epub_svelte_lib::app::preferences::BrowsingPreferencePatch::IncludedCatalogLanguages(languages), cx)) {
            Ok(()) => {
                self.sources = Arc::new(self.services.backend.catalog_sources());
                if matches!(self.mode, StoreMode::WideSearch(_)) {
                    self.search_catalogs(cx);
                }
            }
            Err(error) => self.error = Some(format!("Could not save language filter: {error}").into()),
        }
        cx.notify();
    }

    fn toggle_catalog_language(&mut self, value: &str, cx: &mut Context<Self>) {
        let browsing = self.preferences.read(cx).browsing();
        let selected = toggled_catalog_languages(browsing.included_catalog_languages(), value);
        self.update_catalog_languages(selected, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::{catalog_language_available, toggled_catalog_languages};

    #[test]
    fn catalogue_languages_toggle_as_a_persisted_set() {
        assert_eq!(toggled_catalog_languages(&[], "en"), ["en"]);
        assert_eq!(toggled_catalog_languages(&["en".to_owned()], "sv"), ["en", "sv"]);
        assert_eq!(toggled_catalog_languages(&["en".to_owned(), "sv".to_owned()], "en"), ["sv"]);
        assert!(toggled_catalog_languages(&["en".to_owned()], "en").is_empty());
    }

    #[test]
    fn catalogue_language_options_require_an_available_source_language() {
        assert!(catalog_language_available(["English"], "en", "English"));
        assert!(catalog_language_available(["sv-SE"], "sv", "Swedish"));
        assert!(!catalog_language_available(["French"], "de", "German"));
        assert!(!catalog_language_available(["Multilingual"], "en", "English"));
    }
}
