//! The EPUB reader window.

pub(crate) mod audio;
pub(crate) mod marks;
pub(crate) mod progress;
pub(crate) mod render;
pub(crate) mod search;
pub(crate) mod state;
pub(crate) mod toc;
pub(crate) mod toolbar;

use std::sync::Arc;

use epub_svelte_lib::{BookLocator, LibraryBackend};
use gpui::{Entity, FocusHandle, Pixels, Point, Subscription};
use gpui_component::input::InputState;
use gpui_component::tree::TreeState;
use html_view_gpui::HtmlView;

use crate::epub::audio::AudioState;
use crate::epub::marks::Selection;
use crate::shell::marks::MarksState;
use crate::epub::progress::ProgressState;
use crate::epub::search::SearchState;
use crate::epub::toc::TocState;
use crate::shell::persistence::PositionWriter;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ReaderWindowId(pub(crate) u64);

pub(crate) enum LoadState {
    Loading,
    Ready(Entity<HtmlView>),
    Failed,
}

pub(crate) struct ReaderView {
    pub(crate) id: ReaderWindowId,
    pub(crate) locator: BookLocator,
    pub(crate) library: LibraryBackend,
    pub(crate) book_title: String,
    pub(crate) title: String,
    pub(crate) load_state: LoadState,
    pub(crate) load_generation: u64,
    pub(crate) desired_column_count: u8,
    pub(crate) toc: TocState,
    pub(crate) toc_tree: Entity<TreeState>,
    pub(crate) toc_visible: bool,
    /// Whether the toolbar is currently out of the way.
    ///
    /// Turning a page hides it; moving the pointer into the top strip brings it
    /// back. Deliberately not animated — a reading surface should not have
    /// something sliding about in the corner of the eye while you read.
    pub(crate) chrome_hidden: bool,
    pub(crate) search_input: Entity<InputState>,
    pub(crate) search: SearchState,
    pub(crate) publication: Arc<crate::publication::NativePublication>,
    pub(crate) current_cfi: Option<String>,
    pub(crate) progress: ProgressState,
    pub(crate) toolbar_view: Entity<toolbar::ReaderToolbar>,
    pub(crate) marks: MarksState,
    /// The text the reader has selected, which only an EPUB anchors by CFI.
    pub(crate) selection: Option<Selection>,
    pub(crate) reader_state_visible: bool,
    pub(crate) reader_state_bookmarks: bool,
    pub(crate) annotation_note_input: Entity<InputState>,
    pub(crate) annotation_popup_anchor: Option<Point<Pixels>>,
    pub(crate) footnote: Option<(html_view_core::FootnotePreview, html_view_gpui::PreparedNote)>,
    pub(crate) image_preview: Option<(String, Arc<gpui::Image>)>,
    pub(crate) audio: AudioState,
    pub(crate) can_history_back: bool,
    pub(crate) can_history_forward: bool,
    pub(crate) unavailable: bool,
    pub(crate) focus: FocusHandle,
    pub(crate) renderer_subscription: Option<Subscription>,
    pub(crate) reading_positions: PositionWriter<(String, f32)>,
    pub(crate) _subscriptions: Vec<Subscription>,
}

pub(crate) struct PendingReader {
    pub(crate) id: ReaderWindowId,
    pub(crate) locator: BookLocator,
    pub(crate) title: String,
    pub(crate) library: LibraryBackend,
    pub(crate) publication: Arc<crate::publication::NativePublication>,
    pub(crate) nav_state: Option<String>,
    pub(crate) initial_target: Option<String>,
}
