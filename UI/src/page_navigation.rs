use crate::epub_reader::create_epub_reader;
use crate::home_page::{ HomePage};
use epub_svelte_lib::home::LibraryEntry;
use epub_svelte_lib::library::Library;
use epub_svelte_lib::scanner::{DownloadState, LibraryEvent};
use floem::prelude::{container, create_rw_signal, dyn_view, Decorators, RwSignal, SignalGet, SignalUpdate};
use floem::reactive::{create_effect, ReadSignal};
use floem::View;
use std::collections::HashMap;
use std::rc::Rc;
use epub_svelte_lib::BookID;
use crate::library_page::LibraryState;

#[derive(Clone, Eq, Hash, PartialEq)]

pub struct ReaderState {
    pub book_id: BookID,
    pub library_id: String,
    pub book_title: String,
}


impl LibraryState {
    pub fn navigate_to_folder(&self, dir_id: &str) -> LibraryState {
        Self {
            library_id: self.library_id.clone(),
            folder_id: dir_id.to_string(),
        }
    }
}

#[derive(Clone, Eq, Hash, PartialEq)]
pub enum Page {
    Home,
    Library (LibraryState),
    Reader (ReaderState),
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

    fn prev(&self) -> Option<&Page> {
        if self.index > 0 {
            Some(&self.pages[self.index - 1])
        } else {
            None
        }
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
    //pub active_page: RwSignal<Page>,
    //pub prev_page: RwSignal<Page>,

    pub library_handles: RwSignal<HashMap<LibraryID, Rc<Library>>>,
    pub active_library_id: RwSignal<Option<String>>,
    
}

impl Signals {
    pub fn go_back(&self) {
        self.nav_action.set(Some(NavAction::Back));
    }
    pub fn navigate(&self, page: Page) {
        self.nav_action.set(Some(NavAction::Navigate(page)));
    }

}

pub fn page_navigation_view() -> impl View {

    let signals = Signals {
        nav_stack: create_rw_signal(NavStack {
            pages: vec![Page::Home],
            index: 0,
        }),
        nav_action: create_rw_signal(None),
        library_handles: create_rw_signal(HashMap::new()),
        active_library_id: create_rw_signal(None),
    };

    create_effect(move |h| {
        let Some(action) = signals.nav_action.get() else {return};
        signals.nav_stack.update(|nav| match action {
            NavAction::Back =>  if nav.index > 0 {nav.index -= 1}
            NavAction::Navigate(page) => {
                nav.pages.truncate(nav.index + 1);
                nav.pages.push(page);
                nav.index = nav.pages.len() - 1;
            }

        });
        signals.nav_action.set(None)

    });

    let mut page = HomePage {
        active_library: None,
        library_states: HashMap::new(),
    };

    let content = dyn_view(move || match signals.nav_stack.get().current() {
        Page::Home => container(page.clone().home_view(signals.clone()))
            .style(|s| s.width_full().height_full().flex_grow(1.0)),
        Page::Library(library_state) => container(page.clone().home_view(signals.clone()))
            .style(|s| s.width_full().height_full().flex_grow(1.0)),
        Page::Reader ( reader_state) => container(create_epub_reader(
            reader_state,
            signals.clone(),
        ))
        .style(|s| s.width_full().height_full().flex_grow(1.0)),
    })
    .style(|s| s.width_full().height_full().flex_grow(1.0));

    content
}
