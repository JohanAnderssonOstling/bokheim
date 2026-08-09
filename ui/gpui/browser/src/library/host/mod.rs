//! Controller and lifetime boundary for the selected library.
//!
//! This is the sole writer of [`LibraryState`]. Its empty/active state ties the
//! selected library's root and subscription to the same lifetime.

mod state;

use epub_svelte_lib::app::LibraryEntry;
use epub_svelte_lib::library::LibraryBackend;
use epub_svelte_lib::platform::scanner::LibraryEvent;
use gpui::{App, AppContext, Context, Entity, EventEmitter, IntoElement, Render, Subscription, Window};

use super::services::LibraryContext;
use super::{LibraryRoot, LibraryRootEvent};
use crate::model::{BrowserSession, FolderCrumb};
use crate::route::LibraryRoute;
use crate::services::AppServices;

pub(crate) use state::{LibraryState, LibraryStateChanged};

#[derive(Clone)]
pub(crate) enum LibraryHostEvent {
    SessionChanged,
}

pub(crate) struct LibraryHost {
    services: AppServices,
    state: Entity<LibraryState>,
    hosted: HostedLibrary,
}

enum HostedLibrary {
    Empty,
    Active { root: Entity<LibraryRoot>, _subscription: Subscription },
}

impl EventEmitter<LibraryHostEvent> for LibraryHost {}

impl LibraryHost {
    pub(crate) fn new(libraries: Vec<LibraryEntry>, session: &BrowserSession, services: AppServices, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let requested_id = session.active_library_id.clone().filter(|id| libraries.iter().any(|entry| entry.library_id().as_str() == id)).or_else(|| libraries.first().map(|entry| entry.library_id().as_str().to_owned()));
        let selected = requested_id.and_then(|library_id| match resolve_library(&services, &library_id) {
            Ok(library) => Some((library_id, library)),
            Err(error) => {
                log::error!("failed to restore library selection: {error}");
                None
            }
        });
        let (library_id, library) = selected.map(|(id, library)| (Some(id), Some(library))).unwrap_or((None, None));
        let state = cx.new(|_| LibraryState::new(libraries, library_id));
        let mut host = Self { services, state, hosted: HostedLibrary::Empty };
        if let Some(library) = library {
            host.install_library_root(library, session.library_route, session.folder_path.clone(), window, cx);
        }
        host
    }

    pub(crate) fn state(&self) -> Entity<LibraryState> {
        self.state.clone()
    }

    pub(crate) fn library(&self, cx: &App) -> Option<LibraryBackend> {
        match &self.hosted {
            HostedLibrary::Empty => None,
            HostedLibrary::Active { root, .. } => Some(root.read(cx).backend()),
        }
    }

    pub(crate) fn route(&self, cx: &App) -> LibraryRoute {
        match &self.hosted {
            HostedLibrary::Empty => LibraryRoute::Home,
            HostedLibrary::Active { root, .. } => root.read(cx).route(),
        }
    }

    pub(crate) fn is_configured(&self, cx: &App) -> bool {
        self.state.read(cx).is_configured()
    }

    pub(crate) fn session(&self, cx: &App) -> BrowserSession {
        let (library_route, folder_path) = match &self.hosted {
            HostedLibrary::Empty => (LibraryRoute::Home, Vec::new()),
            HostedLibrary::Active { root, .. } => {
                let root = root.read(cx);
                (root.route(), root.folder_path())
            }
        };
        BrowserSession { active_library_id: self.state.read(cx).library_id().map(str::to_owned), library_route, folder_path }
    }

    pub(crate) fn set_route(&mut self, route: LibraryRoute, window: &mut Window, cx: &mut Context<Self>) {
        if let HostedLibrary::Active { root, .. } = &self.hosted {
            if root.read(cx).route() == route {
                return;
            }
            root.clone().update(cx, |root, cx| root.set_route(route, window, cx));
            cx.notify();
        }
    }

    /// Selects the backend library, publishes the passive state and replaces
    /// the library-scoped UI as one controller operation.
    pub(crate) fn activate_library(&mut self, library_id: String, window: &mut Window, cx: &mut Context<Self>) -> Result<bool, String> {
        if self.state.read(cx).library_id() == Some(library_id.as_str()) {
            if self.route(cx) == LibraryRoute::Home {
                return Ok(false);
            }
            self.set_route(LibraryRoute::Home, window, cx);
            return Ok(true);
        }
        let library = match resolve_library(&self.services, &library_id) {
            Ok(library) => library,
            Err(error) => {
                let entries = self.state.read(cx).entries().to_vec();
                self.state.update(cx, |state, cx| state.replace(entries, None, cx));
                self.drop_library_root(cx);
                return Err(error);
            }
        };
        let entries = self.state.read(cx).entries().to_vec();
        self.state.update(cx, |state, cx| state.replace(entries, Some(library_id), cx));
        self.install_library_root(library, LibraryRoute::Home, Vec::new(), window, cx);
        cx.notify();
        Ok(true)
    }

