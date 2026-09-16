//! Translates backend library updates into GPUI events.

use std::rc::Rc;

use bokheim_backend::LibraryClient;
use gpui::{AppContext, Context, Entity, EventEmitter, Task};
use library_model::LibraryUpdate;
use sync_common::{ContentHash, LibraryId};

pub(crate) struct LibraryContentsChanged;

pub(crate) struct LibraryScanChanged;

pub(crate) struct LibraryCoversChanged {
    content_hashes: Rc<[ContentHash]>,
}

pub(crate) struct LibraryDownloadChanged {
    pub(crate) status: bokheim_backend::BookDownloadStatus,
}

impl LibraryCoversChanged {
    pub(crate) fn contains(&self, content_hash: &ContentHash) -> bool {
        self.content_hashes.contains(content_hash)
    }
}

pub(crate) struct LibraryUpdateBridge {
    library: LibraryClient,
    scanning: Option<bool>,
    importing: bool,
    book_count: Option<usize>,
    _updates: Task<()>,
    _book_count_request: Option<Task<()>>,
}

impl EventEmitter<LibraryContentsChanged> for LibraryUpdateBridge {}
impl EventEmitter<LibraryCoversChanged> for LibraryUpdateBridge {}
impl EventEmitter<LibraryDownloadChanged> for LibraryUpdateBridge {}
impl EventEmitter<LibraryScanChanged> for LibraryUpdateBridge {}

impl LibraryUpdateBridge {
    pub(crate) fn create<C: AppContext>(backend: Rc<bokheim_backend::AppClient>, library_id: LibraryId, cx: &mut C) -> Entity<Self> {
        cx.new(|cx| Self::new(backend, library_id, cx))
    }

    fn new(backend: Rc<bokheim_backend::AppClient>, library_id: LibraryId, cx: &mut Context<Self>) -> Self {
        let library = backend.library(library_id);
        let update_library = library.clone();
        let task = cx.spawn(async move |bridge, cx| {
            let (updates, scanning) = match update_library.updates().await {
                Ok(state) => state,
                Err(error) => {
                    log::error!("failed to subscribe to library updates: {error}");
                    return;
                }
            };
            let _ = bridge.update(cx, |bridge, cx| {
                bridge.scanning = Some(scanning);
                cx.emit(LibraryScanChanged);
                cx.notify();
            });
            while let Ok(update) = updates.recv().await {
                // Everything already queued describes the same instant, so it
                // is answered together. "Contents changed" is idempotent — ten
                // copies ask for exactly what one asks for — and cover
                // notifications compose by union, so a burst costs one reload
                // instead of one per book.
                let mut batch = UpdateBatch::default();
                batch.absorb(update);
                while let Ok(update) = updates.try_recv() {
                    batch.absorb(update);
                }
                let _ = bridge.update(cx, |bridge, cx| bridge.apply(batch, cx));
            }
        });
        let mut bridge = Self { library, scanning: None, importing: false, book_count: None, _updates: task, _book_count_request: None };
        bridge.refresh_book_count(cx);
        bridge
    }

    pub(crate) fn set_importing(&mut self, importing: bool, cx: &mut Context<Self>) {
        self.importing = importing;
        if !importing {
            self.book_count = None;
            self.refresh_book_count(cx);
        }
        cx.emit(LibraryScanChanged);
        cx.notify();
    }

    pub(crate) fn scanning(&self) -> bool {
        self.importing || self.scanning == Some(true)
    }

    pub(crate) fn loading(&self) -> bool {
        self.scanning.is_none() || self.book_count.is_none()
    }

    pub(crate) fn book_count(&self) -> Option<usize> {
        self.book_count
    }

    fn refresh_book_count(&mut self, cx: &mut Context<Self>) {
        let library = self.library.clone();
        let load = async move { library.book_count().await };
        self._book_count_request = Some(cx.spawn(async move |bridge, cx| {
            let result = load.await;
            let _ = bridge.update(cx, |bridge, cx| match result {
                Ok(book_count) => {
                    if bridge.book_count != Some(book_count) {
                        bridge.book_count = Some(book_count);
                        cx.notify();
                    }
                }
                Err(error) => log::warn!("failed to determine whether the selected library is empty: {error}"),
            });
        }));
    }

    fn apply(&mut self, batch: UpdateBatch, cx: &mut Context<Self>) {
        let UpdateBatch { contents, covers, downloads, scanning } = batch;
        if let Some(scanning) = scanning {
            self.scanning = Some(scanning);
            if !scanning {
                self.book_count = None;
            }
            cx.emit(LibraryScanChanged);
            cx.notify();
        }
        if contents || scanning == Some(false) {
            self.refresh_book_count(cx);
        }
        if contents {
            cx.emit(LibraryContentsChanged);
        }
        if !covers.is_empty() {
            cx.emit(LibraryCoversChanged { content_hashes: covers.into() });
        }
        for status in downloads {
            cx.emit(LibraryDownloadChanged { status });
        }
    }
}

/// Everything that arrived before the UI could answer, folded by how each kind
/// of update composes.
#[derive(Default)]
struct UpdateBatch {
    contents: bool,
    covers: Vec<ContentHash>,
    /// Kept per book: each status names a different book, or supersedes an
    /// earlier status for the same one.
    downloads: Vec<bokheim_backend::BookDownloadStatus>,
    scanning: Option<bool>,
}

impl UpdateBatch {
    fn absorb(&mut self, update: LibraryUpdate) {
        match update {
            LibraryUpdate::Contents => self.contents = true,
            LibraryUpdate::Covers(content_hashes) => self.covers.extend(content_hashes),
            LibraryUpdate::Download(status) => {
                self.downloads.retain(|existing| existing.content_hash != status.content_hash);
                self.downloads.push(status);
            }
            LibraryUpdate::Scanning(scanning) => self.scanning = Some(scanning),
        }
    }
}
