//! Data-source boundary for the shared browse page.

use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

use gpui::{AnyElement, App, Entity, Render, Window};
use library_model::{BrowseChipSort, BrowseRow};

use super::controls::{BrowseBookOrder, BrowseControls, BrowseFormatFilter};

pub(crate) type BrowseFuture<T> = Pin<Box<dyn Future<Output = T> + 'static>>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BrowseQuery {
    pub(crate) location: String,
    pub(crate) search: String,
    pub(crate) formats: Vec<BrowseFormatFilter>,
    pub(crate) chip_sort: Option<BrowseChipSort>,
    pub(crate) book_sort: Option<BrowseBookOrder>,
    pub(crate) languages: Vec<&'static str>,
    pub(crate) hide_finished: bool,
    pub(crate) include_direct_child_books: bool,
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
    fn library_context(&self) -> Option<&crate::library::services::LibraryContext> {
        None
    }
    fn can_download_child(_child: &BrowseRow) -> bool {
        false
    }
    fn download_child(&self, _query: &BrowseQuery) -> Option<BrowseFuture<Result<(), String>>> {
        None
    }
    type Response: 'static;
    type Book: Render + 'static;

    /// Interactive review forms refresh explicitly so background writes cannot
    /// replace their inputs, search results, focus, or pagination.
    fn refresh_on_library_changes(&self) -> bool {
        true
    }

    fn request(&self, query: &BrowseQuery) -> BrowseRequest<Self::Response>;
    /// Installs returned data and optionally supplies the authoritative path
    /// for the requested location.
    fn install(&mut self, response: Self::Response, cx: &mut App) -> Option<Vec<BrowseCrumb>>;
    fn child_icon(&self) -> ui_components::NavigationIcon;
    fn children(&self) -> Rc<Vec<BrowseRow>>;
    fn chip_groups(&self) -> &[library_model::BrowseChipGroup] {
        &[]
    }
    /// Real folder parents remain in the trail; presentation-only subject groups do not.
    fn include_group_in_path(&self) -> bool {
        true
    }
    fn books(&self) -> Rc<Vec<Entity<Self::Book>>>;
    fn controls(&self) -> &BrowseControls;
    fn minimum_search_length() -> usize {
        1
    }
    fn child_count(child: &library_model::BrowseRow) -> Option<usize> {
        Self::show_child_counts().then_some(child.book_count)
    }
    fn show_child_counts() -> bool {
        true
    }
    fn custom_body(&self) -> Option<AnyElement> {
        None
    }
    fn prepare_book(book: &Entity<Self::Book>, window: &mut Window, cx: &mut App);
    fn activate_book(_book: &Entity<Self::Book>, _window: &mut Window, _cx: &mut App) {}
    /// Tells a book whether it holds the keyboard cursor. The paginator owns
    /// where the cursor is; the card owns what focused looks like. Adapters
    /// whose cards have no focused state can leave this alone.
    fn set_book_focused(_book: &Entity<Self::Book>, _focused: bool, _cx: &mut App) {}
    /// Tells a book whether a rubber band has picked it up. Drawn like the
    /// cursor — the two never coexist, since moving the cursor drops the
    /// selection — but tracked apart, because only a selected card hands its
    /// secondary click to the page.
    fn set_book_selected(_book: &Entity<Self::Book>, _selected: bool, _cx: &mut App) {}
    /// Tells a book that a rubber band is sweeping. A card drops its hover style
    /// while one is, so the band is the only thing reporting what it covers
    /// rather than every card the pointer crosses flashing in turn.
    fn set_book_marquee(_book: &Entity<Self::Book>, _marquee: bool, _cx: &mut App) {}
    /// Whether this adapter's books can be drawn detailed — twice as wide, with
    /// the blurb beside the cover. Adapters whose cards have no such mode leave
    /// this alone, and their pages offer no switch.
    fn supports_detailed_cards() -> bool {
        false
    }
    /// Tells a book which of the two card modes it is being drawn in. The page
    /// widens the cells; the card decides what to put in one.
    fn set_book_detailed(_book: &Entity<Self::Book>, _detailed: bool, _cx: &mut App) {}
    /// Identifies a book for the commands a selection can run.
    fn book_content_hash(_book: &Entity<Self::Book>, _cx: &App) -> Option<sync_common::ContentHash> {
        None
    }
    fn book_title(_book: &Entity<Self::Book>, _cx: &App) -> String {
        String::new()
    }
    fn path_to(&self, _child: &BrowseRow) -> Result<Option<Vec<BrowseCrumb>>, String> {
        Ok(None)
    }
    fn has_cached_book_detail(&self, _content_hash: sync_common::ContentHash, _cx: &App) -> bool {
        false
    }
    fn prepare_book_detail(&mut self, _book: library_model::BookCardRow, _cover: Option<crate::library::widgets::LoadedCover>) {}
    fn clear_cached_book_detail(&mut self) {}
    fn show_cached_book_detail(&mut self) {}
    fn hide_cached_book_detail(&mut self, _cx: &mut App) {}
}
