use std::rc::Rc;
use std::sync::Arc;

use app_contract::FrontendContract;
use app_contract::backend::{AppBackend, LibraryBackend};
use app_contract::library::BookLocator;
use gpui::{App, AppContext, Entity, Window};
use sync_common::ContentHash;

use super::LibraryUpdateBridge;
use crate::services::AppServices;

pub(crate) type HomeBrowseRows = app_contract::library::LibraryHomeView;

/// UI state associated with one selected library.
///
/// Backend operations remain on the two backend traits; this value only keeps
/// the selected trait object together with GPUI-specific callbacks and events.
#[derive(Clone)]
pub(crate) struct LibraryContext {
    app: AppServices,
    backend: Rc<dyn LibraryBackend>,
    updates: Entity<LibraryUpdateBridge>,
}

impl LibraryContext {
    pub(crate) fn new<C: AppContext>(app: AppServices, backend: Rc<dyn LibraryBackend>, cx: &mut C) -> Self {
        let updates = LibraryUpdateBridge::create(&app, backend.id(), cx);
        Self { app, backend, updates }
    }

    pub(crate) fn backend(&self) -> &dyn LibraryBackend {
        self.backend.as_ref()
    }

    pub(crate) fn app_backend(&self) -> &dyn AppBackend {
        self.app.backend.as_ref()
    }

    pub(crate) fn updates(&self) -> Entity<LibraryUpdateBridge> {
        self.updates.clone()
    }

    pub(crate) fn contracts(&self) -> &Arc<FrontendContract> {
        &self.app.contracts
    }

    pub(crate) fn open_reader(&self, content_hash: ContentHash, title: String, window: &mut Window, cx: &mut App) {
        let locator = BookLocator::new(self.backend.id(), content_hash);
        (self.app.open_reader)(locator, title, None, window, cx);
    }
}