    /// Replaces the catalogue. If the active entry disappeared, fallback
    /// selection and `LibraryRoot` replacement happen here rather than inside
    /// the passive state object.
    pub(crate) fn replace_catalogue(&mut self, libraries: Vec<LibraryEntry>, preferred_library_id: Option<String>, window: &mut Window, cx: &mut Context<Self>) -> Result<bool, String> {
        let current_id = self.state.read(cx).library_id().map(str::to_owned);
        let preferred_id = preferred_library_id.filter(|id| libraries.iter().any(|entry| entry.library_id().as_str() == id));
        let target_id = preferred_id.or_else(|| current_id.clone().filter(|id| libraries.iter().any(|entry| entry.library_id().as_str() == id))).or_else(|| libraries.first().map(|entry| entry.library_id().as_str().to_owned()));

        if target_id == current_id && current_id.is_some() {
            self.state.update(cx, |state, cx| state.set_entries(libraries, cx));
            return Ok(false);
        }

        let Some(target_id) = target_id else {
            self.state.update(cx, |state, cx| state.replace(libraries, None, cx));
            self.drop_library_root(cx);
            return Ok(current_id.is_some());
        };
        let library = match resolve_library(&self.services, &target_id) {
            Ok(library) => library,
            Err(error) => {
                self.state.update(cx, |state, cx| state.replace(libraries, None, cx));
                self.drop_library_root(cx);
                return Err(error);
            }
        };
        self.state.update(cx, |state, cx| state.replace(libraries, Some(target_id), cx));
        self.install_library_root(library, LibraryRoute::Home, Vec::new(), window, cx);
        cx.notify();
        Ok(true)
    }

    pub(crate) fn remove_library(&mut self, library_id: &str, window: &mut Window, cx: &mut Context<Self>) -> Result<bool, String> {
        self.services.backend.remove_library(library_id).map_err(|error| error.to_string())?;
        let libraries = self.services.backend.libraries().map_err(|error| error.to_string())?;
        self.replace_catalogue(libraries, None, window, cx)
    }

    pub(crate) fn refresh_active(&mut self, cx: &mut Context<Self>) {
        if let HostedLibrary::Active { root, .. } = &self.hosted {
            root.clone().update(cx, |root, cx| root.refresh_active(cx));
        }
    }

    pub(crate) fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let HostedLibrary::Active { root, .. } = &self.hosted {
            root.clone().update(cx, |root, cx| root.focus_search(window, cx));
        }
    }

    pub(crate) fn handle_library_event(&mut self, library_id: &str, event: &LibraryEvent, cx: &mut Context<Self>) {
        if self.state.read(cx).library_id() != Some(library_id) {
            return;
        }
        if let HostedLibrary::Active { root, .. } = &self.hosted {
            root.clone().update(cx, |root, cx| root.handle_library_event(event, cx));
        }
    }

    fn install_library_root(&mut self, library: LibraryBackend, route: LibraryRoute, folder_path: Vec<FolderCrumb>, window: &mut Window, cx: &mut Context<Self>) {
        let services = LibraryContext::new(self.services.clone(), library);
        let root = cx.new(|cx| LibraryRoot::new(services, route, folder_path, window, cx));
        let subscription = cx.subscribe(&root, |_: &mut Self, _, event: &LibraryRootEvent, cx| match event {
            LibraryRootEvent::FolderPathChanged => cx.emit(LibraryHostEvent::SessionChanged),
        });
        self.hosted = HostedLibrary::Active { root, _subscription: subscription };
    }

    fn drop_library_root(&mut self, cx: &mut Context<Self>) {
        self.hosted = HostedLibrary::Empty;
        cx.notify();
    }
}

fn resolve_library(services: &AppServices, library_id: &str) -> Result<LibraryBackend, String> {
    services.backend.library(library_id).map_err(|error| {
        log::error!("failed to select library {library_id}: {error}");
        format!("Could not open library: {error}")
    })
}

impl Render for LibraryHost {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        match &self.hosted {
            HostedLibrary::Empty => gpui::div().into_any_element(),
            HostedLibrary::Active { root, .. } => root.clone().into_any_element(),
        }
    }
}
