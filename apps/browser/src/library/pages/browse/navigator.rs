//! Where a browse page is and what it lists there: the location, the sort and
//! filter choices, the installed listing, and the request that replaces it.
//! The page's views read it and redraw when it loads; none of them load
//! anything themselves.

use gpui::{Context, EventEmitter, SharedString, Task};
use library_backend::{DownloadState, LibraryBrowseData};
use library_model::BrowseLocation;

use super::controls::{BrowseOptionsEvent, BrowseOptionsSelection};
use super::listing::{BrowseCrumb, BrowseDestination, BrowseItem, BrowseKind, BrowseListing, chip_id};
use crate::library::services::{LibraryContentsChanged, LibraryContext, LibraryDownloadChanged};

/// A listing was installed.
pub(super) struct Loaded {
    /// The location the page was on before, when this load moved it.
    pub(super) left: Option<BrowseLocation>,
    /// Whether this load refreshed the same contents rather than replacing them.
    pub(super) refresh: bool,
}

pub(super) struct BrowseNavigator {
    ui: LibraryContext,
    pub(super) kind: BrowseKind,
    pub(super) destination: BrowseDestination,
    pub(super) listing: BrowseListing,
    pub(super) selection: BrowseOptionsSelection,
    pub(super) error: Option<SharedString>,
    /// The load in flight. Replacing it drops the one before, which cancels it.
    request: Option<Task<()>>,
    /// A feed's next page being fetched, to append. A load drops it.
    more: Option<Task<()>>,
    _subscriptions: Vec<gpui::Subscription>,
}

impl EventEmitter<Loaded> for BrowseNavigator {}

impl BrowseNavigator {
    pub(super) fn new(ui: LibraryContext, kind: BrowseKind, listing: BrowseListing, cx: &mut Context<Self>) -> Self {
        let selection = BrowseOptionsSelection::for_controls(&listing.controls);
        let updates = ui.updates();
        let subscriptions = vec![
            cx.subscribe(&updates, |navigator, _, _: &LibraryContentsChanged, cx| navigator.refresh(cx)),
            cx.subscribe(&updates, |navigator, _, event: &LibraryDownloadChanged, cx| {
                if matches!(event.status.state, DownloadState::Downloaded | DownloadState::NotDownloaded) {
                    navigator.refresh(cx);
                }
            }),
        ];
        let mut navigator = Self { ui, kind, destination: BrowseDestination::from_path(kind, Vec::new()), listing, selection, error: None, request: None, more: None, _subscriptions: subscriptions };
        navigator.reload(cx);
        navigator
    }

    pub(super) fn reload(&mut self, cx: &mut Context<Self>) {
        self.load(self.destination.clone(), false, cx);
    }

