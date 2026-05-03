use crate::epub_reader::create_epub_reader;
use crate::home_page::HomePage;
use crate::library_page::LibraryState;
use epub_svelte_lib::home::get_library_entries;
use epub_svelte_lib::library::Library;
use epub_svelte_lib::BookID;
use floem::action::exec_after;
use floem::prelude::{container, create_rw_signal, dyn_view, label, Decorators, RwSignal, SignalGet, SignalUpdate, SignalWith};
use floem::reactive::create_effect;
use floem::views::stack;
use floem::View;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone, Eq, Hash, PartialEq)]
pub struct ReaderState {
    pub book_id: BookID,
    pub library_id: String,
    pub book_title: String,
}

#[derive(Clone, Eq, Hash, PartialEq)]
pub enum Page {
    Home,
    Reader(ReaderState),
}

#[derive(Clone)]
pub struct NavStack {
    pages: Vec<Page>,
    index: usize,
}

impl NavStack {
    fn current(&self) -> &Page {
        &self.pages[self.index]
    }
}

#[derive(Clone)]
pub enum NavAction {
    Back,
    Navigate(Page),
}

pub type LibraryID = String;

#[derive(Clone)]
pub struct Signals {
    pub nav_stack: RwSignal<NavStack>,
    pub nav_action: RwSignal<Option<NavAction>>,
    pub library_handles: RwSignal<HashMap<LibraryID, Rc<Library>>>,
    pub library_states: RwSignal<HashMap<LibraryID, LibraryState>>,
    pub library_refresh_tick: RwSignal<u64>,
    pub library_event_counter: Arc<AtomicU64>,
    pub active_library_id: RwSignal<Option<String>>,
    pub active_library_name: RwSignal<String>,
}

impl Signals {
    pub fn go_back(&self) {
        self.nav_action.set(Some(NavAction::Back));
    }
    pub fn navigate(&self, page: Page) {
        self.nav_action.set(Some(NavAction::Navigate(page)));
    }

    /// Returns the cached `LibraryState` for the given id, if present.
    /// States are eagerly populated in the root scope by `page_navigation_view`
    /// (and re-populated when libraries are added/removed) so signals on them
    /// outlive `dyn_view` rebuilds.
    pub fn library_state(&self, library_id: &str) -> Option<LibraryState> {
        self.library_states.with_untracked(|m| m.get(library_id).cloned())
    }
}

pub fn page_navigation_view() -> impl View {
    let signals = Signals {
        nav_stack: create_rw_signal(NavStack { pages: vec![Page::Home], index: 0 }),
        nav_action: create_rw_signal(None),
        library_handles: create_rw_signal(HashMap::new()),
        library_states: create_rw_signal(HashMap::new()),
        library_refresh_tick: create_rw_signal(0),
        library_event_counter: Arc::new(AtomicU64::new(0)),
        active_library_id: create_rw_signal(None),
        active_library_name: create_rw_signal(String::new()),
    };

    // Eagerly populate the per-library state cache in the root scope so the
    // contained signals outlive any `dyn_view` rebuild.
    {
        let mut states = HashMap::new();
        for entry in get_library_entries() {
            states.insert(entry.library_id.clone(), LibraryState::new(entry.library_id, entry.library_name));
        }
        signals.library_states.set(states);
    }

    create_effect(move |h| {
        let Some(action) = signals.nav_action.get() else { return };
        signals.nav_stack.update(|nav| match action {
            NavAction::Back => {
                if nav.index > 0 {
                    nav.index -= 1
                }
            }
            NavAction::Navigate(page) => {
                nav.pages.truncate(nav.index + 1);
                nav.pages.push(page);
                nav.index = nav.pages.len() - 1;
            }
        });
        signals.nav_action.set(None)
    });

    // Bridge backend library events into a reactive UI tick without rebuilding views.
    {
        let signals_for_bridge = signals.clone();
        let last_seen_counter = create_rw_signal(0_u64);
        fn schedule_library_event_bridge(signals: Signals, last_seen_counter: RwSignal<u64>) {
            exec_after(Duration::from_millis(200), move |_| {
                let current = signals.library_event_counter.load(Ordering::Relaxed);
                if current != last_seen_counter.get_untracked() {
                    last_seen_counter.set(current);
                    signals.library_refresh_tick.set(current);
                }
                schedule_library_event_bridge(signals.clone(), last_seen_counter);
            });
        }
        schedule_library_event_bridge(signals_for_bridge, last_seen_counter);
    }

    // Home view is created once and shown/hidden — never reconstructed on navigation.
    let home_content = HomePage {}.home_view(signals.clone());
    let nav_stack = signals.nav_stack;
    let home_view = container(home_content).style(move |s| {
        if matches!(nav_stack.get().current(), Page::Home) {
            s.width_full().height_full().flex_grow(1.0).min_width(0).min_height(0)
        } else {
            s.hide()
        }
    });

    // Overlay view handles Reader; hidden on Home so home_view shows through.
    let overlay_view = dyn_view({
        let signals = signals.clone();
        move || match signals.nav_stack.get().current().clone() {
            Page::Home => Box::new(label(|| "")) as Box<dyn View>,
            Page::Reader(reader_state) => Box::new(
                container(create_epub_reader(&reader_state, signals.clone())).style(|s| s.width_full().height_full().flex_grow(1.0).min_width(0).min_height(0)),
            ) as Box<dyn View>,
        }
    })
    .style(move |s| {
        if matches!(nav_stack.get().current(), Page::Home) {
            s.hide()
        } else {
            s.width_full().height_full().flex_grow(1.0).min_width(0).min_height(0)
        }
    });

    stack((home_view, overlay_view)).style(|s| s.width_full().height_full().flex_grow(1.0).min_width(0).min_height(0))
}
