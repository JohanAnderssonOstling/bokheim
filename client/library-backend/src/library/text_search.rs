//! Search indexing shares library lifecycle events and the existing CPU host.
use super::LibrarySession;
use crate::{BackendError, BookId};
use client_platform_runtime::{executor, text_search};
use library_model::{BookTextMatchPage, BookTextSearchPage, NotebookQuery, NotebookRequest};
use std::mem;
use std::time::Duration;

pub(super) async fn run(session: &LibrarySession) {
    let mut changes = session.events.text_search.subscribe();
    let mut policy = text_search::subscribe();
    loop {
        let failed = tokio::select! {
            biased;
            _ = policy.changed() => continue,
            result = pass(session) => {
                result.map_err(|error| log::warn!("book text indexing will retry: {error}")).is_err()
            },
        };
        tokio::select! {
            _ = changes.changed() => {},
            _ = policy.changed() => {},
            _ = executor::sleep(if failed { Duration::from_secs(30) } else { Duration::from_secs(24*60*60) }) => {},
        }
    }
}

const INDEX_BATCH_BOOKS: usize = 8;
const INDEX_BATCH_BYTES: usize = 8 * 1024 * 1024;

async fn pass(session: &LibrarySession) -> Result<(), BackendError> {
    let enabled = text_search::enabled();
    session.db.configure_text_index(enabled)?;
    if !enabled {
        return Ok(());
    }
    session.db.prune_text_index()?;
    let mut failed = false;
    let mut batch = Vec::with_capacity(INDEX_BATCH_BOOKS);
    let mut batch_bytes = 0;
    for book in session.db.text_index_candidates()? {
        if !text_search::enabled() {
            return Ok(());
        }
        match session.book_text(book.book, cpu_host::IndexText { format: book.format }).await {
            Ok(Some(index)) => {
                let bytes = index.text.len();
                if !batch.is_empty() && batch_bytes + bytes > INDEX_BATCH_BYTES {
                    session.db.store_book_text_batch(mem::take(&mut batch)).await?;
                    batch_bytes = 0;
                }
                batch.push((book, index));
                batch_bytes += bytes;
                // A book larger than the cap is committed on its own.
                if batch.len() >= INDEX_BATCH_BOOKS || batch_bytes >= INDEX_BATCH_BYTES {
                    session.db.store_book_text_batch(mem::take(&mut batch)).await?;
                    batch_bytes = 0;
                }
            }
            Ok(None) => {}
            Err(error) => {
                log::warn!("could not index book {}: {error}", book.book);
                failed = true;
            }
        }
        // CPU work returns between books, allowing commands and removal events to run.
    }
    session.db.store_book_text_batch(batch).await?;
    if failed { Err(BackendError::message("some book text could not be indexed")) } else { Ok(()) }
}

impl LibrarySession {
    async fn book_text<O: cpu_host::BookOperation>(&self, book: BookId, operation: O) -> Result<Option<O::Reply>, BackendError> {
        let paths = self.db.book_paths(&book)?;
        let Some(reader) = self.assets.open_book_reader(&book, paths).await.map_err(BackendError::operation)? else { return Ok(None) };
        cpu_host::submit_book(&*self.cpu, operation, cpu_host::as_cpu_reader(reader)).await.map(Some)
    }

    pub(crate) async fn text_search(&self, query: NotebookQuery, request: NotebookRequest) -> Result<Option<BookTextSearchPage>, BackendError> {
        if !text_search::enabled() {
            return Ok(Some(BookTextSearchPage::default()));
        }
        Ok(self.db.text_search(&query, &request)?)
    }

    pub(crate) async fn text_search_matches(&self, query: NotebookQuery, revision: String, book: BookId, offset: usize, limit: usize) -> Result<Option<BookTextMatchPage>, BackendError> {
        if !text_search::enabled() {
            return Ok(None);
        }
        let Some(mut prepared) = self.db.text_search_matches(&query, &revision, book, offset, limit)? else { return Ok(None) };
        if prepared.locations.iter().any(|location| !location.documents.is_empty()) {
            if let Some(matches) = self.book_text(book, cpu_host::TextMatches { format: prepared.format, locations: prepared.locations, query: query.search }).await? {
                prepared.page.matches = matches;
            } else {
                self.db.remove_text_match(book, &prepared.revision)?;
            }
        }
        Ok(Some(prepared.page))
    }
}
