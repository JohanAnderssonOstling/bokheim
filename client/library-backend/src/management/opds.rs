//! OPDS validation feeds into the shared registered-library lifecycle.
use super::LibraryService;
use crate::BackendError;
use account_client::Url;
use library_model::{AddOpdsFeedResult, OpdsCredentials};
use sync_common::storage::{LibraryKind, LibraryStorage};

impl LibraryService {
    pub async fn add_opds_library(&self, url: String, name: Option<String>, credentials: Option<OpdsCredentials>) -> Result<AddOpdsFeedResult, BackendError> {
        let Some(feed) = library_opds::validated_source(url, name, credentials.as_ref()).await? else { return Ok(AddOpdsFeedResult::SignInRequired) };
        self.run(move |this| {
            let kind = LibraryKind::Opds { url: Url::parse(&feed.url).map_err(BackendError::operation)? };
            let entry = this.register(&feed.name, None, LibraryStorage::Local, kind)?;
            if let Some(credentials) = credentials {
                this.state.catalog.set_credentials(*entry.library_id(), credentials)?;
            }
            this.finish_created(entry)?;
            Ok(())
        })
        .await?;
        Ok(AddOpdsFeedResult::Added)
    }
}
