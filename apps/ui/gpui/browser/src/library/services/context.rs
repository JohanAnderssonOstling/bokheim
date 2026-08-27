use bokheim_backend::{AppBackend, LibraryBackend};
use gpui::{App, AppContext, Entity, Window};
use library_model::BookLocator;
use std::rc::Rc;
use sync_common::ContentHash;

use super::LibraryUpdateBridge;
use crate::services::AppServices;

pub(crate) type HomeBrowseRows = library_model::LibraryHomeView;

/// UI state associated with one selected library.
///
/// Keeps the selected concrete backend together with GPUI-specific callbacks
/// and events.
#[derive(Clone)]
pub(crate) struct LibraryContext {
    app: AppServices,
    backend: Rc<LibraryBackend>,
    updates: Entity<LibraryUpdateBridge>,
}

impl LibraryContext {
    pub(crate) fn new<C: AppContext>(app: AppServices, backend: Rc<LibraryBackend>, cx: &mut C) -> Self {
        let updates = LibraryUpdateBridge::create(&app, *backend.id(), cx);
        Self { app, backend, updates }
    }

    pub(crate) fn backend(&self) -> &LibraryBackend {
        self.backend.as_ref()
    }

    pub(crate) fn app_backend(&self) -> &AppBackend {
        self.app.backend.as_ref()
    }

    pub(crate) fn updates(&self) -> Entity<LibraryUpdateBridge> {
        self.updates.clone()
    }

    pub(crate) fn open_reader(&self, content_hash: ContentHash, title: String, window: &mut Window, cx: &mut App) {
        let locator = BookLocator::new(*self.backend.id(), content_hash);
        (self.app.open_reader)(locator, title, None, window, cx);
    }
}
