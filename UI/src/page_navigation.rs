use crate::components::{navigation_layer, navigation_stack, reader_navigation_surface};
use crate::epub_reader::create_epub_reader;
use crate::home_page::home_view;
use crate::library_page::{LibraryMeta, LibraryState, LibraryStateData};
use crate::session::{load_session, save_session, HomeSession, PersistedPage, ReaderUiSession, UiSession, WindowSession};
use crate::style::{ThemeContext, TypographyContext};
use epub_svelte_lib::app_core::AppCore;
use epub_svelte_lib::preferences::{ApplicationTheme, BrowsingPreferences, ReaderPreferencePatch, ReaderPreferences, UiFontFamily};
use epub_svelte_lib::BookId;
use floem::action::exec_after;
use floem::event::{Event, EventListener};
use floem::kurbo::Rect;
use floem::prelude::{create_rw_signal, dyn_view, label, Decorators, RwSignal, SignalGet, SignalUpdate, SignalWith};
use floem::reactive::{create_effect, provide_context};
use floem::View;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone, Eq, Hash, PartialEq)]
pub struct ReaderState {
    pub book_id: BookId,
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
    pub app_core: AppCore,
    pub nav_stack: RwSignal<NavStack>,
    pub nav_action: RwSignal<Option<NavAction>>,
    pub libraries: RwSignal<Vec<LibraryState>>,
    pub library_refresh_tick: RwSignal<u64>,
    pub library_event_counter: Arc<AtomicU64>,
    pub active_library_id: RwSignal<Option<String>>,
    pub viewport_height: RwSignal<Rect>,
    pub session: RwSignal<Option<epub_svelte_lib::auth::Session>>,
    pub application_theme: RwSignal<ApplicationTheme>,
    pub interface_font: RwSignal<UiFontFamily>,
    pub heading_font: RwSignal<UiFontFamily>,
    pub reader_preferences: RwSignal<ReaderPreferences>,
    pub reader_ui: RwSignal<ReaderUiSession>,
    pub window: RwSignal<Option<WindowSession>>,
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
        id.as_deref().and_then(|active_id| self.libraries.with_untracked(|v| v.iter().find(|s| s.meta.id == active_id).map(|s| s.meta.name.clone()))).unwrap_or_default()
    }
}

