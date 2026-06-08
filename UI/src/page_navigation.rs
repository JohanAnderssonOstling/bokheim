use crate::epub_reader::create_epub_reader;
use crate::home_page::home_view;
use crate::library_page::LibraryState;
use crate::style::{ThemeContext, ThemeMode, DEFAULT_THEME_MODE};
use epub_svelte_lib::home::get_library_entries;
use epub_svelte_lib::BookID;
use floem::action::exec_after;
use floem::prelude::{container, create_rw_signal, dyn_view, label, Decorators, RwSignal, SignalGet, SignalUpdate, SignalWith};
use floem::reactive::{create_effect, provide_context};
use floem::views::stack;
use floem::View;
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

#[derive(Clone)]
pub struct Signals {
    pub nav_stack: RwSignal<NavStack>,
    pub nav_action: RwSignal<Option<NavAction>>,
    pub libraries: RwSignal<Vec<LibraryState>>,
    pub library_refresh_tick: RwSignal<u64>,
    pub library_event_counter: Arc<AtomicU64>,
    pub active_library_id: RwSignal<Option<String>>,
    pub viewport_height: RwSignal<f64>,
    pub session: RwSignal<Option<epub_svelte_lib::auth::Session>>,
    pub theme_mode: RwSignal<ThemeMode>,
}

impl Signals {
    pub fn go_back(&self) {
        self.nav_action.set(Some(NavAction::Back));
    }
    pub fn navigate(&self, page: Page) {
        self.nav_action.set(Some(NavAction::Navigate(page)));
    }

    pub fn library_by_id(&self, library_id: &str) -> Option<LibraryState> {
        self.libraries.with_untracked(|v| v.iter().find(|s| s.meta.id == library_id).cloned())
    }

    pub fn active_library_name(&self) -> String {
        let id = self.active_library_id.get();
        id.as_deref()
            .and_then(|active_id| self.libraries.with_untracked(|v| v.iter().find(|s| s.meta.id == active_id).map(|s| s.meta.name.clone())))
            .unwrap_or_default()
    }
}

pub fn page_navigation_view() -> impl View {
    let signals = Signals {
        nav_stack: create_rw_signal(NavStack { pages: vec![Page::Home], index: 0 }),
        nav_action: create_rw_signal(None),
        libraries: create_rw_signal(Vec::new()),
        library_refresh_tick: create_rw_signal(0),
        library_event_counter: Arc::new(AtomicU64::new(0)),
        active_library_id: create_rw_signal(None),
        viewport_height: create_rw_signal(0.0),
        session: create_rw_signal(epub_svelte_lib::auth::Session::load()),
        theme_mode: create_rw_signal(DEFAULT_THEME_MODE),
    };
    provide_context(ThemeContext { mode: signals.theme_mode });

    // Eagerly populate the library cache in the root scope so signals on
    // each `LibraryState` outlive any `dyn_view` rebuild. Backend handles
    // are opened lazily on first access.
    {
        let initial: Vec<LibraryState> = get_library_entries()
            .into_iter()
            .map(|e| LibraryState::new(e.library_id, e.library_name, e.library_path))
            .collect();
        signals.libraries.set(initial);
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

    fn poll_library_events(signals: Signals) {
        exec_after(Duration::from_millis(200), move |_| {
            let current = signals.library_event_counter.load(Ordering::Relaxed);
            if current != signals.library_refresh_tick.get_untracked() {
                signals.library_refresh_tick.set(current);
            }
            poll_library_events(signals.clone());
        });
    }
    poll_library_events(signals.clone());

    // Home view is created once and shown/hidden — never reconstructed on navigation.
    let home_content = home_view(signals.clone());
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
            Page::Reader(reader_state) => match signals.library_by_id(&reader_state.library_id) {
                Some(state) => {
                    let library = state.handle(signals.library_event_counter.clone());
                    Box::new(
                        container(create_epub_reader(&reader_state, library, signals.clone())).style(|s| s.width_full().height_full().flex_grow(1.0).min_width(0).min_height(0)),
                    ) as Box<dyn View>
                }
                None => Box::new(label(|| "Library not found".to_string())) as Box<dyn View>,
            },
        }
    })
    .style(move |s| {
        if matches!(nav_stack.get().current(), Page::Home) {
            s.hide()
        } else {
            s.width_full().height_full().flex_grow(1.0).min_width(0).min_height(0)
        }
    });

    stack((home_view, overlay_view))
        .on_resize(move |rect| {
            let next = rect.height();
            if (next - signals.viewport_height.get_untracked()).abs() > 0.5 {
                signals.viewport_height.set(next);
            }
        })
        .style(|s| s.width_full().height_full().flex_grow(1.0).min_width(0).min_height(0))
}
