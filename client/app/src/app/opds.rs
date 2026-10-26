//! The account's OPDS feeds. Like libraries, the list lives on the sync
//! server and every device keeps a copy in the registry, so the feeds list
//! offline; changes made offline queue and are sent once signed in.
//! Credentials stay in this device's registry and never reach the server.
//!
//! Native only: a browser may fetch only from servers that allow it, and most
//! OPDS servers do not.

use super::core::*;
use library_backend::LibraryBrowseData;
use library_model::{AddOpdsFeedResult, CardKey, LibraryBrowseQuery, OpdsCredentials, OpdsSource};
use sync_common::api::opds_feeds::{OpdsFeed, OpdsFeedId};

const FEEDS: &str = "opds-feeds";
const CHANGES: &str = "opds-feed-changes";

fn credentials_key(id: OpdsFeedId) -> String {
    format!("opds-credentials/{id}")
}

/// A change made on this device, not yet accepted by the server.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
enum FeedChange {
    Put(OpdsFeed),
    Remove(OpdsFeedId),
}

impl FeedChange {
    fn apply(&self, feeds: &mut Vec<OpdsFeed>) {
        match self {
            Self::Put(feed) => match feeds.iter_mut().find(|existing| existing.id == feed.id) {
                Some(existing) => *existing = feed.clone(),
                None => feeds.push(feed.clone()),
            },
            Self::Remove(id) => feeds.retain(|feed| feed.id != *id),
        }
    }
}

impl AppBackend {
    pub(super) fn read_registry_json<T: serde::de::DeserializeOwned + Default>(&self, key: &str) -> Result<T, BackendError> {
        let Some(bytes) = self.state.registry.read_value(key).map_err(BackendError::operation)? else { return Ok(T::default()) };
        serde_json::from_slice(&bytes).map_err(BackendError::operation)
    }

    pub(super) fn write_registry_json(&self, key: &str, value: &impl serde::Serialize) -> Result<(), BackendError> {
        let bytes = serde_json::to_vec(value).map_err(BackendError::operation)?;
        self.state.registry.write_value(key, &bytes).map_err(BackendError::operation)
    }

    /// The feeds, by name, as this device last knew them. Refreshing from the
    /// server first is best effort: offline, the copy is the answer.
    pub(crate) async fn opds_feeds(&self) -> Result<Vec<OpdsSource>, BackendError> {
        if let Err(error) = self.sync_opds_feeds().await {
            log::warn!("OPDS feeds are listed from this device's copy: {error}");
        }
        let mut feeds: Vec<OpdsFeed> = self.read_registry_json(FEEDS)?;
        feeds.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        Ok(feeds.into_iter().map(|feed| OpdsSource { id: feed.id, name: feed.name, url: feed.url }).collect())
    }

    /// Sends the changes queued here, then takes the server's list, with any
    /// change still unsent applied on top.
    pub(super) async fn sync_opds_feeds(&self) -> Result<(), BackendError> {
        if self.current_account()?.is_none() {
            return Ok(());
        }
        let changes: Vec<FeedChange> = self.read_registry_json(CHANGES)?;
        let mut settled = 0;
        for change in &changes {
            let change = change.clone();
            let result = self
                .state
                .account
                .request(move |session| {
                    let change = change.clone();
                    Box::pin(async move {
                        match change {
                            FeedChange::Put(feed) => account_client::put_opds_feed(session, &feed).await.map(drop),
                            FeedChange::Remove(id) => account_client::delete_opds_feed(session, id).await,
                        }
                    })
                })
                .await;
            match result {
                Ok(()) => settled += 1,
                // Refused for good, such as a feed another device removed:
                // the server's list below is the answer.
                Err(account_client::AccountSessionError::Authentication(error)) if error.is_rejected() => settled += 1,
                Err(error) => {
                    log::warn!("OPDS feed changes remain queued: {error}");
                    break;
                }
            }
        }
        let remote = self.state.account.request(|session| Box::pin(account_client::opds_feeds(session))).await.map_err(super::account::account_error)?;
        let settled = changes[..settled].to_vec();
        self.with_storage(move |this| {
            let mut pending: Vec<FeedChange> = this.read_registry_json(CHANGES)?;
            // Changes queued during the requests follow the ones just sent.
            if pending.starts_with(&settled) {
                pending.drain(..settled.len());
            }
            let mut feeds = remote;
            for change in &pending {
                change.apply(&mut feeds);
            }
            this.write_registry_json(CHANGES, &pending)?;
            this.write_registry_json(FEEDS, &feeds)
        })
    }