pub fn page_navigation_view(app_core: AppCore) -> impl View {
    let restored_session = load_session(&app_core);
    let restored_reader_ui = restored_session.legacy_reader_settings.map(|legacy| ReaderUiSession { left_panel_width: legacy.left_panel_width, toc_open: legacy.toc_open }).unwrap_or(restored_session.reader_ui).sanitized();
    let preferences_file_exists = app_core.has_persisted_reader_preferences();
    let mut restored_preferences = app_core.reader_preferences().unwrap_or_else(|error| {
        eprintln!("failed to load reader preferences: {error}");
        ReaderPreferences::default()
    });
    if !preferences_file_exists {
        if let Some(legacy) = restored_session.legacy_reader_settings {
            for patch in [ReaderPreferencePatch::FontSizePx(legacy.font_size), ReaderPreferencePatch::MaxLineWidthPx(legacy.column_width), ReaderPreferencePatch::Zoom(legacy.scale), ReaderPreferencePatch::LineNumbers(legacy.line_numbers)] {
                match app_core.update_reader_preference(patch) {
                    Ok(updated) => restored_preferences = updated,
                    Err(error) => eprintln!("failed to migrate reader preference: {error}"),
                }
            }
        }
    }
    let restored_browsing_preferences = app_core.browsing_preferences().unwrap_or_else(|error| {
        eprintln!("failed to load browsing preferences: {error}");
        BrowsingPreferences::default()
    });
    let restored_theme = restored_browsing_preferences.theme();
    let restored_libraries = restore_libraries(&app_core, &restored_session);
    let restored_active_library_id = restore_active_library_id(restored_session.home.active_library_id.clone(), &restored_libraries);
    let library_event_counter = Arc::new(AtomicU64::new(0));
    let library_events = app_core.take_events().expect("desktop application event receiver is unique");
    let event_counter = library_event_counter.clone();
    std::thread::spawn(move || {
        while library_events.recv().is_ok() {
            event_counter.fetch_add(1, Ordering::Relaxed);
        }
    });

    let signals = Signals {
        app_core,
        nav_stack: create_rw_signal(NavStack { pages: vec![Page::Home], index: 0 }),
        nav_action: create_rw_signal(None),
        libraries: create_rw_signal(Vec::new()),
        library_refresh_tick: create_rw_signal(0),
        library_event_counter,
        active_library_id: create_rw_signal(restored_active_library_id),
        viewport_height: create_rw_signal(Rect::new(0.0, 0.0, 1000.0, 1000.0)),
        session: create_rw_signal(epub_svelte_lib::auth::Session::load().unwrap_or_else(|error| {
            eprintln!("failed to load session: {error}");
            None
        })),
        application_theme: create_rw_signal(restored_theme),
        interface_font: create_rw_signal(restored_browsing_preferences.interface_font()),
        heading_font: create_rw_signal(restored_browsing_preferences.heading_font()),
        reader_preferences: create_rw_signal(restored_preferences),
        reader_ui: create_rw_signal(restored_reader_ui),
        window: create_rw_signal(restored_session.window.and_then(|window| window.sanitized())),
    };
    provide_context(ThemeContext { theme: signals.application_theme });
    provide_context(TypographyContext { interface_font: signals.interface_font, heading_font: signals.heading_font });

    // Eagerly populate the library cache in the root scope so signals on
    // each `LibraryState` outlive any `dyn_view` rebuild. Backend handles
    // are opened lazily on first access.
    {
        signals.libraries.set(restored_libraries);
    }

    if let Some(page) = restore_page(&restored_session.current_page, &signals) {
        signals.nav_stack.set(NavStack { pages: vec![Page::Home, page], index: 1 });
    }

    create_effect(move |_| {
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

    create_effect({
        let signals = signals.clone();
        move |_| {
            let libraries = signals.libraries.get();
            let snapshots = libraries.iter().map(|library| library.snapshot()).collect();
            let session = UiSession {
                home: HomeSession { active_library_id: signals.active_library_id.get(), libraries: snapshots },
                current_page: match signals.nav_stack.get().current() {
                    Page::Home => PersistedPage::Home,
                    Page::Reader(reader) => PersistedPage::from(reader),
                },
                reader_ui: signals.reader_ui.get().sanitized(),
                legacy_reader_settings: None,
                window: signals.window.get(),
            };
            if let Err(err) = save_session(&signals.app_core, &session) {
                eprintln!("failed to save UI session: {err}");
            }
        }
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
    let home_view = navigation_layer(home_content, move || matches!(nav_stack.get().current(), Page::Home));

    // Overlay view handles Reader; hidden on Home so home_view shows through.
    let overlay_view = dyn_view({
        let signals = signals.clone();
        move || match signals.nav_stack.get().current().clone() {
            Page::Home => Box::new(label(|| "")) as Box<dyn View>,
            Page::Reader(reader_state) => match signals.library_by_id(&reader_state.library_id) {
                Some(_) => Box::new(reader_navigation_surface(create_epub_reader(&reader_state, signals.clone()))) as Box<dyn View>,
                None => Box::new(label(|| "Library not found".to_string())) as Box<dyn View>,
            },
        }
    });
    let nav_stack = signals.nav_stack;
    let overlay_view = navigation_layer(overlay_view, move || !matches!(nav_stack.get().current(), Page::Home));

    navigation_stack((home_view, overlay_view))
        .on_resize(move |rect| {
            signals.viewport_height.set(rect);
        })
        .on_event_cont(EventListener::WindowResized, {
            let signals = signals.clone();
            move |event| {
                if let Event::WindowResized(size) = event {
                    let current = signals.window.get_untracked().unwrap_or(WindowSession { width: size.width, height: size.height, x: 0.0, y: 0.0 });
                    signals.window.set(Some(current.with_size(*size)));
                }
            }
        })
        .on_event_cont(EventListener::WindowMoved, move |event| {
            if let Event::WindowMoved(position) = event {
                let current = signals.window.get_untracked().unwrap_or(WindowSession { width: 1000.0, height: 1000.0, x: position.x, y: position.y });
                signals.window.set(Some(current.with_position(*position)));
            }
        })
}

fn restore_libraries(core: &AppCore, session: &UiSession) -> Vec<LibraryState> {
    let saved_by_id: HashMap<String, LibraryStateData> = session.home.libraries.iter().cloned().map(|state| (state.meta.id.clone(), state)).collect();
    core.libraries()
        .unwrap_or_else(|error| {
            eprintln!("failed to load library registry: {error}");
            Vec::new()
        })
        .into_iter()
        .map(|entry| {
            let library_id = entry.library_id().as_str().to_owned();
            let library_name = entry.library_name().to_owned();
            let library_path = entry.library_path().to_string_lossy().into_owned();
            let meta = LibraryMeta { id: library_id.clone(), name: library_name.clone(), path: library_path.clone() };
            if let Some(saved) = saved_by_id.get(&library_id) {
                let mut data = saved.clone();
                data.meta = meta;
                data.folder_stack = validate_folder_stack(core, &library_id, &data.folder_stack);
                LibraryState::from_data(data)
            } else {
                LibraryState::new(library_id, library_name, library_path)
            }
        })
        .collect()
}

fn restore_active_library_id(saved_id: Option<String>, libraries: &[LibraryState]) -> Option<String> {
    saved_id.filter(|id| libraries.iter().any(|library| library.meta.id == *id)).or_else(|| libraries.first().map(|library| library.meta.id.clone()))
}

fn validate_folder_stack(core: &AppCore, library_id: &str, saved_stack: &[(String, String)]) -> Vec<(String, String)> {
    if saved_stack.is_empty() {
        return vec![];
    }
    let mut parent_id = "__root__".to_string();
    let mut valid = Vec::new();
    for (folder_id, folder_name) in saved_stack {
        let Ok(contents) = core.directory_in(library_id, &parent_id) else {
            break;
        };
        if contents.dirs.iter().any(|dir| dir.id().as_str() == folder_id) {
            valid.push((folder_id.clone(), folder_name.clone()));
            parent_id = folder_id.clone();
        } else {
            break;
        }
    }
    valid
}

fn restore_page(saved_page: &PersistedPage, signals: &Signals) -> Option<Page> {
    let PersistedPage::Reader { library_id, book_id, book_title } = saved_page else {
        return None;
    };
    signals.library_by_id(library_id)?;
    if signals.app_core.validate_book_in(library_id, *book_id).is_err() {
        return None;
    }
    Some(Page::Reader(ReaderState { book_id: *book_id, library_id: library_id.clone(), book_title: book_title.clone() }))
}
