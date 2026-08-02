use crate::components::{library_workspace, nav_rail, nav_rail_item, tab_panel};
use crate::library_tabs::authors_tab::authors_tab_view;
use crate::library_tabs::folders_tab::folders_tab_view;
use crate::library_tabs::home_tab::home_tab_view;
use crate::library_tabs::search_tab::search_tab_view;
use crate::library_tabs::store_tab::store_tab_view;
use crate::page_navigation::Signals;
use epub_svelte_lib::frontend_contract::AppFeature;
use floem::event::EventPropagation;
use floem::keyboard::{Key, Modifiers, NamedKey};
use floem::prelude::{create_rw_signal, Decorators, RwSignal, SignalGet, SignalUpdate};
use floem::views::stack;
use floem::View;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum LibraryTab {
    Home,
    Authors,
    Search,
    Folders,
    Store,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LibraryMeta {
    pub id: String,
    pub name: String,
    pub path: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LibraryStateData {
    #[serde(flatten)]
    pub meta: LibraryMeta,
    pub active_tab: LibraryTab,
    pub folder_stack: Vec<(String, String)>,
}

#[derive(Clone)]
pub struct LibraryState {
    pub meta: LibraryMeta,
    pub active_tab: RwSignal<LibraryTab>,
    pub folder_stack: RwSignal<Vec<(String, String)>>,
}

impl LibraryState {
    pub fn new(id: String, name: String, path: String) -> Self {
        Self { meta: LibraryMeta { id, name, path }, active_tab: create_rw_signal(LibraryTab::Folders), folder_stack: create_rw_signal(vec![]) }
    }

    pub fn from_data(data: LibraryStateData) -> Self {
        Self { meta: data.meta, active_tab: create_rw_signal(data.active_tab), folder_stack: create_rw_signal(data.folder_stack) }
    }

    pub fn snapshot(&self) -> LibraryStateData {
        LibraryStateData { meta: self.meta.clone(), active_tab: self.active_tab.get(), folder_stack: self.folder_stack.get() }
    }

    pub fn library_view(&self, signals: Signals) -> impl View {
        let active_tab = self.active_tab;
        let folder_stack = self.folder_stack;
        let library_id = self.meta.id.clone();
        let library_name = self.meta.name.clone();

        let sidebar = library_sidebar(active_tab);

        let library_id_for_home = library_id.clone();
        let library_id_for_authors = library_id.clone();
        let library_id_for_folders = library_id;
        let home_tab = tab_panel(home_tab_view(library_id_for_home, signals.clone()), move || active_tab.get() == LibraryTab::Home);
        let authors_tab = tab_panel(authors_tab_view(library_id_for_authors, signals.clone()), move || active_tab.get() == LibraryTab::Authors);
        let search_tab = tab_panel(search_tab_view(), move || active_tab.get() == LibraryTab::Search);
        let folders_tab = tab_panel(folders_tab_view(library_id_for_folders, library_name, folder_stack, signals.clone()), move || active_tab.get() == LibraryTab::Folders);
        let store_tab = tab_panel(store_tab_view(signals.clone()), move || active_tab.get() == LibraryTab::Store);
        let content = stack((home_tab, authors_tab, search_tab, folders_tab, store_tab));

        let library_root = library_workspace(sidebar, content)
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
            .on_key_down(Key::Character("1".into()), |mods: Modifiers| mods.control(), move |_| active_tab.set(LibraryTab::Home))
            .on_key_down(Key::Character("2".into()), |mods: Modifiers| mods.control(), move |_| active_tab.set(LibraryTab::Authors))
            .on_key_down(Key::Character("3".into()), |mods: Modifiers| mods.control(), move |_| active_tab.set(LibraryTab::Search))
            .on_key_down(Key::Character("4".into()), |mods: Modifiers| mods.control(), move |_| active_tab.set(LibraryTab::Folders))
            .on_key_down(Key::Character("5".into()), |mods: Modifiers| mods.control(), move |_| active_tab.set(LibraryTab::Store));

        let library_root_id = library_root.id();
        library_root.on_click_stop(move |_| {
            library_root_id.request_focus();
        })
    }
}

fn library_sidebar(active_tab: RwSignal<LibraryTab>) -> impl View {
    nav_rail((
        nav_rail_item(AppFeature::Home.label(), HOME_ICON, move || active_tab.get() == LibraryTab::Home).on_click(move |_| {
            active_tab.set(LibraryTab::Home);
            EventPropagation::Continue
        }),
        nav_rail_item(AppFeature::Authors.label(), AUTHORS_ICON, move || active_tab.get() == LibraryTab::Authors).on_click(move |_| {
            active_tab.set(LibraryTab::Authors);
            EventPropagation::Continue
        }),
        nav_rail_item(AppFeature::Search.label(), SEARCH_ICON, move || active_tab.get() == LibraryTab::Search).on_click(move |_| {
            active_tab.set(LibraryTab::Search);
            EventPropagation::Continue
        }),
        nav_rail_item(AppFeature::Library.label(), FOLDERS_ICON, move || active_tab.get() == LibraryTab::Folders).on_click(move |_| {
            active_tab.set(LibraryTab::Folders);
            EventPropagation::Continue
        }),
        nav_rail_item(AppFeature::Store.label(), STORE_ICON, move || active_tab.get() == LibraryTab::Store).on_click(move |_| {
            active_tab.set(LibraryTab::Store);
            EventPropagation::Continue
        }),
    ))
}

const HOME_ICON: &str = r##"<svg viewBox="0 0 24 24" fill="none" stroke="#374151" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 10.5 12 3l9 7.5"/><path d="M5 9.5V21h14V9.5"/></svg>"##;
const AUTHORS_ICON: &str = r##"<svg viewBox="0 0 24 24" fill="none" stroke="#374151" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="9" cy="8" r="3"/><path d="M3.5 20a6 6 0 0 1 11 0"/><circle cx="17" cy="9" r="2.5"/><path d="M14.5 20a4.8 4.8 0 0 1 6 0"/></svg>"##;
const SEARCH_ICON: &str = r##"<svg viewBox="0 0 24 24" fill="none" stroke="#374151" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/></svg>"##;
const FOLDERS_ICON: &str = r##"<svg viewBox="0 0 24 24" fill="none" stroke="#374151" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 6h6l2 2h10v10a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/></svg>"##;
const STORE_ICON: &str =
    r##"<svg viewBox="0 0 24 24" fill="none" stroke="#374151" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 9h16l-1 12H5z"/><path d="m3 9 2-5h14l2 5"/><path d="M9 9V6a3 3 0 0 1 6 0v3"/></svg>"##;
