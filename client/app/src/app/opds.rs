//! The account's OPDS feeds. Like libraries, the list lives on the sync
//! server and every device keeps a copy in the registry, so the feeds list
//! offline; changes made offline queue and are sent once signed in.
//! Credentials stay in this device's registry and never reach the server.
//!
//! Native only: a browser may fetch only from servers that allow it, and most
//! OPDS servers do not.

use super::core::*;
use library_backend::library::handle::{LibraryActorCommand, LibraryControl};
use library_backend::{BookId, LibraryBrowseData, LibraryCommand, PendingCommand, ReadyCommand};
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
        self.ensure_opds_libraries()?;
        let mut feeds: Vec<OpdsFeed> = self.read_registry_json(FEEDS)?;
        feeds.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        Ok(feeds.into_iter().map(|feed| OpdsSource { id: feed.id, name: feed.name, url: feed.url }).collect())
    }

    pub(super) fn ensure_opds_libraries(&self) -> Result<(), BackendError> {
        self.with_storage(|this| {
            let feeds: Vec<OpdsFeed> = this.read_registry_json(FEEDS)?;
            let mut changed = false;
            for entry in this.state.registry.entries().map_err(BackendError::operation)? {
                if entry.is_opds() && !feeds.iter().any(|feed| &feed.id == entry.library_id()) && this.state.library_directory.try_get(entry.library_id())?.is_some() {
                    this.state.library_directory.retire(entry.library_id())?;
                    changed = true;
                }
            }
            for feed in feeds {
                match this.state.registry.entry(&feed.id).map_err(BackendError::operation)? {
                    None => {
                        this.state.registry.attach_remote(&feed.id, &feed.name).map_err(BackendError::operation)?;
                        this.state.registry.write_value(&format!("opds-library:{}", feed.id), b"1").map_err(BackendError::operation)?;
                        this.state.registry.make_device_only(&feed.id).map_err(BackendError::operation)?;
                        changed = true;
                    }
                    Some(entry) if !entry.is_opds() => return Err("OPDS source conflicts with an ordinary library".into()),
                    Some(entry) if entry.library_name() != feed.name => {
                        this.state.registry.attach_remote(&feed.id, &feed.name).map_err(BackendError::operation)?;
                        changed = true;
                    }
                    Some(_) => {}
                }
                let url_key = format!("opds-library-url:{}", feed.id);
                if this.state.registry.read_value(&url_key).map_err(BackendError::operation)?.as_deref() != Some(feed.url.as_bytes()) {
                    this.state.library_directory.retire(&feed.id)?;
                    this.state.registry.write_value(&url_key, feed.url.as_bytes()).map_err(BackendError::operation)?;
                }
                if this.state.library_directory.try_get(&feed.id)?.is_none() {
                    if let Err(error) = this.open_registered_library(&feed.id) {
                        log::warn!("Could not open OPDS library {}: {error}", feed.id);
                    }
                }
            }
            if changed {
                this.send_library_list_changed();
            }
            Ok(())
        })
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
        })?;
        self.ensure_opds_libraries()
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
        self.ensure_opds_libraries()?;
        self.send_library_list_changed();
        Ok(AddOpdsFeedResult::Added)
    }

    /// Removes a feed from the account, and this device's credentials for it.
    pub(crate) async fn remove_opds_feed(&self, id: OpdsFeedId) -> Result<(), BackendError> {
        self.record_opds_change(FeedChange::Remove(id))?;
        self.with_storage(move |this| this.state.registry.remove_value(&credentials_key(id)).map_err(BackendError::operation))?;
        if let Err(error) = self.sync_opds_feeds().await {
            log::warn!("the OPDS feed removal is queued: {error}");
        }
        self.state.library_directory.retire(&id)?;
        self.send_library_list_changed();
        Ok(())
    }

    pub(crate) fn set_opds_credentials(&self, id: OpdsFeedId, credentials: OpdsCredentials) -> Result<(), BackendError> {
        self.with_storage(move |this| {
            this.write_registry_json(&credentials_key(id), &credentials)?;
            if let Some(library) = this.state.library_directory.try_get(&id)? {
                library.commands.try_send(LibraryActorCommand::Control(LibraryControl::SetOpdsCredentials(Some(credentials)))).map_err(|_| BackendError::message("OPDS library stopped"))?;
            }
            Ok(())
        })
    }

    /// A feed this device knows, with its credentials here.
    pub(super) fn opds_access(&self, id: OpdsFeedId) -> Result<(OpdsSource, Option<OpdsCredentials>), BackendError> {
        let feeds: Vec<OpdsFeed> = self.read_registry_json(FEEDS)?;
        let feed = feeds.into_iter().find(|feed| feed.id == id).ok_or_else(|| BackendError::classified(Failure::NotFound, "this feed is no longer one of the account's"))?;
        let credentials = self.state.registry.read_value(&credentials_key(id)).map_err(BackendError::operation)?.map(|bytes| serde_json::from_slice(&bytes)).transpose().map_err(BackendError::operation)?;
        Ok((OpdsSource { id: feed.id, name: feed.name, url: feed.url }, credentials))
    }

    /// A page of one of the feeds, fetched from the feed itself.
    pub(crate) async fn browse_opds(&self, query: LibraryBrowseQuery) -> Result<LibraryBrowseData, BackendError> {
        let library_model::BrowseLocation::Opds { source, page } = query.location else { return Err("not an OPDS feed page".into()) };
        self.ensure_opds_libraries()?;
        let (source, credentials) = self.opds_access(source)?;
        let (mut contents, feed) = library_opds::browse(&source, credentials.as_ref(), page, &query.search).await?;
        let books = contents.books.iter().filter_map(|book| library_opds::publication(book.key()).ok()).collect();
        let states: Vec<(BookId, f32, bool)> = self.state.library_directory.get(&source.id)?.dispatch(LibraryCommand::Ready(ReadyCommand::RegisterOpdsBooks { books })).await?.take()?;
        for book in &mut contents.books {
            let id = BookId::for_opds_entry(&book.key().0);
            if let Some((_, progress, downloaded)) = states.iter().find(|(key, _, _)| *key == id) {
                book.progress = *progress;
                book.downloaded = *downloaded;
            }
        }
        Ok(LibraryBrowseData { sections: Vec::new(), contents, path: Vec::new(), ancestors: Vec::new(), language_counts: Vec::new(), format_counts: Vec::new(), feed: Some(feed), page: None })
    }

    /// A listed feed book's cover, as a JPEG `width` pixels wide.
    pub(crate) async fn opds_cover(&self, source: OpdsFeedId, entry: CardKey, width: u32, card: bool) -> Result<Option<Vec<u8>>, BackendError> {
        self.opds_access(source)?;
        let bytes: Option<serde_bytes::ByteBuf> = self.state.library_directory.get(&source)?.dispatch(LibraryCommand::Pending(PendingCommand::OpdsCover { entry, width, card })).await?.take()?;
        Ok(bytes.map(serde_bytes::ByteBuf::into_vec))
    }

    /// A listed feed book's file and the name to import it under.
    pub(crate) async fn download_opds_book(&self, source: OpdsFeedId, entry: CardKey) -> Result<(String, Vec<u8>), BackendError> {
        let (source, credentials) = self.opds_access(source)?;
        library_opds::download(&source, credentials.as_ref(), entry).await
    }
}
