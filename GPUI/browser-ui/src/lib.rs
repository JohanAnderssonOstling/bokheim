//! GPUI library-browsing window.

mod authors;
mod browser_actions;
mod cloud;
mod covers;
mod data;
mod folder_view;
mod home;
mod model;
mod view;

use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;
use std::sync::Arc;

use bokheim_ui_runtime::BlockingWorker;
use epub_svelte_lib::app_core::{AccountStatus, AppCore, LibrarySyncStatus};
use epub_svelte_lib::home::LibraryEntry;
use epub_svelte_lib::library::{AuthorSummary, Library};
use epub_svelte_lib::preferences::BrowsingPreferences;
use epub_svelte_lib::scanner::LibraryEvent;
use epub_svelte_lib::{BookId, BookLocator, LibraryTransfers};
use gpui::prelude::*;
use gpui::{App, Context, Entity, FocusHandle, Image, SharedString, Subscription, Task, Window, actions};
use gpui_component::input::{InputEvent, InputState};

use model::{AuthorGroup, BookCard, HomeSection};
pub use model::{BrowserSection, BrowserSession, BrowserWindowGeometry, FolderCrumb};

actions!(bokheim_browser, [FocusSearch, ShowHome, ShowAuthors, ShowFolders, ShowSearch, ShowSync, ShowSettings, DismissSettings]);

const COVER_CACHE_LIMIT: usize = 96;
const COVER_CACHE_BYTES: usize = 64 * 1024 * 1024;

pub type ResolveLibrary = Rc<dyn Fn(&str) -> Option<Arc<Library>>>;
pub type OpenReader = Rc<dyn Fn(BookLocator, String, &mut App)>;
pub type SaveBrowserState = Rc<dyn Fn(BrowserSession, Option<BrowserWindowGeometry>, &mut App)>;

pub struct BrowserView {
    pub(crate) libraries: Vec<LibraryEntry>,
    pub(crate) resolve_library: ResolveLibrary,
    pub(crate) open_reader: OpenReader,
    pub(crate) save_state: SaveBrowserState,
    pub(crate) blocking_worker: BlockingWorker,
    pub(crate) app_core: AppCore,
    pub(crate) browsing_preferences: BrowsingPreferences,
    pub(crate) account_status: AccountStatus,
    pub(crate) sync_statuses: Arc<Vec<LibrarySyncStatus>>,
    pub(crate) transfer_groups: Arc<Vec<LibraryTransfers>>,
    pub(crate) account_username_input: Entity<InputState>,
    pub(crate) account_password_input: Entity<InputState>,
    pub(crate) cloud_busy: bool,
    pub(crate) cloud_message: Option<SharedString>,
    pub(crate) cloud_error: Option<SharedString>,
    pub(crate) cloud_task: Option<Task<()>>,
    pub(crate) active_library_id: Option<String>,
    pub(crate) active_library: Option<Arc<Library>>,
    pub(crate) section: BrowserSection,
    pub(crate) settings_open: bool,
    pub(crate) folder_path: Vec<FolderCrumb>,
    pub(crate) folder_directories: Arc<Vec<FolderCrumb>>,
    pub(crate) books: Arc<Vec<BookCard>>,
    pub(crate) home_sections: Arc<Vec<HomeSection>>,
    pub(crate) authors: Arc<Vec<AuthorSummary>>,
    pub(crate) author_groups: Arc<Vec<AuthorGroup>>,
    pub(crate) selected_author: Option<String>,
    pub(crate) search_input: Entity<InputState>,
    pub(crate) author_filter_input: Entity<InputState>,
    pub(crate) new_folder_input: Entity<InputState>,
    pub(crate) search_active: bool,
    pub(crate) focus: FocusHandle,
    pub(crate) cover_cache: HashMap<BookId, Arc<Image>>,
    pub(crate) cover_order: VecDeque<BookId>,
    pub(crate) cover_costs: HashMap<BookId, usize>,
    pub(crate) cover_cache_bytes: usize,
    pub(crate) pending_cover_ids: HashSet<BookId>,
    pub(crate) loading_cover_ids: HashSet<BookId>,
    pub(crate) missing_cover_ids: HashSet<BookId>,
    pub(crate) cover_load_running: bool,
    pub(crate) cover_generation: u64,
    pub(crate) data_generation: u64,
    pub(crate) data_task: Option<Task<()>>,
    pub(crate) pending_thumbnail_ids: HashSet<BookId>,
    pub(crate) thumbnail_reload_scheduled: bool,
    pub(crate) refresh_scheduled: bool,
    pub(crate) _subscriptions: Vec<Subscription>,
}

