//! Server registration uses the registry's current name, ownership, and policy.
use super::LibraryService;
use crate::account::account_error;
use crate::{BackendError, LibraryId};
use account_client::{AuthError, create_library_with_storage};
use std::sync::Arc;

impl LibraryService {
    /// The existing account-maintenance loop retries offline or failed creations.
    pub(crate) async fn publish_pending_library_registrations(self: &Arc<Self>) -> Result<(), BackendError> {
        let mut failure = None;
        for handle in self.open_handles() {
            let id = *handle.id();
            if let Err(error) = self.ensure_remote_library(id).await {
                log::warn!("registration for library {id} will retry: {error}");
                failure.get_or_insert(error);
            }
        }
        failure.map_or(Ok(()), Err)
    }

    /// Foreground operations that require server books use the same path as
    /// background maintenance. Retries use the same library ID.
    pub(crate) async fn ensure_remote_library(self: &Arc<Self>, id: LibraryId) -> Result<(), BackendError> {
        let prepared = self.run(move |this| {
            let entry = this.entry_on_storage(&id)?;
            if this.config.registry.device_only(&id).map_err(BackendError::operation)? {
                return Ok(None);
            }
            let identity = this.config.account.require().map_err(account_error)?.identity();
            let storage = this.config.registry.storage(&id).map_err(BackendError::operation)?;
            Ok(Some((this.handle(&id)?, identity, entry.library_name().to_owned(), storage)))
        }).await?;
        let Some((handle, identity, name, storage)) = prepared else { return Ok(()) };
        if handle.remote_registration_identity().await?.as_deref() == Some(identity.as_str()) {
            return Ok(());
        }
        let registration = self.config.account.request(|session| {
            let matches = session.identity() == identity;
            let name = name.clone();
            let storage = storage.clone();
            Box::pin(async move {
                if !matches {
                    return Err(AuthError::SignInRequired("the account changed during library registration"));
                }
                self.check_library_account(&id, &session).await?;
                create_library_with_storage(&session, id, name, storage).await.map(|_| ())
            })
        });
        tokio::select! {
            biased;
            _ = handle.closed() => return Err(BackendError::message("library was retired during registration")),
            result = registration => result.map_err(account_error)?,
        }
        handle.mark_remote_registered(identity).await
    }
}
