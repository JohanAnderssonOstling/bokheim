use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use bokheim_ui_runtime::BlockingWorker;
use epub_svelte_lib::BookLocator;
use epub_svelte_lib::library::Library;
use gpui::{App, Entity, FocusHandle, Global, Subscription, Task};
use gpui_component::input::InputState;
use gpui_component::tree::TreeState;
use html_view_core::TocEntry;
use html_view_gpui::HtmlView;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ReaderWindowId(pub(crate) u64);

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ReaderPreferences {
    pub font_size: f32,
    pub column_width: f64,
    pub scale: f64,
    pub line_numbers: bool,
    pub toc_width: f32,
}

impl Default for ReaderPreferences {
    fn default() -> Self {
        Self { font_size: 20.0, column_width: 600.0, scale: 1.0, line_numbers: true, toc_width: 300.0 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ReaderWindowGeometry {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

pub type SaveReaderSettings = Rc<dyn Fn(ReaderPreferences, Option<ReaderWindowGeometry>, &mut App)>;

pub(crate) struct ReaderSettings {
    pub(crate) preferences: ReaderPreferences,
    pub(crate) geometry: Option<ReaderWindowGeometry>,
    pub(crate) save: Option<SaveReaderSettings>,
    pub(crate) blocking_worker: Option<BlockingWorker>,
    pub(crate) persistence_worker: Option<BlockingWorker>,
}

impl Default for ReaderSettings {
    fn default() -> Self {
        Self { preferences: ReaderPreferences::default(), geometry: None, save: None, blocking_worker: None, persistence_worker: None }
    }
}

impl Global for ReaderSettings {}

pub(crate) enum LoadState {
    Loading,
    Ready(Entity<HtmlView>),
    Failed,
}

pub(crate) struct ReaderView {
    pub(crate) id: ReaderWindowId,
    pub(crate) locator: BookLocator,
    pub(crate) book_title: String,
    pub(crate) title: String,
    pub(crate) load_state: LoadState,
    pub(crate) load_generation: u64,
    pub(crate) toc_entries: Arc<Vec<TocEntry>>,
    pub(crate) toc_tree: Entity<TreeState>,
    pub(crate) toc_resolutions: HashMap<String, (usize, Option<String>)>,
    pub(crate) active_toc_link: Option<String>,
    pub(crate) toc_visible: bool,
    pub(crate) search_input: Entity<InputState>,
    pub(crate) search_query: String,
    pub(crate) applied_search_query: String,
    pub(crate) search_generation: u64,
    pub(crate) search_task: Option<Task<()>>,
    pub(crate) search_match: Option<(usize, usize)>,
    pub(crate) search_active: bool,
    pub(crate) can_history_back: bool,
    pub(crate) can_history_forward: bool,
    pub(crate) unavailable: bool,
    pub(crate) focus: FocusHandle,
    pub(crate) renderer_subscription: Option<Subscription>,
    pub(crate) reading_position_tx: async_channel::Sender<String>,
    pub(crate) _subscriptions: Vec<Subscription>,
}

pub(crate) struct PendingReader {
    pub(crate) id: ReaderWindowId,
    pub(crate) locator: BookLocator,
    pub(crate) title: String,
    pub(crate) library: Arc<Library>,
    pub(crate) bytes: Vec<u8>,
    pub(crate) nav_state: Option<String>,
}
