//! Application-owned transport and lifetime adapters for the library client.

use crate::AppClient;
#[cfg(not(target_arch = "wasm32"))]
use crate::BackendError;
#[cfg(not(target_arch = "wasm32"))]
use library_backend::LibraryHandle;
use library_backend::client::{ClientTransport, LibraryClient};
#[cfg(target_arch = "wasm32")]
use library_backend::{ContentHash, DirId, DirectoryImport, ImportFailure, LibraryCommand, ResolvedBookData};
use sync_common::LibraryId;

pub(crate) fn connect(app: AppClient, library_id: LibraryId) -> LibraryClient {
    #[cfg(not(target_arch = "wasm32"))]
    let handle = app.transport.libraries.get(&library_id);
    LibraryClient::new(
        library_id,
        Connection {
            app,
            #[cfg(target_arch = "wasm32")]
            library_id,
            #[cfg(not(target_arch = "wasm32"))]
            handle,
        },
    )
}

struct Connection {
    // Retain the app worker/session for as long as this client can issue requests.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    app: AppClient,
    #[cfg(target_arch = "wasm32")]
    library_id: LibraryId,
    #[cfg(not(target_arch = "wasm32"))]
    handle: Result<LibraryHandle, BackendError>,
}

#[cfg(not(target_arch = "wasm32"))]
impl ClientTransport for Connection {
    fn handle(&self) -> Result<&LibraryHandle, BackendError> {
        self.handle.as_ref().map_err(Clone::clone)
    }
}

#[cfg(target_arch = "wasm32")]
use library_backend::client::TransportResult;
#[cfg(target_arch = "wasm32")]
impl ClientTransport for Connection {
    fn request(&self, command: LibraryCommand) -> TransportResult<'_, client_runtime::wire::WorkerPayload> {
        Box::pin(async move { self.app.transport.library_request(self.library_id, command).await.map_err(|error| error.to_string()) })
    }
    fn updates(&self) -> TransportResult<'_, (library_backend::LibraryUpdates, bool)> {
        Box::pin(async move { self.app.transport.library_updates_transport(self.library_id).await.map(|(updates, scanning)| (updates.into(), scanning)).map_err(|error| error.to_string()) })
    }
    fn subscribe_book(&self, content_hash: ContentHash) -> TransportResult<'_, (async_channel::Receiver<()>, ResolvedBookData)> {
        Box::pin(async move { self.app.transport.book_subscription(self.library_id, content_hash).await.map_err(|error| error.to_string()) })
    }
    fn import_directory(&self, parent_id: DirId, directory: DirectoryImport, create_root: bool) -> TransportResult<'_, Vec<ImportFailure>> {
        Box::pin(async move { self.app.transport.import_selected_directory(self.library_id, parent_id, directory, create_root).await.map_err(|error| error.to_string()) })
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    #[tokio::test]
    async fn native_library_client_retains_worker_without_app_client() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        let root = tempfile::tempdir().unwrap();
        let context = crate::BackendContext::initialize(crate::AppDataLocation::native_path(root.path())).unwrap();
        let backend = crate::app::AppBackend::new(context).unwrap();
        let id = *backend.ensure_import_library().unwrap().library_id();
        let (app, worker) = crate::runtime::native::channel(&backend);
        let library = app.library(id);
        drop(app);
        let stopped = Arc::new(AtomicBool::new(false));
        let finished = stopped.clone();
        let serve = async move {
            worker.serve(backend).await;
            finished.store(true, Ordering::SeqCst);
        };
        let requests = async move {
            tokio::task::yield_now().await;
            assert!(!stopped.load(Ordering::SeqCst));
            assert_eq!(library.book_count().await.unwrap(), 0);
            assert!(!stopped.load(Ordering::SeqCst));
            drop(library);
        };
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            tokio::join!(serve, requests);
        })
        .await
        .unwrap();
    }
}
