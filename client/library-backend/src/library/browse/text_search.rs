//! Search indexing shares library lifecycle events and the existing CPU host.
use crate::asset_store::AssetStore;
use crate::library::{LibraryHandle, LibraryResources};
use crate::{BackendError, BookId};
use client_platform_runtime::{executor, text_search};
use cpu_host::{BookOperation, CpuHost, IndexText, TextMatches};
use library_database::{text_search::TextMatchPage, DatabaseClient};
use library_model::{BookTextMatchPage, BookTextSearchPage, NotebookQuery, NotebookRequest};
use std::mem;
use std::time::Duration;
use tokio::sync::watch;

pub(in crate::library) async fn run(session: &LibraryResources, mut changes: watch::Receiver<()>) {
    let mut policy = text_search::subscribe();
    loop {
        let failed = tokio::select! {
            biased;
            _ = policy.changed() => continue,
            result = async {
                pass(session).await
            } => {
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

async fn pass(session: &LibraryResources) -> Result<(), BackendError> {
    let (database, assets) = {
        let storage = session.state.read().await;
        (storage.database().clone(), storage.assets().clone())
    };
    let cpu = &*session.cpu;
    let candidates = database.prepare_text_index().await?;
    let mut failed = false;
    let mut batch = Vec::with_capacity(INDEX_BATCH_BOOKS);
    let mut batch_bytes = 0;
    for book in candidates {
        if !text_search::enabled() {
            return Ok(());
        }
        let paths = database.book_paths(&book.book).await?;
        match book_text(&assets, cpu, book.book, paths, IndexText { format: book.format }).await {
            Ok(Some(index)) => {
                let bytes = index.text.len();
                if !batch.is_empty() && batch_bytes + bytes > INDEX_BATCH_BYTES {
                    database.store_book_text_batch(mem::take(&mut batch)).await?;
                    batch_bytes = 0;
                }
                batch.push((book, index));
                batch_bytes += bytes;
                // A book larger than the cap is committed on its own.
                if batch.len() >= INDEX_BATCH_BOOKS || batch_bytes >= INDEX_BATCH_BYTES {
                    database.store_book_text_batch(mem::take(&mut batch)).await?;
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
    if !batch.is_empty() {
        database.store_book_text_batch(batch).await?;
    }
    if failed {
        Err(BackendError::message("some book text could not be indexed"))
    } else {
        Ok(())
    }
}

async fn book_text<O: BookOperation>(assets: &AssetStore, cpu: &dyn CpuHost, book: BookId, paths: Vec<library_model::RelativeBookPath>, operation: O) -> Result<Option<O::Reply>, BackendError> {
    let Some(reader) = assets.open_book_reader(&book, paths).await.map_err(BackendError::operation)? else { return Ok(None) };
    cpu_host::submit_book(cpu, operation, cpu_host::as_cpu_reader(reader)).await.map(Some)
}

pub(super) async fn search(db: &DatabaseClient, query: NotebookQuery, request: NotebookRequest) -> Result<Option<BookTextSearchPage>, BackendError> {
    if !text_search::enabled() {
        return Ok(Some(BookTextSearchPage::default()));
    }
    Ok(db.text_search(&query, &request).await?)
}

pub(super) async fn resolve_matches(session: &LibraryResources, book: BookId, search: String, prepared: Option<TextMatchPage>) -> Result<Option<BookTextMatchPage>, BackendError> {
    let Some(mut prepared) = prepared else { return Ok(None) };
    if prepared.locations.iter().any(|location| !location.documents.is_empty()) {
        // Keep paths and opening the reader together, then release the gate
        // before matching text on the CPU host.
        let (database, reader) = {
            let storage = session.state.read().await;
            let paths = storage.database().book_paths(&book).await?;
            let reader = storage.assets().open_book_reader(&book, paths).await.map_err(BackendError::operation)?;
            (storage.database().clone(), reader)
        };
        if let Some(reader) = reader {
            prepared.page.matches = cpu_host::submit_book(&*session.cpu, TextMatches { format: prepared.format, locations: prepared.locations, query: search }, cpu_host::as_cpu_reader(reader)).await?;
        } else {
            database.remove_text_match(book, &prepared.revision).await?;
        }
    }
    Ok(Some(prepared.page))
}

impl LibraryHandle {
    pub async fn text_search_index_bytes(&self) -> Result<u64, BackendError> {
        let database = self.database();
        Ok(database.text_index_bytes().await?)
    }

    pub async fn cleanup_text_search_index(&self) -> Result<u64, BackendError> {
        let database = self.database();
        if !client_platform_runtime::text_search::enabled() {
            database.configure_text_index(false).await?;
        }
        Ok(0_u64)
    }

    pub async fn text_search(&self, query: library_model::NotebookQuery, request: library_model::NotebookRequest) -> Result<Option<library_model::BookTextSearchPage>, BackendError> {
        let database = self.database();
        Ok(search(database, query, request).await?)
    }

    pub async fn text_search_matches(&self, query: library_model::NotebookQuery, revision: String, book: BookId, offset: usize, limit: usize) -> Result<Option<library_model::BookTextMatchPage>, BackendError> {
        self.run_io(async {
            let library = &self.owner.resources;
            let database = &library.database;
            let prepared = if client_platform_runtime::text_search::enabled() { database.text_search_matches(&query, &revision, book, offset, limit).await? } else { None };
            resolve_matches(&library, book, query.search, prepared).await
        })
        .await
    }
}
