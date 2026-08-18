//! Controller and lifetime boundary for the selected library.
//!
//! This is the sole writer of [`LibraryState`]. Its empty/active state ties the
//! selected library's root and subscription to the same lifetime.

mod state;

use epub_svelte_lib::LibraryId;
use epub_svelte_lib::app::LibraryEntry;
use epub_svelte_lib::library::LibraryBackend;
use epub_svelte_lib::platform::scanner::LibraryEvent;
use gpui::{App, AppContext, Context, Entity, EventEmitter, IntoElement, Render, Subscription, Window};

use super::services::LibraryContext;
use super::{LibraryRoot, LibraryRootEvent};
use crate::model::{BrowserSession, LibraryPlace};
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
    /// Where browsing was left off in each library. The entry for the *active*
    /// library is stale while it is active — the live position lives in the
    /// `LibraryRoot` — and is written back by [`Self::store_active_place`]
    /// before that root is replaced.
    places: std::collections::HashMap<epub_svelte_lib::LibraryId, LibraryPlace>,
}

enum HostedLibrary {
    Empty,
    Active { root: Entity<LibraryRoot>, _subscription: Subscription },
}

impl EventEmitter<LibraryHostEvent> for LibraryHost {}

impl LibraryHost {
    pub(crate) fn new(libraries: Vec<LibraryEntry>, session: &BrowserSession, services: AppServices, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let requested_id = session.active_library_id.filter(|id| libraries.iter().any(|entry| entry.library_id() == id)).or_else(|| libraries.first().map(|entry| *entry.library_id()));
        let selected = requested_id.and_then(|library_id| match resolve_library(&services, &library_id) {
            Ok(library) => Some((library_id, library)),
            Err(error) => {
                log::error!("failed to restore library selection: {error}");
                None
            }
        });
        let (library_id, library) = selected.map(|(id, library)| (Some(id), Some(library))).unwrap_or((None, None));
        let state = cx.new(|_| LibraryState::new(libraries, library_id));
        let places = session.places.clone();
        let mut host = Self { services, state, hosted: HostedLibrary::Empty, places };
        if let Some(library) = library {
            let place = session.place(&library_id.expect("a resolved library has an id"));
            host.install_library_root(library, place, window, cx);
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
        let active_library_id = self.state.read(cx).library_id().copied();
        let mut places = self.places.clone();
        // The stored entry for the active library is stale by construction, so
        // read the live position out of the root rather than trusting it.
        if let (Some(library_id), Some(place)) = (active_library_id, self.active_place(cx)) {
            places.insert(library_id, place);
        }
        BrowserSession { active_library_id, places }
    }

    fn active_place(&self, cx: &App) -> Option<LibraryPlace> {
        match &self.hosted {
            HostedLibrary::Empty => None,
            HostedLibrary::Active { root, .. } => {
                let root = root.read(cx);
                Some(LibraryPlace { route: root.route(), folder_path: root.folder_path(), subject_path: root.subject_path() })
            }
        }
    }

    /// Writes the live position back into the map. Must run before the hosted
    /// root is replaced, or the outgoing library forgets where it was.
    fn store_active_place(&mut self, cx: &App) {
        if let (Some(library_id), Some(place)) = (self.state.read(cx).library_id().copied(), self.active_place(cx)) {
            self.places.insert(library_id, place);
        }
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

    /// Dismisses a transient book detail before application-level navigation
    /// decides whether the destination route itself changed.
    pub(crate) fn dismiss_detail(&mut self, cx: &mut Context<Self>) -> bool {
        match &self.hosted {
            HostedLibrary::Active { root, .. } => root.clone().update(cx, |root, cx| root.dismiss_detail(cx)),
            HostedLibrary::Empty => false,
        }
    }

    /// Selects the backend library, publishes the passive state and replaces
    /// the library-scoped UI as one controller operation.
    pub(crate) fn activate_library(&mut self, library_id: LibraryId, window: &mut Window, cx: &mut Context<Self>) -> Result<bool, String> {
        // Re-picking the library you are already in is not a navigation. It
        // used to force Home, which threw away the user's place for no reason.
        if self.state.read(cx).library_id() == Some(&library_id) {
            return Ok(false);
        }
        self.store_active_place(cx);
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
        let place = self.places.get(&library_id).cloned().unwrap_or_default();
        self.install_library_root(library, place, window, cx);
        cx.notify();
        Ok(true)
    }

    /// Replaces the catalogue. If the active entry disappeared, fallback
    /// selection and `LibraryRoot` replacement happen here rather than inside
    /// the passive state object.
    pub(crate) fn replace_catalogue(&mut self, libraries: Vec<LibraryEntry>, preferred_library_id: Option<LibraryId>, window: &mut Window, cx: &mut Context<Self>) -> Result<bool, String> {
        crate::model::retain_library_places(&mut self.places, |library_id| libraries.iter().any(|entry| entry.library_id() == library_id));
        let current_id = self.state.read(cx).library_id().copied();
        let preferred_id = preferred_library_id.filter(|id| libraries.iter().any(|entry| entry.library_id() == id));
        let target_id = preferred_id.or_else(|| current_id.filter(|id| libraries.iter().any(|entry| entry.library_id() == id))).or_else(|| libraries.first().map(|entry| *entry.library_id()));

        if target_id == current_id && current_id.is_some() {
            self.state.update(cx, |state, cx| state.set_entries(libraries, cx));
            return Ok(false);
        }
        self.store_active_place(cx);

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
        let place = self.places.get(&target_id).cloned().unwrap_or_default();
        self.install_library_root(library, place, window, cx);
        cx.notify();
        Ok(true)
    }

    pub(crate) fn refresh_after_library_removal(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Result<bool, String> {
        let libraries = self.services.backend.libraries().map_err(|error| error.to_string())?;
        self.replace_catalogue(libraries, None, window, cx)
    }

    pub(crate) fn set_library_view(&mut self, library_view: app_contract::preferences::LibraryView, cx: &mut Context<Self>) {
        if let HostedLibrary::Active { root, .. } = &self.hosted {
            root.clone().update(cx, |root, cx| root.set_library_view(library_view, cx));
        }
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

    pub(crate) fn handle_library_event(&mut self, library_id: &LibraryId, event: &LibraryEvent, cx: &mut Context<Self>) {
        if self.state.read(cx).library_id() != Some(library_id) {
            return;
        }
        if let HostedLibrary::Active { root, .. } = &self.hosted {
            root.clone().update(cx, |root, cx| root.handle_library_event(event, cx));
        }
    }

    fn install_library_root(&mut self, library: LibraryBackend, place: LibraryPlace, window: &mut Window, cx: &mut Context<Self>) {
        let scanner_backend = self.services.backend.clone();
        let services = LibraryContext::new(self.services.clone(), library);
        let root = cx.new(|cx| LibraryRoot::new(services, place.route, place.folder_path, place.subject_path, window, cx));
        let subscription = cx.subscribe(&root, |_: &mut Self, _, event: &LibraryRootEvent, cx| match event {
            LibraryRootEvent::FolderPathChanged => cx.emit(LibraryHostEvent::SessionChanged),
            LibraryRootEvent::SubjectPathChanged => cx.emit(LibraryHostEvent::SessionChanged),
        });
        self.hosted = HostedLibrary::Active { root, _subscription: subscription };
        cx.on_next_frame(window, move |_, _, _| {
            if let Err(error) = scanner_backend.enable_library_scanning() {
                log::error!("failed to start library scanning after the first frame: {error}");
            }
        });
    }

    fn drop_library_root(&mut self, cx: &mut Context<Self>) {
        self.hosted = HostedLibrary::Empty;
        cx.notify();
    }
}

fn resolve_library(services: &AppServices, library_id: &LibraryId) -> Result<LibraryBackend, String> {
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