    /// Refreshes scanner-driven content without replacing the current page by
    /// a loading frame. Newly discovered covers can prepare in place while the
    /// already-visible book cards remain painted.
    /// A feed page is fetched from the feed, which a library change does not
    /// touch, and a refresh would drop the pages appended to it.
    pub(super) fn refresh(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.destination.location, BrowseLocation::Opds { .. }) {
            self.load(self.destination.clone(), true, cx);
        }
    }

    pub(super) fn open_path(&mut self, path: Vec<BrowseCrumb>, cx: &mut Context<Self>) {
        self.load(BrowseDestination::from_path(self.kind, path), false, cx);
    }

    /// Searches for `query`, or drops the search for an empty one.
    pub(super) fn search(&mut self, query: String, cx: &mut Context<Self>) {
        self.selection.search = query;
        self.reload(cx);
    }

    pub(super) fn select_option(&mut self, event: BrowseOptionsEvent, cx: &mut Context<Self>) {
        // A feed's own choice is another page of the feed, in place of this one.
        if let BrowseOptionsEvent::OpenFeedPage(location) = event {
            let mut path = self.destination.path.clone();
            if let Some(crumb) = path.last_mut() {
                crumb.location = location;
            }
            self.open_path(path, cx);
            return;
        }
        if self.selection.apply(event, &self.listing.controls) {
            self.reload(cx);
        }
    }

    /// Fetches the feed's next page and appends it to the listing, unless one
    /// is already on its way.
    pub(super) fn load_more(&mut self, cx: &mut Context<Self>) {
        let Some(next) = self.listing.next else { return };
        if self.more.is_some() {
            return;
        }
        // The next page's address already carries any search.
        let mut query = self.selection.backend_query(next, true);
        query.search.clear();
        let ui = self.ui.clone();
        self.more = Some(cx.spawn(async move |navigator, cx| {
            let result = fetch(&ui, query).await;
            let _ = navigator.update(cx, |navigator, cx| {
                navigator.more = None;
                match result {
                    // A load since has replaced the listing this page continued.
                    Ok(_) if navigator.listing.next != Some(next) => {}
                    Ok(data) => {
                        navigator.listing.append(data, cx);
                        cx.emit(Loaded { left: None, refresh: true });
                    }
                    Err(error) => {
                        log::warn!("Loading the next feed page failed: {error}");
                        navigator.error = Some(error.into());
                    }
                }
                cx.notify();
            });
        }));
    }

    /// The trail to the child `child_id`: under the current path, through its
    /// group where the group is a real folder. The backend supplies the full
    /// displayed breadcrumb trail after the destination loads.
    pub(super) fn child_path(&self, child_id: BrowseLocation) -> Option<Vec<BrowseCrumb>> {
        let item = self.listing.item(&chip_id(child_id))?;
        let child = item.child()?;
        let mut path = self.destination.path.clone();
        if let BrowseItem::Child { parent: Some(parent), .. } = item {
            let parent = self.listing.child(BrowseLocation::Folder(*parent))?;
            path.push(BrowseCrumb { location: parent.id, label: parent.name.clone() });
        }
        path.push(BrowseCrumb { location: child.id, label: child.name.clone() });
        Some(path)
    }

    /// Starts a load in place of any still running; the installed frame
    /// remains painted.
    fn load(&mut self, destination: BrowseDestination, refresh: bool, cx: &mut Context<Self>) {
        let location = destination.location;
        let query = self.selection.backend_query(location, true);
        #[cfg(target_os = "android")]
        log::info!("Browse request: location={} formats={:?} languages={:?} search_length={}", location, query.file_types, query.languages, query.search.len());
        // Navigation keeps the current frame painted until the next one is
        // ready. A browse query is a local read that returns inside a frame,
        // so covering the grid in the meantime only ever produced a flash —
        // one that appeared going back up the tree and not going down it, for
        // the same query against the same database.
        let ui = self.ui.clone();
        let searched = !query.search.is_empty();
        self.error = None;
        // A next page belongs to the listing this load replaces.
        self.more = None;
        self.request = Some(cx.spawn(async move |navigator, cx| {
            let result = fetch(&ui, query).await;
            let _ = navigator.update(cx, |navigator, cx| navigator.finish_request(destination, refresh, searched, result, cx));
        }));
    }

    fn finish_request(&mut self, mut destination: BrowseDestination, refresh: bool, searched: bool, result: Result<LibraryBrowseData, String>, cx: &mut Context<Self>) {
        match result {
            Ok(data) => {
                let left = (self.destination.location != destination.location).then_some(self.destination.location);
                let path = self.listing.install(data, searched, cx);
                // A feed page knows nothing of how it was reached; its trail is
                // the chips followed to it.
                if !matches!(destination.location, BrowseLocation::Opds { .. }) {
                    destination.path = path;
                }
                self.destination = destination;
                self.error = None;
                #[cfg(target_os = "android")]
                log::info!("Browse loaded: location={} children={} books={}", self.destination.location, self.listing.children.len(), self.listing.books.len());
                cx.emit(Loaded { left, refresh });
            }
            Err(error) => {
                log::warn!("Browse failed for {}: {}", destination.location, error);
                self.error = Some(error.into());
            }
        }
        cx.notify();
    }
}

/// A page from where it lives: a feed page from the feed, through the app,
/// and anything else from the library. The catalog's root also lists the
/// account's OPDS feeds, each as a chip with its own section.
async fn fetch(ui: &LibraryContext, query: library_model::LibraryBrowseQuery) -> Result<LibraryBrowseData, String> {
    if matches!(query.location, BrowseLocation::Opds { .. }) {
        return ui.app().browse_opds(query).await;
    }
    let feeds_listed = query.location == BrowseLocation::Catalog(library_model::ROOT_SUBJECT_ROUTE_ID) && query.search.is_empty();
    let mut data = ui.backend().browse(query).await?;
    if feeds_listed {
        // Without its feeds the catalog still lists; the failure is logged.
        let feeds = ui.app().opds_feeds().await.unwrap_or_else(|error| {
            log::warn!("could not list OPDS feeds: {error}");
            Vec::new()
        });
        for feed in feeds {
            let chip = library_model::BrowseRow { id: BrowseLocation::Opds { source: feed.id, page: None }, name: feed.name, path: None, book_count: 0, downloaded_book_count: 0 };
            data.sections.push(library_model::BrowseSection { parent: chip.clone(), children: Vec::new(), books: Vec::new() });
            data.contents.children.push(chip);
        }
    }
    Ok(data)
}