    fn record_opds_change(&self, change: FeedChange) -> Result<(), BackendError> {
        self.with_storage(move |this| {
            let mut feeds: Vec<OpdsFeed> = this.read_registry_json(FEEDS)?;
            change.apply(&mut feeds);
            let mut pending: Vec<FeedChange> = this.read_registry_json(CHANGES)?;
            pending.push(change);
            this.write_registry_json(FEEDS, &feeds)?;
            this.write_registry_json(CHANGES, &pending)
        })
    }

    /// Adds the feed at `url`, named `name` or else by its own title. The
    /// feed is fetched once first, so an address that is not a feed, or
    /// credentials it refuses, are refused here.
    pub(crate) async fn add_opds_feed(&self, url: String, name: Option<String>, credentials: Option<OpdsCredentials>) -> Result<AddOpdsFeedResult, BackendError> {
        let Some(title) = library_opds::feed_title(&url, credentials.as_ref()).await? else {
            return Ok(AddOpdsFeedResult::SignInRequired);
        };
        let name = name.map(|name| name.trim().to_owned()).filter(|name| !name.is_empty()).unwrap_or(title);
        let feed = OpdsFeed { id: uuid::Uuid::new_v4(), name, url: url.trim().to_owned() };
        if let Some(credentials) = credentials {
            self.set_opds_credentials(feed.id, credentials)?;
        }
        self.record_opds_change(FeedChange::Put(feed))?;
        if let Err(error) = self.sync_opds_feeds().await {
            log::warn!("the new OPDS feed is queued: {error}");
        }
        Ok(AddOpdsFeedResult::Added)
    }

    /// Removes a feed from the account, and this device's credentials for it.
    pub(crate) async fn remove_opds_feed(&self, id: OpdsFeedId) -> Result<(), BackendError> {
        self.record_opds_change(FeedChange::Remove(id))?;
        self.with_storage(move |this| this.state.registry.remove_value(&credentials_key(id)).map_err(BackendError::operation))?;
        if let Err(error) = self.sync_opds_feeds().await {
            log::warn!("the OPDS feed removal is queued: {error}");
        }
        Ok(())
    }

    pub(crate) fn set_opds_credentials(&self, id: OpdsFeedId, credentials: OpdsCredentials) -> Result<(), BackendError> {
        self.with_storage(move |this| this.write_registry_json(&credentials_key(id), &credentials))
    }

    /// A feed this device knows, with its credentials here.
    fn opds_access(&self, id: OpdsFeedId) -> Result<(OpdsSource, Option<OpdsCredentials>), BackendError> {
        let feeds: Vec<OpdsFeed> = self.read_registry_json(FEEDS)?;
        let feed = feeds.into_iter().find(|feed| feed.id == id).ok_or_else(|| BackendError::classified(Failure::NotFound, "this feed is no longer one of the account's"))?;
        let credentials = self.state.registry.read_value(&credentials_key(id)).map_err(BackendError::operation)?.map(|bytes| serde_json::from_slice(&bytes)).transpose().map_err(BackendError::operation)?;
        Ok((OpdsSource { id: feed.id, name: feed.name, url: feed.url }, credentials))
    }

    /// A page of one of the feeds, fetched from the feed itself.
    pub(crate) async fn browse_opds(&self, query: LibraryBrowseQuery) -> Result<LibraryBrowseData, BackendError> {
        let library_model::BrowseLocation::Opds { source, page } = query.location else { return Err("not an OPDS feed page".into()) };
        let (source, credentials) = self.opds_access(source)?;
        let (contents, feed) = library_opds::browse(&source, credentials.as_ref(), page, &query.search).await?;
        Ok(LibraryBrowseData { sections: Vec::new(), contents, path: Vec::new(), ancestors: Vec::new(), language_counts: Vec::new(), format_counts: Vec::new(), feed: Some(feed), page: None })
    }

    /// A listed feed book's cover, as a JPEG `width` pixels wide.
    pub(crate) async fn opds_cover(&self, source: OpdsFeedId, entry: CardKey, width: u32, card: bool) -> Result<Option<Vec<u8>>, BackendError> {
        let (source, credentials) = self.opds_access(source)?;
        Ok(if card { library_opds::card_cover(&source, credentials.as_ref(), entry, width).await } else { library_opds::cover(&source, credentials.as_ref(), entry, width).await })
    }

    /// A listed feed book's file and the name to import it under.
    pub(crate) async fn download_opds_book(&self, source: OpdsFeedId, entry: CardKey) -> Result<(String, Vec<u8>), BackendError> {
        let (source, credentials) = self.opds_access(source)?;
        library_opds::download(&source, credentials.as_ref(), entry).await
    }
}
