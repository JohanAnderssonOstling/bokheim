use epub_svelte_lib::app::AppBackend;
use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};

pub(crate) struct AppState {
    pub(crate) core: AppBackend,
    // Shared by remote EPUB streaming and native PDF range reads: both just
    // need a seekable reader kept alive across a series of IPC range requests.
    pub(crate) range_reader_sessions: Arc<Mutex<HashMap<String, epub_svelte_lib::library::ResolvedBook>>>,
    pub(crate) next_range_reader_id: AtomicU64,
}

impl AppState {
    pub(crate) fn new(core: AppBackend) -> Self {
        Self { core, range_reader_sessions: Arc::new(Mutex::new(HashMap::new())), next_range_reader_id: AtomicU64::new(1) }
    }
}
