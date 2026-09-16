use crate::api::library::LibraryClient;
use crate::{
    LibraryId,
    runtime::{AppCommand, LibraryReply, Transport},
};

/// The frontend entry point for running backend operations.
/// Backend implementation objects are deliberately not part of this API.
///
/// ```compile_fail
/// use bokheim_backend::app::{AppBackend, BackendContext};
/// ```
/// ```compile_fail
/// use bokheim_backend::library::LibrarySession;
/// ```
#[derive(Clone)]
pub struct AppClient {
    pub(crate) transport: Transport,
}

impl AppClient {
    pub(crate) async fn request<T: LibraryReply>(&self, command: AppCommand) -> Result<T, String> {
        self.transport.request(command).await
    }

    pub fn library(&self, library_id: LibraryId) -> LibraryClient {
        LibraryClient::new(self.clone(), library_id)
    }
}
