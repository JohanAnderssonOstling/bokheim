//! The EPUB reader surface.

pub(crate) mod marks;
pub(crate) mod progress;
pub(crate) mod render;
pub(crate) mod search;
pub(crate) mod state;
pub(crate) mod toc;
pub(crate) mod toolbar;

use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use bokheim_backend::LibraryBackend;
use gpui::{Entity, FocusHandle, Pixels, Point, Subscription, Task};
use gpui_component::input::InputState;
use gpui_component::tree::TreeState;
use html_view_gpui::HtmlView;
use library_model::BookLocator;

use crate::epub::toc::TocState;
use crate::shell::marks::MarkSession;
use crate::shell::persistence::PositionWriter;
use crate::shell::state_panel::SidebarState;

enum LoadState {
    Loading,
    Ready(Entity<HtmlView>),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Selection {
    cfi_range: String,
    text: String,
}

#[derive(Debug, PartialEq)]
struct ProgressState {
    fraction: f32,
    doc_fraction: f32,
    location: u64,
    total_locations: u64,
    section: usize,
    section_count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ProgressSummary {
    percent: u8,
    location: u64,
    total_locations: u64,
    section: usize,
    section_count: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct SearchFilters {
    match_case: bool,
    whole_word: bool,
    match_diacritics: bool,
    whole_book: bool,
}

struct SearchState {
    /// `None` while search is closed; `Some` owns the active query.
    query: Option<String>,
    applied_query: String,
    generation: u64,
    cancellation: Arc<AtomicU64>,
    debounce: Option<Task<()>>,
    index_task: Option<Task<()>>,
    filters: SearchFilters,
    match_position: Option<(usize, usize)>,
}

pub(crate) struct ReaderView {
    locator: BookLocator,
    library: Rc<LibraryBackend>,
    book_title: String,
    title: String,
    load_state: LoadState,
    desired_column_count: u8,
    toc: TocState,
    toc_tree: Entity<TreeState>,
    sidebar: SidebarState,
    chrome: crate::shell::chrome::ChromeState,
    search_input: Entity<InputState>,
    search: SearchState,
    publication: Arc<crate::publication::NativePublication>,
    current_cfi: Option<String>,
    progress: ProgressState,
    marks: MarkSession,
    /// The text the reader has selected, which only an EPUB anchors by CFI.
    selection: Option<Selection>,
    annotation_note_input: Entity<InputState>,
    annotation_popup_anchor: Option<Point<Pixels>>,
    footnote: Option<(html_view_core::FootnotePreview, html_view_gpui::PreparedNote)>,
    image_preview: Option<(String, Arc<gpui::Image>)>,
    focus: FocusHandle,
    renderer_subscription: Option<Subscription>,
    reading_positions: PositionWriter<(String, f32)>,
    return_to_library: crate::ReturnToLibrary,
    _search_subscription: Subscription,
}
