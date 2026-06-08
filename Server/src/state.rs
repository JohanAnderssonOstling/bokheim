use sqlx::PgPool;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex};
use tokio::sync::Mutex as AsyncMutex;

pub type LibraryLock = Arc<AsyncMutex<()>>;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub books_dir: PathBuf,
    pub thumbnails_dir: PathBuf,
    pub library_locks: Arc<StdMutex<HashMap<String, LibraryLock>>>,
}

impl AppState {
    pub fn new(pool: PgPool, books_dir: PathBuf, thumbnails_dir: PathBuf) -> Self {
        Self { pool, books_dir, thumbnails_dir, library_locks: Arc::new(StdMutex::new(HashMap::new())) }
    }

    /// Per-library async mutex. Serializes pushes to one library (so server_seq
    /// is assigned in commit order) while letting different libraries proceed in
    /// parallel.
    pub fn library_lock(&self, library_id: &str) -> LibraryLock {
        let mut map = self.library_locks.lock().unwrap();
        map.entry(library_id.to_string())
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone()
    }
}