impl BrowserView {
    pub fn new(
        libraries: Vec<LibraryEntry>, session: BrowserSession, resolve_library: ResolveLibrary, open_reader: OpenReader, save_state: SaveBrowserState, blocking_worker: BlockingWorker, app_core: AppCore, window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let active_library_id = session.active_library_id.filter(|id| libraries.iter().any(|entry| entry.library_id().as_str() == id)).or_else(|| libraries.first().map(|entry| entry.library_id().as_str().to_owned()));
        let section = match session.section {
            BrowserSection::Settings => BrowserSection::Folders,
            section => section,
        };
        let browsing_preferences = app_core.browsing_preferences().unwrap_or_else(|error| {
            log::error!("failed to load browsing preferences: {error}");
            BrowsingPreferences::default()
        });
        let account_status = app_core.account_status().unwrap_or_else(|error| {
            log::error!("failed to load account status: {error}");
            AccountStatus::default()
        });
        browser_actions::apply_browsing_appearance(browsing_preferences, cx);
        let active_library = active_library_id.as_deref().and_then(|id| resolve_library(id));
        let search_input = cx.new(|cx| InputState::new(window, cx).placeholder("Search books and authors"));
        let author_filter_input = cx.new(|cx| InputState::new(window, cx).placeholder("Filter authors"));
        let new_folder_input = cx.new(|cx| InputState::new(window, cx).placeholder("Folder name"));
        let account_username_input = cx.new(|cx| InputState::new(window, cx).placeholder("Username"));
        let account_password_input = cx.new(|cx| InputState::new(window, cx).placeholder("Password").masked(true));
        let subscriptions = vec![
            cx.subscribe_in(&search_input, window, |this, input, event: &InputEvent, _window, cx| {
                if matches!(event, InputEvent::Change) {
                    let query = input.read(cx).value().to_string();
                    this.search(query, cx);
                }
            }),
            cx.subscribe_in(&author_filter_input, window, |_, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
            cx.observe_window_bounds(window, {
                let save_state = save_state.clone();
                move |this, window, cx| {
                    let bounds = window.bounds();
                    save_state(this.session(), Some(BrowserWindowGeometry { x: f32::from(bounds.origin.x), y: f32::from(bounds.origin.y), width: f32::from(bounds.size.width), height: f32::from(bounds.size.height) }), cx);
                }
            }),
        ];

        let focus = cx.focus_handle();
        focus.focus(window, cx);

        let mut view = Self {
            libraries,
            resolve_library,
            open_reader,
            save_state,
            blocking_worker,
            app_core,
            browsing_preferences,
            account_status,
            sync_statuses: Arc::new(Vec::new()),
            transfer_groups: Arc::new(Vec::new()),
            account_username_input,
            account_password_input,
            cloud_busy: false,
            cloud_message: None,
            cloud_error: None,
            cloud_task: None,
            active_library_id,
            active_library,
            section,
            settings_open: false,
            folder_path: session.folder_path,
            folder_directories: Arc::new(Vec::new()),
            books: Arc::new(Vec::new()),
            home_sections: Arc::new(Vec::new()),
            authors: Arc::new(Vec::new()),
            author_groups: Arc::new(Vec::new()),
            selected_author: None,
            search_input,
            author_filter_input,
            new_folder_input,
            search_active: false,
            focus,
            cover_cache: HashMap::new(),
            cover_order: VecDeque::new(),
            cover_costs: HashMap::new(),
            cover_cache_bytes: 0,
            pending_cover_ids: HashSet::new(),
            loading_cover_ids: HashSet::new(),
            missing_cover_ids: HashSet::new(),
            cover_load_running: false,
            cover_generation: 0,
            data_generation: 0,
            data_task: None,
            pending_thumbnail_ids: HashSet::new(),
            thumbnail_reload_scheduled: false,
            refresh_scheduled: false,
            _subscriptions: subscriptions,
        };
        view.refresh_active_section(cx);
        if view.section == BrowserSection::Sync {
            view.refresh_cloud(cx);
        }
        view
    }

    pub fn session(&self) -> BrowserSession {
        BrowserSession { active_library_id: self.active_library_id.clone(), section: self.section, folder_path: self.folder_path.clone() }
    }

    pub fn handle_library_event(&mut self, library_id: &str, event: &LibraryEvent, cx: &mut Context<Self>) {
        log::debug!("GPUI browser received library event for {library_id}: {event:?}");
        if self.active_library_id.as_deref() != Some(library_id) {
            return;
        }

        match event {
            LibraryEvent::DownloadStatusChanged { .. } => {}
            LibraryEvent::ThumbnailGenerated { book_id } => {
                self.queue_visible_cover_reload(*book_id, cx);
            }
            LibraryEvent::BookOpened { .. } if self.section != BrowserSection::Home => {}
            _ if matches!(self.section, BrowserSection::Search | BrowserSection::Sync | BrowserSection::Settings) => {}
            _ => self.schedule_refresh(cx),
        }
    }
}

pub fn bind_keys(cx: &mut App) {
    view::bind_keys(cx);
}
