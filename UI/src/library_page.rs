use crate::components::create_sidebar_nav_button;
use crate::library_tabs::authors_tab::authors_tab_view;
use crate::library_tabs::folders_tab::folders_tab_view;
use crate::library_tabs::home_tab::home_tab_view;
use crate::library_tabs::search_tab::search_tab_view;
use crate::page_navigation::Signals;
use floem::event::EventPropagation;
use floem::keyboard::{Key, Modifiers, NamedKey};
use floem::prelude::{create_rw_signal, h_stack, stack, v_stack, Decorators, RwSignal, SignalGet, SignalUpdate};
use floem::style::{AlignItems, Style};
use floem::View;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub enum LibraryTab {
    Home,
    Authors,
    Search,
    Folders,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct LibraryStateData {
    pub library_id: String,
    pub library_name: String,
    pub active_tab: LibraryTab,
    pub folder_stack: Vec<(String, String)>,
}

#[derive(Clone)]
pub struct LibraryState {
    pub library_id: String,
    pub library_name: String,
    pub active_tab: RwSignal<LibraryTab>,
    pub folder_stack: RwSignal<Vec<(String, String)>>,
}

impl LibraryState {
    pub fn new(library_id: String, library_name: String) -> Self {
        Self {
            library_id,
            library_name,
            active_tab: create_rw_signal(LibraryTab::Folders),
            folder_stack: create_rw_signal(vec![]),
        }
    }

    pub fn from_data(data: LibraryStateData) -> Self {
        Self {
            library_id: data.library_id,
            library_name: data.library_name,
            active_tab: create_rw_signal(data.active_tab),
            folder_stack: create_rw_signal(data.folder_stack),
        }
    }

    pub fn snapshot(&self) -> LibraryStateData {
        LibraryStateData {
            library_id: self.library_id.clone(),
            library_name: self.library_name.clone(),
            active_tab: self.active_tab.get_untracked(),
            folder_stack: self.folder_stack.get_untracked(),
        }
    }

    pub fn library_view(&self, signals: Signals) -> impl View {
        let active_tab = self.active_tab;
        let folder_stack = self.folder_stack;
        let library_id = self.library_id.clone();
        let library_name = self.library_name.clone();

        let sidebar = create_library_sidebar(active_tab);

        let library_id_for_home = library_id.clone();
        let library_id_for_authors = library_id.clone();
        let library_id_for_folders = library_id;
        let home_tab = home_tab_view(library_id_for_home, signals.clone()).style(move |s| tab_visibility(s, active_tab, LibraryTab::Home));
        let authors_tab = authors_tab_view(library_id_for_authors, signals.clone()).style(move |s| tab_visibility(s, active_tab, LibraryTab::Authors));
        let search_tab = search_tab_view().style(move |s| tab_visibility(s, active_tab, LibraryTab::Search));
        let folders_tab = folders_tab_view(library_id_for_folders, library_name, folder_stack, signals.clone()).style(move |s| tab_visibility(s, active_tab, LibraryTab::Folders));
        let content = stack((home_tab, authors_tab, search_tab, folders_tab)).style(|s| s.width_full().height_full().flex_grow(1.0));

        let library_root = h_stack((sidebar, content))
            .on_key_down(
                Key::Named(NamedKey::Backspace),
                |_mods: Modifiers| true,
                move |_| {
                    if active_tab.get() != LibraryTab::Folders {
                        return;
                    }
                    folder_stack.update(|stack| {
                        if !stack.is_empty() {
                            stack.pop();
                        }
                    });
                },
            )
            .on_key_down(Key::Character("1".into()), |mods: Modifiers| mods.control(), move |_| { active_tab.set(LibraryTab::Home); })
            .on_key_down(Key::Character("2".into()), |mods: Modifiers| mods.control(), move |_| { active_tab.set(LibraryTab::Authors); })
            .on_key_down(Key::Character("3".into()), |mods: Modifiers| mods.control(), move |_| { active_tab.set(LibraryTab::Search); })
            .on_key_down(Key::Character("4".into()), |mods: Modifiers| mods.control(), move |_| { active_tab.set(LibraryTab::Folders); })
            .style(|s| s.width_full().height_full().flex_grow(1.0));

        let library_root_id = library_root.id();
        let library_root = library_root.on_click_stop(move |_| {
            library_root_id.request_focus();
        });

        library_root
    }
}

fn tab_visibility(s: Style, active_tab: RwSignal<LibraryTab>, tab: LibraryTab) -> Style {
    if active_tab.get() == tab {
        s.size_full()
    } else {
        s.hide()
    }
}

fn create_library_sidebar(active_tab: RwSignal<LibraryTab>) -> impl View {
    v_stack((
        create_sidebar_nav_button("Home", HOME_ICON, move || active_tab.get() == LibraryTab::Home).on_click(move |_| {
            active_tab.set(LibraryTab::Home);
            EventPropagation::Continue
        }),
        create_sidebar_nav_button("Authors", AUTHORS_ICON, move || active_tab.get() == LibraryTab::Authors).on_click(move |_| {
            active_tab.set(LibraryTab::Authors);
            EventPropagation::Continue
        }),
        create_sidebar_nav_button("Search", SEARCH_ICON, move || active_tab.get() == LibraryTab::Search).on_click(move |_| {
            active_tab.set(LibraryTab::Search);
            EventPropagation::Continue
        }),
        create_sidebar_nav_button("Folders", FOLDERS_ICON, move || active_tab.get() == LibraryTab::Folders).on_click(move |_| {
            active_tab.set(LibraryTab::Folders);
            EventPropagation::Continue
        }),
    ))
    .style(|s| s.width(92).height_full().padding(0).gap(0).align_items(AlignItems::Start))
}

const HOME_ICON: &str = r##"<svg viewBox="0 0 24 24" fill="none" stroke="#374151" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 10.5 12 3l9 7.5"/><path d="M5 9.5V21h14V9.5"/></svg>"##;
const AUTHORS_ICON: &str = r##"<svg viewBox="0 0 24 24" fill="none" stroke="#374151" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="9" cy="8" r="3"/><path d="M3.5 20a6 6 0 0 1 11 0"/><circle cx="17" cy="9" r="2.5"/><path d="M14.5 20a4.8 4.8 0 0 1 6 0"/></svg>"##;
const SEARCH_ICON: &str = r##"<svg viewBox="0 0 24 24" fill="none" stroke="#374151" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/></svg>"##;
const FOLDERS_ICON: &str = r##"<svg viewBox="0 0 24 24" fill="none" stroke="#374151" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 6h6l2 2h10v10a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/></svg>"##;
