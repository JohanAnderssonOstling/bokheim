//! Asynchronous library persistence coordinated by the reader.
//!
//! The backend owns any platform-specific blocking work. This module only
//! folds loaded data back into its entity, serializes optimistic mutations,
//! and debounces position updates.

use library_backend::ContentHash;
use std::cell::RefCell;
use std::pin::Pin;
use std::rc::Rc;
use std::time::Duration;

use gpui::{App, Context};
use library_backend::LibraryClient;

pub(crate) type PersistenceFuture<T> = Pin<Box<dyn Future<Output = Result<T, String>> + 'static>>;
type Mutation = Box<dyn FnOnce(&LibraryClient) -> PersistenceFuture<()> + 'static>;

/// Awaits `op` against the library, then applies `apply` to the view with its
/// result.
///
/// `what` names the write for log messages, phrased to follow "failed to" —
/// for example `"save annotation"`. If the operation fails, the failure is
/// logged and `apply` is not called.
pub(crate) fn write<V, T>(view: &Context<V>, library: Rc<LibraryClient>, what: &'static str, op: impl FnOnce(&LibraryClient) -> PersistenceFuture<T> + 'static, apply: impl FnOnce(&mut V, &mut Context<V>, T) + 'static)
where
    V: 'static,
    T: 'static,
{
    view.spawn(async move |this, cx| {
        let result = op(library.as_ref()).await;
        let _ = this.update(cx, |this, cx| match result {
            Ok(value) => apply(this, cx, value),
            Err(error) => log::error!("failed to {what}: {error}"),
        });
    })
    .detach();
}

/// Serializes durable mutations independently of UI updates.
///
/// Reader state is changed optimistically on the GPUI thread. This queue then
/// preserves the same order in storage.
pub(crate) struct MutationWriter {
    mutations: async_channel::Sender<(&'static str, Mutation)>,
}

impl MutationWriter {
    pub(crate) fn new(cx: &App, library: Rc<LibraryClient>) -> Self {
        let (mutations, incoming) = async_channel::unbounded::<(&'static str, Mutation)>();
        cx.spawn(async move |_cx| {
            while let Ok((what, mutation)) = incoming.recv().await {
                if let Err(error) = mutation(library.as_ref()).await {
                    log::error!("failed to {what}: {error}");
                }
            }
        })
        .detach();
        Self { mutations }
    }

    pub(crate) fn record(&self, what: &'static str, mutation: impl FnOnce(&LibraryClient) -> PersistenceFuture<()> + 'static) {
        if self.mutations.try_send((what, Box::new(mutation))).is_err() {
            log::error!("failed to queue {what}");
        }
    }
}

/// How long a position update waits for a quieter one to supersede it.
const POSITION_DEBOUNCE: Duration = Duration::from_secs(1);

/// Coalesces a stream of reading-position updates into occasional library
/// writes.
///
/// Page turns arrive far faster than the database should be written. Each
/// update waits out [`POSITION_DEBOUNCE`]; anything that queued up meanwhile is
/// drained and only the newest position is persisted.
pub(crate) struct PositionWriter<P> {
    updates: async_channel::Sender<P>,
}

impl<P: 'static> PositionWriter<P> {
    /// Starts the debouncing task. `persist` performs the actual library
    /// write for the newest position; `what` names it for log messages.
    pub(crate) fn new(cx: &App, what: &'static str, persist: impl Fn(&LibraryClient, &P) -> PersistenceFuture<()> + 'static, library: Rc<LibraryClient>) -> Self {
        let (updates, incoming) = async_channel::unbounded::<P>();
        let executor = cx.background_executor().clone();
        let persist = Rc::new(persist);
        cx.spawn(async move |_cx| {
            while let Ok(mut position) = incoming.recv().await {
                executor.timer(POSITION_DEBOUNCE).await;
                while let Ok(newer) = incoming.try_recv() {
                    position = newer;
                }
                if let Err(error) = persist(library.as_ref(), &position).await {
                    log::error!("failed to {what}: {error}");
                }
            }
        })
        .detach();
        Self { updates }
    }

    /// Records a new position. Dropping it because the task has stopped is
    /// logged once per update and is not otherwise fatal.
    pub(crate) fn record(&self, position: P, what: &'static str) {
        if self.updates.try_send(position).is_err() {
            log::warn!("{what} persistence task stopped");
        }
    }
}

/// One reading position as the library stores it.
///
/// Every format reduces to this pair before persistence, so no reader decides
/// on its own whether to report progress or in which unit. `percent` is always
/// 0–100: readers work in their own coordinates — an EPUB layout fraction, a
/// page out of a count — and convert exactly once, in the constructor for
/// their format.
#[derive(Clone)]
pub(crate) struct StoredPosition {
    position: String,
    percent: f32,
    /// The navigation entry this position falls inside, where the reader knows
    /// it. Only the renderer can resolve a CFI or a page to an entry, so the
    /// library cannot work it out later — it travels with the position instead.
    entry: Option<String>,
}

impl StoredPosition {
    /// The renderer reports how far through the book layout has reached as a
    /// 0–1 fraction.
    pub(crate) fn epub(cfi: String, fraction: f32) -> Self {
        Self { position: cfi, percent: book_model::layout_fraction_progress(fraction), entry: None }
    }

    /// Pages are a PDF's only progress signal: there is no reflowed text to
    /// weigh, so page N of M is both what the toolbar shows and what is
    /// stored. `offset` locates the view within the page and rides along in
    /// the position string without affecting progress.
    pub(crate) fn pdf_page(page_index: usize, offset: f32, page_count: usize) -> Self {
        Self { position: book_model::pdf_page_position_reading_position(page_index as u32, offset), percent: book_model::pdf_page_progress(page_index, page_count), entry: Some(book_model::pdf_toc_target(page_index)) }
    }

    /// The entry resolved from this reading position. It may become available
    /// after the publication table of contents finishes loading.
    pub(crate) fn within(mut self, entry: Option<String>) -> Self {
        self.entry = entry;
        self
    }

    /// The stored 0–100, so a toolbar can show exactly what was persisted.
    pub(crate) fn percent(&self) -> f32 {
        self.percent
    }
}

/// The debounced writer for the reading-position register.
///
/// Wraps [`PositionWriter`] with the one payload and the one library call that
/// every format shares, leaving readers nothing to vary but their own
/// coordinates.
pub(crate) struct ReadingPositionWriter {
    writer: PositionWriter<StoredPosition>,
    narration_position: RefCell<Option<(u64, StoredPosition)>>,
}

impl ReadingPositionWriter {
    pub(crate) fn new(cx: &App, content_hash: ContentHash, library: Rc<LibraryClient>) -> Self {
        let writer = PositionWriter::new(
            cx,
            "save reading position",
            move |library, position: &StoredPosition| {
                let stored = position.position.clone();
                let percent = position.percent;
                let entry = position.entry.clone();
                let library = library.clone();
                Box::pin(async move { library.update_reading_position(content_hash, stored, Some(percent), entry).await })
            },
            library,
        );
        Self { writer, narration_position: RefCell::new(None) }
    }

    pub(crate) fn record(&self, position: StoredPosition, navigation_revision: u64) {
        let mut narration = self.narration_position.borrow_mut();
        let position = match narration.as_ref() {
            Some((revision, spoken)) if *revision == navigation_revision => spoken.clone(),
            _ => {
                *narration = None;
                position
            }
        };
        self.writer.record(position, "reading position");
    }

    /// Keep the audible source address until the user navigates. Following a
    /// passage or reflowing its page may emit a coarser viewport position later.
    pub(crate) fn record_narration(&self, position: StoredPosition, navigation_revision: u64) {
        *self.narration_position.borrow_mut() = Some((navigation_revision, position.clone()));
        self.writer.record(position, "reading position");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn writer() -> (ReadingPositionWriter, async_channel::Receiver<StoredPosition>) {
        let (updates, incoming) = async_channel::unbounded();
        (ReadingPositionWriter { writer: PositionWriter { updates }, narration_position: RefCell::new(None) }, incoming)
    }

    #[test]
    fn spoken_epub_chunks_survive_coarser_viewport_updates_until_manual_navigation() {
        let (writer, incoming) = writer();
        let viewport = || StoredPosition::epub("epubcfi(/6/2!/4/2/1:0)".into(), 0.2).within(Some("chapter.xhtml".into()));
        let spoken = "epubcfi(/6/2!/4/2/1:120)";
        writer.record_narration(StoredPosition::epub(spoken.into(), 0.2).within(Some("chapter.xhtml".into())), 7);
        assert_eq!(incoming.try_recv().unwrap().position, spoken);

        // Layout can publish its page start after the playback event, including
        // while narration is paused or after it has stopped.
        writer.record(viewport(), 7);
        let saved = incoming.try_recv().unwrap();
        assert_eq!(saved.position, spoken);
        assert_eq!(saved.percent, 20.0);
        assert_eq!(saved.entry.as_deref(), Some("chapter.xhtml"));

        let next_chunk = "epubcfi(/6/2!/4/2/1:240)";
        writer.record_narration(StoredPosition::epub(next_chunk.into(), 0.2), 7);
        assert_eq!(incoming.try_recv().unwrap().position, next_chunk);
        writer.record(viewport(), 7);
        assert_eq!(incoming.try_recv().unwrap().position, next_chunk);

        writer.record(viewport(), 8);
        assert_eq!(incoming.try_recv().unwrap().position, viewport().position);
        assert!(writer.narration_position.borrow().is_none());
    }

    #[test]
    fn spoken_pdf_block_keeps_its_full_page_offset_until_manual_navigation() {
        let (writer, incoming) = writer();
        let spoken = || StoredPosition::pdf_page(4, 0.63, 20);
        writer.record_narration(spoken(), 3);
        assert_eq!(incoming.try_recv().unwrap().position, spoken().position);

        writer.record(StoredPosition::pdf_page(4, 0.5, 20), 3);
        let saved = incoming.try_recv().unwrap();
        assert_eq!(saved.position, spoken().position);
        assert_eq!(saved.percent, spoken().percent);
        assert_eq!(saved.entry, spoken().entry);

        let navigated = StoredPosition::pdf_page(5, 0.0, 20);
        let expected = navigated.position.clone();
        writer.record(navigated, 4);
        assert_eq!(incoming.try_recv().unwrap().position, expected);
        assert!(writer.narration_position.borrow().is_none());
    }
}
