use crate::executor::BoxedBackendFuture;
use crate::library::LibraryCommand;
#[cfg(not(target_arch = "wasm32"))]
use crate::library::LibraryHandle;
use crate::BackendError;
#[cfg(target_arch = "wasm32")]
use crate::BookId;
use crate::LibraryId;
use crate::LibraryUpdates;
use client_runtime::reply::WireReply;

#[cfg(target_arch = "wasm32")]
use std::rc::Rc as Shared;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc as Shared;

/// A library-scoped client. The host supplies access without exposing application services.
#[derive(Clone)]
pub struct LibraryClient {
    library_id: LibraryId,
    pub(super) transport: Shared<dyn super::ClientTransport>,
}
impl LibraryClient {
    pub fn new(library_id: LibraryId, transport: impl super::ClientTransport + 'static) -> Self {
        Self { library_id, transport: Shared::new(transport) }
    }

    pub fn id(&self) -> &LibraryId {
        &self.library_id
    }

    /// Keep this library's database cache warm while a view selects it.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn retain_database_cache(&self) -> Result<crate::library::handle::DatabaseCacheRetention, BackendError> {
        self.transport.handle().await?.retain_database_cache().await
    }

    // Wire commands and reply decoding belong to this adapter. Consumers use
    // operations whose signatures fix the response type and hide scheduling.
    pub(super) async fn request<T: WireReply>(&self, command: LibraryCommand) -> Result<T, BackendError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.transport.handle().await?.dispatch(command).await.and_then(client_runtime::reply::Reply::take)
        }
        #[cfg(target_arch = "wasm32")]
        {
            client_runtime::wire::decode_worker_payload(self.transport.request(command).await?)
        }
    }

    pub async fn updates(&self) -> Result<(LibraryUpdates, bool), BackendError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.transport.handle().await?.subscribe_updates().await
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.transport.updates().await
        }
    }
}

/// Native hosts resolve an actor asynchronously, retaining their worker lifetime.
/// Creating a client does not require the library to have opened yet.
#[cfg(not(target_arch = "wasm32"))]
pub trait ClientTransport: Send + Sync {
    fn handle(&self) -> BoxedBackendFuture<'_, Result<&LibraryHandle, BackendError>>;
}
#[cfg(not(target_arch = "wasm32"))]
impl ClientTransport for LibraryHandle {
    fn handle(&self) -> BoxedBackendFuture<'_, Result<&LibraryHandle, BackendError>> {
        Box::pin(async move { Ok(self) })
    }
}

#[cfg(target_arch = "wasm32")]
pub type TransportResult<'a, T> = BoxedBackendFuture<'a, Result<T, BackendError>>;

/// Browser hosts route library requests and subscriptions over their worker transport.
#[cfg(target_arch = "wasm32")]
pub trait ClientTransport {
    fn request(&self, command: LibraryCommand) -> TransportResult<'_, client_runtime::wire::WorkerPayload>;
    fn updates(&self) -> TransportResult<'_, (LibraryUpdates, bool)>;
    fn subscribe_book(&self, book_id: BookId) -> TransportResult<'_, (async_channel::Receiver<()>, crate::ResolvedBookData)>;
    fn import_directory(&self, parent_id: crate::DirId, directory: crate::DirectoryImport, create_root: bool) -> TransportResult<'_, Vec<crate::ImportFailure>>;
}
