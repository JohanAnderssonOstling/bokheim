//! Background access to a shared database connection. SQL locks never span an await.
use crate::{Database, DatabaseError};
use client_platform_runtime::executor::BackendExecutor;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use tokio::sync::oneshot;

/// Native calls use the application's blocking pool; browser calls use its
/// OPFS worker. Operations and reply handling share one implementation.
#[derive(Clone)]
pub struct DatabaseClient {
    owner: Arc<DatabaseOwner>,
}

struct DatabaseOwner {
    database: Mutex<Option<Database>>,
    executor: BackendExecutor,
    path: PathBuf,
}

impl DatabaseOwner {
    fn run<T>(&self, operation: impl FnOnce(&Database) -> Result<T, DatabaseError>) -> Result<T, DatabaseError> {
        let database = self.database.lock().unwrap_or_else(PoisonError::into_inner);
        // Catch before releasing the guard so an operation panic does not
        // poison the shared connection. Transactions roll back on unwind.
        catch_unwind(AssertUnwindSafe(|| operation(database.as_ref().expect("database connection is open")))).unwrap_or_else(|_| Err(DatabaseError::message("database operation panicked")))
    }
}

impl Drop for DatabaseOwner {
    fn drop(&mut self) {
        let database = self.database.get_mut().unwrap_or_else(PoisonError::into_inner).take();
        // The last client may belong to the UI. OPFS resources must also be
        // disposed of on their storage worker.
        self.executor.storage_cleanup(move || drop(database));
    }
}

impl std::fmt::Debug for DatabaseClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("DatabaseClient").field("path", &self.owner.path).finish_non_exhaustive()
    }
}

impl DatabaseClient {
    pub async fn open(path: impl AsRef<Path>, executor: &BackendExecutor) -> Result<Self, DatabaseError> {
        let path = path.as_ref().to_owned();
        let storage = executor.clone();
        let (reply, result) = oneshot::channel();
        executor
            .dispatch_storage(move || {
                if reply.is_closed() {
                    return;
                }
                let opened = Database::open(&path).map(|database| Self { owner: Arc::new(DatabaseOwner { database: Mutex::new(Some(database)), executor: storage, path }) });
                let _ = reply.send(opened);
            })
            .map_err(DatabaseError::operation)?;
        result.await.map_err(DatabaseError::operation)?
    }

    pub fn path(&self) -> &Path {
        &self.owner.path
    }

    pub async fn run<T: Send + 'static>(&self, operation: impl FnOnce(&Database) -> Result<T, DatabaseError> + Send + 'static) -> Result<T, DatabaseError> {
        let owner = self.owner.clone();
        let (reply, result) = oneshot::channel();
        self.owner
            .executor
            .dispatch_storage(move || {
                let output = owner.run(|database| {
                    // Cancellation before connection access must not start SQL.
                    // Once started, the batch completes while holding its mutex.
                    if reply.is_closed() {
                        return Err(DatabaseError::message("database operation was cancelled"));
                    }
                    operation(database)
                });
                let _ = reply.send(output);
            })
            .map_err(DatabaseError::operation)?;
        result.await.map_err(DatabaseError::operation)?
    }
}

impl DatabaseClient {
    pub async fn catalog_entry(&self, key: library_model::CardKey) -> Result<Option<crate::catalog::CatalogBook>, DatabaseError> {
        self.run(move |_| crate::catalog::entry(key)).await
    }

    pub async fn catalog_import_record(&self, hash: sync_common::BookId, format: book_model::BookFormat) -> Result<Option<book_model::BookRecord>, DatabaseError> {
        self.run(move |_| crate::catalog::import_record(hash, format)).await
    }
}
