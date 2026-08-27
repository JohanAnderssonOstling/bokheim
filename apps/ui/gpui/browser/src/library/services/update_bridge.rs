//! Translates backend library updates into GPUI events.

use std::rc::Rc;

use gpui::{AppContext, Context, Entity, EventEmitter, Task};
use library_model::LibraryUpdate;
use sync_common::{ContentHash, LibraryId};

use crate::services::AppServices;

pub(crate) struct LibraryContentsChanged;

pub(crate) struct LibraryCoversChanged {
    content_hashes: Rc<[ContentHash]>,
}

impl LibraryCoversChanged {
    pub(crate) fn contains(&self, content_hash: &ContentHash) -> bool {
        self.content_hashes.contains(content_hash)
    }
}

pub(crate) struct LibraryUpdateBridge {
    _updates: Task<()>,
}

impl EventEmitter<LibraryContentsChanged> for LibraryUpdateBridge {}
impl EventEmitter<LibraryCoversChanged> for LibraryUpdateBridge {}

impl LibraryUpdateBridge {
    pub(crate) fn create<C: AppContext>(services: &AppServices, library_id: LibraryId, cx: &mut C) -> Entity<Self> {
        let updates = services.backend.library_updates(library_id);
        cx.new(|cx| Self::new(updates, cx))
    }

    fn new(updates: async_channel::Receiver<LibraryUpdate>, cx: &mut Context<Self>) -> Self {
        let task = cx.spawn(async move |bridge, cx| {
            while let Ok(update) = updates.recv().await {
                let _ = bridge.update(cx, |bridge, cx| bridge.apply(update, cx));
            }
        });
        Self { _updates: task }
    }

    fn apply(&mut self, update: LibraryUpdate, cx: &mut Context<Self>) {
        match update {
            LibraryUpdate::Contents => cx.emit(LibraryContentsChanged),
            LibraryUpdate::Covers(content_hashes) => {
                cx.emit(LibraryCoversChanged { content_hashes: content_hashes.into() });
            }
        }
    }
}
